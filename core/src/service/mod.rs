use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use self::backup::{BackupData, BackupInstalledAddon, BackupManualDependency};
use self::fs_util::{fs_delete_addon, fs_read_addon};
use self::result::*;
use crate::addons::{Addon, get_root_dir};
use crate::api::ApiClient;
use crate::config::{self, Config, HmConfigUpdate, TTCRegion, TtcConfigUpdate};
use crate::error::{self, Result};
use entity::addon as DbAddon;
use entity::addon_dependency as AddonDep;
use entity::addon_detail as AddonDetail;
use entity::addon_dir as AddonDir;
use entity::addon_image as AddonImage;
use entity::category as Category;
use entity::category_parent as CategoryParent;
use entity::game_compatibility as GameCompat;
use entity::installed_addon as InstalledAddon;
use entity::manual_dependency as ManualDependency;
use migration::{Condition, Migrator, MigratorTrait};

use futures::StreamExt;
use lazy_async_promise::ImmediateValuePromise;
use md5::{Digest, Md5};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions, ConnectionTrait,
    DatabaseConnection, DbBackend, DbErr, EntityTrait, FromQueryResult, IntoActiveModel, JoinType,
    ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
    Statement, TransactionTrait, Value,
};
use snafu::{OptionExt, ResultExt, ensure};
use tempfile::NamedTempFile;
use tracing::log::{self, error, info, warn};
use version_compare::Version;
use walkdir::WalkDir;
use zip::ZipArchive;

mod backup;
mod fs_util;
pub mod result;

const TTC_NA_DOMAIN: &str = "us.tamrieltradecentre.com";
const TTC_EU_DOMAIN: &str = "eu.tamrieltradecentre.com";

/// Safe upper bound for SQLite bound parameters per statement (SQLITE_MAX_VARIABLE_NUMBER).
/// Defaults to 32766 on 3.32.0+ (bundled by libsqlite3-sys); 32000 leaves headroom.
/// Only relevant for `IN (?, ?, ...)` clauses — row inserts run as per-row prepared statements.
const SQLITE_MAX_VARS: usize = 32000;

#[derive(Debug, Clone, Default)]
pub struct AddonService {
    pub api: ApiClient,
    pub config: config::Config,
    pub db: DatabaseConnection,
    pub errors: Arc<Mutex<Vec<ErrorRecord>>>,
}
impl AddonService {
    pub async fn new() -> Self {
        // setup config
        let mut config = Config::load();

        // init api/download client
        let mut client = ApiClient::default();
        if config.file_list.is_empty() {
            client.update_endpoints().await.unwrap();
            info!("Saving config");
            client.file_details_url.clone_into(&mut config.file_details);
            client.file_list_url.clone_into(&mut config.file_list);
            client.list_files_url.clone_into(&mut config.list_files);
            client
                .category_list_url
                .clone_into(&mut config.category_list);
            config.save().unwrap();
        } else {
            client.update_endpoints_from_config(&config);
        }

        // create db file if not exists
        let db_file = Config::default_db_path();
        if !db_file.exists() {
            File::create(&db_file).unwrap();
        }
        // setup database connection and apply migrations if needed
        let mut opt = ConnectOptions::new(format!("sqlite://{}", db_file.to_string_lossy()));
        opt.sqlx_logging_level(log::LevelFilter::Debug); // Setting SQLx log level
        let db = sea_orm::Database::connect(opt).await.unwrap();
        Migrator::up(&db, None).await.unwrap();

        Self {
            api: client,
            config,
            db,
            errors: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn record_error(&self, context: impl Into<String>, message: impl ToString) {
        let record = ErrorRecord {
            timestamp: chrono::Utc::now(),
            context: context.into(),
            message: message.to_string(),
        };
        error!("{}: {}", record.context, record.message);
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(record);
        }
    }

    pub fn errors(&self) -> Vec<ErrorRecord> {
        self.errors.lock().map(|e| e.clone()).unwrap_or_default()
    }

    pub fn clear_errors(&self) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.clear();
        }
    }

    pub fn install(&self, addon_id: i32, update: bool) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            if let Err(e) = service.p_install(addon_id, update).await {
                let action = if update { "updating" } else { "installing" };
                let label = service.addon_label(addon_id).await;
                service.record_error(format!("Error {action} {label}"), e);
            }
            Ok(())
        })
    }

    /// Best-effort human-readable label for an addon, e.g. "NinjaWicca UI (#4551)".
    /// Falls back to "addon {id}" when the name can't be looked up.
    async fn addon_label(&self, addon_id: i32) -> String {
        match DbAddon::Entity::find_by_id(addon_id).one(&self.db).await {
            Ok(Some(a)) => format!("{} (#{addon_id})", a.name),
            _ => format!("addon {addon_id}"),
        }
    }
    async fn p_install(&self, addon_id: i32, update: bool) -> Result<()> {
        self.p_update_addon_details(addon_id).await?;
        let entry = DbAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        let entry = entry.context(error::AddonNotFoundSnafu { id: addon_id })?;
        let installed_entry = InstalledAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;

        if let Some(installed_entry) = installed_entry
            && installed_entry.version == entry.version
            && !update
        {
            info!("Addon {} is already installed and up to date", entry.name);
            return Ok(());
        }

        if update {
            info!("Updating addon: {addon_id}");
        } else {
            info!("Installing addon: {addon_id}");
        }

        let download = entry
            .download
            .clone()
            .context(error::AddonMissingDownloadUrlSnafu { id: addon_id })?;
        let installed = self.fs_download_addon(&download, entry.md5).await?;
        let installed_entry = InstalledAddon::ActiveModel {
            addon_id: ActiveValue::Set(addon_id),
            version: ActiveValue::Set(entry.version.to_string()),
            date: ActiveValue::Set(entry.date.to_string()),
        };

        let result = InstalledAddon::Entity::insert(installed_entry)
            .on_conflict(
                OnConflict::column(InstalledAddon::Column::AddonId)
                    .update_columns([
                        InstalledAddon::Column::Date,
                        InstalledAddon::Column::Version,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await;
        check_db_result(result)?;

        // get addon IDs from dependency dirs, there may be more than on for each directory
        if !installed.depends_on.is_empty() {
            let deps = installed.depends_on.iter().map(|x| AddonDep::ActiveModel {
                addon_id: ActiveValue::Set(addon_id),
                dependency_dir: ActiveValue::Set(x.to_owned()),
            });
            // insert all dependencies
            let result = AddonDep::Entity::insert_many(deps)
                .on_conflict(
                    OnConflict::columns([
                        AddonDep::Column::AddonId,
                        AddonDep::Column::DependencyDir,
                    ])
                    .do_nothing()
                    .to_owned(),
                )
                .exec(&self.db)
                .await;
            check_db_result(result)?;
        }

        // leave check for missing depenency options after install to client
        Ok(())
    }

    pub async fn upgrade(&mut self) -> Result<UpdateResult> {
        // update all addons that have a newer date than installed date
        let updates = InstalledAddon::Entity::find()
            .columns([
                DbAddon::Column::Id,
                DbAddon::Column::CategoryId,
                DbAddon::Column::Name,
            ])
            .column_as(Expr::value(1), "installed")
            .inner_join(DbAddon::Entity)
            .filter(
                Condition::any()
                    .add(
                        Expr::col((InstalledAddon::Entity, InstalledAddon::Column::Date))
                            .lt(Expr::col((DbAddon::Entity, DbAddon::Column::Date))),
                    )
                    .add(
                        Expr::col((InstalledAddon::Entity, InstalledAddon::Column::Version))
                            .ne(Expr::col((DbAddon::Entity, DbAddon::Column::Version))),
                    ),
            )
            .into_model::<AddonDetails>()
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        for _update in updates.iter() {
            // self.install(update.id, true).await.unwrap(); // TODO: add back?
        }

        let result = UpdateResult {
            ..Default::default()
        };

        Ok(result)
    }

    pub fn update(&mut self, upgrade_all: bool) -> ImmediateValuePromise<UpdateResult> {
        let mut service = self.clone();
        ImmediateValuePromise::new(async move {
            // update categories
            service.update_categories().await?;

            // update addons
            let file_list = service.api.get_file_list().await?;

            let mut insert_addons = vec![];
            let mut insert_addon_dirs = vec![];
            let mut insert_compats = vec![];
            let mut insert_imgs = vec![];
            let mut addon_ids = vec![];
            for list_item in file_list.iter() {
                let addon_id: i32 = match list_item.id.parse() {
                    Ok(id) => id,
                    Err(e) => {
                        warn!("Skipping addon with non-integer id {:?}: {e}", list_item.id);
                        continue;
                    }
                };
                addon_ids.push(addon_id);
                let addon = DbAddon::ActiveModel {
                    id: ActiveValue::Set(addon_id),
                    category_id: ActiveValue::Set(list_item.category.to_owned()),
                    version: ActiveValue::Set(list_item.version.to_owned()),
                    date: ActiveValue::Set(list_item.date.to_string()),
                    name: ActiveValue::Set(list_item.name.to_owned()),
                    author_name: ActiveValue::Set(Some(list_item.author_name.to_owned())),
                    file_info_url: ActiveValue::Set(Some(list_item.file_info_url.to_owned())),
                    download_total: ActiveValue::Set(Some(list_item.download_total.to_owned())),
                    download_monthly: ActiveValue::Set(Some(list_item.download_monthly.to_owned())),
                    favorite_total: ActiveValue::Set(Some(list_item.favorite_total.to_owned())),
                    ..Default::default()
                };

                // AddOn Directories
                for addon_dir in list_item.directories.iter() {
                    let addon_dir_model = AddonDir::ActiveModel {
                        addon_id: ActiveValue::Set(addon_id),
                        dir: ActiveValue::Set(addon_dir.to_string()),
                    };
                    insert_addon_dirs.push(addon_dir_model);
                }

                // Game Compatibility
                if let Some(compats) = &list_item.compatibility {
                    for (index, item) in compats.iter().enumerate() {
                        let Ok(idx) = index.try_into() else {
                            warn!(
                                "Skipping compat entry {index} for addon {addon_id} (index out of range)"
                            );
                            continue;
                        };
                        insert_compats.push(GameCompat::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            id: ActiveValue::Set(idx),
                            version: ActiveValue::Set(item.version.to_owned()),
                            name: ActiveValue::Set(item.name.to_owned()),
                        });
                    }
                }

                // AddOn Images
                if let (Some(thumbs), Some(imgs)) = (&list_item.image_thumbnails, &list_item.images)
                {
                    let it = thumbs.iter().zip(imgs.iter());
                    for (i, (thumb, img)) in it.enumerate() {
                        let Ok(idx) = i.try_into() else {
                            warn!("Skipping image {i} for addon {addon_id} (index out of range)");
                            continue;
                        };
                        insert_imgs.push(AddonImage::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            index: ActiveValue::Set(idx),
                            thumbnail: ActiveValue::Set(thumb.to_owned()),
                            image: ActiveValue::Set(img.to_owned()),
                        })
                    }
                }

                insert_addons.push(addon);
            }
            let txn = service.db.begin().await.context(error::DbPutSnafu)?;

            let addon_on_conflict = OnConflict::column(DbAddon::Column::Id)
                .update_columns([
                    DbAddon::Column::CategoryId,
                    DbAddon::Column::Version,
                    DbAddon::Column::Date,
                    DbAddon::Column::Name,
                    DbAddon::Column::AuthorName,
                    DbAddon::Column::FileInfoUrl,
                    DbAddon::Column::DownloadTotal,
                    DbAddon::Column::DownloadMonthly,
                    DbAddon::Column::FavoriteTotal,
                ])
                .to_owned();
            for addon in insert_addons {
                DbAddon::Entity::insert(addon)
                    .on_conflict(addon_on_conflict.clone())
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }

            // delete + re-insert dirs/compat/images for the IDs we just touched.
            // is_in still bind-counts the IDs, so chunk to stay under SQLITE_MAX_VARS.
            for id_chunk in addon_ids.chunks(SQLITE_MAX_VARS) {
                AddonDir::Entity::delete_many()
                    .filter(AddonDir::Column::AddonId.is_in(id_chunk.iter().copied()))
                    .exec(&txn)
                    .await
                    .context(error::DbDeleteSnafu)?;
                GameCompat::Entity::delete_many()
                    .filter(GameCompat::Column::AddonId.is_in(id_chunk.iter().copied()))
                    .exec(&txn)
                    .await
                    .context(error::DbDeleteSnafu)?;
                AddonImage::Entity::delete_many()
                    .filter(AddonImage::Column::AddonId.is_in(id_chunk.iter().copied()))
                    .exec(&txn)
                    .await
                    .context(error::DbDeleteSnafu)?;
            }

            for dir in insert_addon_dirs {
                AddonDir::Entity::insert(dir)
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }
            for compat in insert_compats {
                GameCompat::Entity::insert(compat)
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }
            for img in insert_imgs {
                AddonImage::Entity::insert(img)
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }

            txn.commit().await.context(error::DbPutSnafu)?;

            let mut result = UpdateResult::default();
            if upgrade_all {
                result = service.upgrade().await?;
            }
            Ok(result)
        })
    }

    async fn p_update_addon_details(&self, id: i32) -> Result<()> {
        // check addon_detail not present or out of date
        let addon = DbAddon::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        let addon_detail = AddonDetail::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        if let (Some(addon), Some(addon_detail)) = (&addon, &addon_detail)
            && addon.version == addon_detail.version.clone().unwrap_or_default()
            && addon.file_name.is_some()
            && addon.download.is_some()
        {
            return Ok(());
        }

        info!("Downloading addon details for addon: {id}");

        let file_details = self.api.get_file_details(id).await?;
        let record = AddonDetail::ActiveModel {
            id: ActiveValue::Set(id),
            description: ActiveValue::Set(Some(file_details.description)),
            change_log: ActiveValue::Set(Some(file_details.change_log)),
            version: ActiveValue::Set(Some(file_details.version)),
        };

        let Some(addon) = DbAddon::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?
        else {
            warn!("No addon found with id {id} when updating details");
            return Ok(());
        };
        let mut active: DbAddon::ActiveModel = addon.into_active_model();
        active.md5 = Set(Some(file_details.md5.to_owned()));
        active.file_name = Set(Some(file_details.file_name.to_owned()));
        active.download = Set(Some(file_details.download_url.to_owned()));
        active.update(&self.db).await.context(error::DbPutSnafu)?;

        AddonDetail::Entity::insert(record)
            .on_conflict(
                OnConflict::column(AddonDetail::Column::Id)
                    .update_columns([
                        AddonDetail::Column::Description,
                        AddonDetail::Column::ChangeLog,
                        AddonDetail::Column::Version,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await
            .context(error::DbPutSnafu)?;

        Ok(())
    }
    pub fn update_addon_details(&self, id: i32) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            if let Err(e) = service.p_update_addon_details(id).await {
                let label = service.addon_label(id).await;
                service.record_error(format!("Error updating details for {label}"), e);
            }
            Ok(())
        })
    }

    async fn update_categories(&self) -> Result<()> {
        info!("Updating categories");
        let categories = self.api.get_categories().await?;
        let mut insert_categories = vec![];
        let mut category_parents = vec![];
        for category in categories.iter() {
            let Ok(cat_id) = category.id.parse() else {
                warn!("Skipping category with non-integer id {:?}", category.id);
                continue;
            };
            let file_count = category.file_count.parse().ok();
            let db_category = Category::ActiveModel {
                id: ActiveValue::Set(cat_id),
                title: ActiveValue::Set(category.title.to_owned()),
                icon: ActiveValue::Set(Some(category.icon.to_owned())),
                file_count: ActiveValue::Set(file_count),
            };
            insert_categories.push(db_category);

            for parent_id in category.parent_ids.iter() {
                let Ok(parent) = parent_id.parse() else {
                    warn!("Skipping non-integer parent id {parent_id:?} for category {cat_id}");
                    continue;
                };
                let db_parent = CategoryParent::ActiveModel {
                    id: ActiveValue::Set(cat_id),
                    parent_id: ActiveValue::Set(parent),
                };
                category_parents.push(db_parent);
            }
        }
        let txn = self.db.begin().await.context(error::DbPutSnafu)?;

        let category_on_conflict = OnConflict::column(Category::Column::Id)
            .update_columns([
                Category::Column::Title,
                Category::Column::Icon,
                Category::Column::FileCount,
            ])
            .to_owned();
        for category in insert_categories {
            let result = Category::Entity::insert(category)
                .on_conflict(category_on_conflict.clone())
                .exec(&txn)
                .await;
            check_db_result(result)?;
        }

        let parent_on_conflict =
            OnConflict::columns([CategoryParent::Column::Id, CategoryParent::Column::ParentId])
                .do_nothing()
                .to_owned();
        for parent in category_parents {
            let result = CategoryParent::Entity::insert(parent)
                .on_conflict(parent_on_conflict.clone())
                .exec(&txn)
                .await;
            check_db_result(result)?;
        }

        txn.commit().await.context(error::DbPutSnafu)?;
        Ok(())
    }

    pub fn remove(&self, addon_id: i32) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Removing addon with id: {addon_id}");
            let Some(addon) = DbAddon::Entity::find_by_id(addon_id)
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?
            else {
                warn!("Not a valid addon ID!");
                return Ok(());
            };
            let Some(installed_addon) = addon
                .find_related(InstalledAddon::Entity)
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?
            else {
                warn!("Addon not installed!");
                return Ok(());
            };
            // get installed dirs
            let installed_dirs = addon
                .find_related(AddonDir::Entity)
                .filter(AddonDir::Column::Dir.ne("")) // don't delete main AddOns dir
                .all(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            installed_addon
                .delete(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // delete any manual dependency entities for this addon
            ManualDependency::Entity::delete_many()
                .filter(ManualDependency::Column::SatisfiedBy.eq(addon_id))
                .exec(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // delete installed addon directories
            match fs_delete_addon(&service.get_addon_dir(), &installed_dirs) {
                Ok(_) => {
                    info!("Removed addon {}", addon.name);
                }
                Err(err) => {
                    warn!("{err}");
                }
            }

            Ok(())
        })
    }

    pub fn clear_installed(&self) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            InstalledAddon::Entity::delete_many().exec(&db).await?;
            Ok(())
        })
    }

    pub fn search(&self, search_string: String) -> ImmediateValuePromise<Vec<AddonShowDetails>> {
        // let mut results = vec![];
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let addons = DbAddon::Entity::find()
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(Category::Column::Title, "category")
                .column_as(Expr::value("NULL"), "description")
                .column_as(Expr::value("NULL"), "change_log")
                .column_as(Expr::value("NULL"), "game_compat_version")
                .column_as(Expr::value("NULL"), "game_compat_name")
                .column_as(Category::Column::Icon, "category_icon")
                .inner_join(Category::Entity)
                .left_join(InstalledAddon::Entity)
                .filter(DbAddon::Column::Name.like(format!("%{search_string}%").as_str()))
                .order_by_desc(DbAddon::Column::Date)
                .into_model::<AddonShowDetails>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            Ok(addons)
        })
    }

    pub async fn get_installed_addon_count(&self) -> Result<u64> {
        let count = InstalledAddon::Entity::find()
            .count(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        Ok(count)
    }

    pub fn get_installed_addons(&self) -> ImmediateValuePromise<Vec<AddonShowDetails>> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            // 1. Check for untracked installed addons
            info!("Checking for untracked addons");
            // grab every dir under Addons/
            let walker = WalkDir::new(addon_dir.clone())
                .min_depth(1)
                .max_depth(1)
                .into_iter();
            let addon_dirs: Vec<String> = walker
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    e.path()
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                })
                .collect();
            let mut addon_versions = HashMap::new();
            for addon_dir in addon_dirs {
                addon_versions.insert(addon_dir, "0".to_string());
            }

            // now check every txt and addon file matching the directory name to get the installed version
            let parser = eso_addon_manifest::AddonManifestParser::default();
            let walker = WalkDir::new(&addon_dir)
                .min_depth(2)
                .max_depth(4)
                .into_iter();
            let mut manifest_deps: HashMap<String, Vec<String>> = HashMap::new();
            let mut nested_dirs: HashMap<String, Vec<String>> = HashMap::new();
            for entry in walker.filter_map(|e| e.ok()).filter(|e| {
                let path = e.path();
                let Some(parent_name) = path.parent().and_then(|p| p.file_name()) else {
                    return false;
                };
                let Some(stem) = path.file_stem() else {
                    return false;
                };
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                    return false;
                };
                path.is_file() && parent_name == stem && ["txt", "addon"].contains(&ext)
            }) {
                let parent = entry
                    .path()
                    .parent()
                    .expect("walker min_depth(2) guarantees a parent");
                let Some(dir_name) = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(String::from)
                else {
                    continue;
                };
                let stripped = entry
                    .path()
                    .strip_prefix(&addon_dir)
                    .expect("walker rooted at addon_dir");
                let Some(top_level) = stripped
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str())
                    .map(String::from)
                else {
                    continue;
                };
                let Some(path_str) = entry.path().to_str() else {
                    warn!("Skipping non-UTF-8 manifest path {:?}", entry.path());
                    continue;
                };
                let manifest = match parser.parse(path_str, None) {
                    Ok(manifest) => manifest,
                    Err(err) => {
                        warn!("{}", err);
                        continue;
                    }
                };
                if dir_name != top_level {
                    // nested sub-addon, e.g. CombatMetrics/CombatMetricsFightData/
                    nested_dirs.entry(top_level).or_default().push(dir_name);
                    continue;
                }
                let new_version = manifest.version.unwrap_or("0".to_string());
                if let Some(version) = addon_versions.get(&manifest.title) {
                    let current = Version::from(version);
                    let candidate = Version::from(&new_version);
                    if let (Some(current), Some(candidate)) = (current, candidate)
                        && current < candidate
                    {
                        addon_versions.insert(manifest.title, new_version);
                    }
                }
                if !manifest.depends_on.is_empty() {
                    manifest_deps.insert(
                        dir_name,
                        manifest.depends_on.into_iter().map(|d| d.title).collect(),
                    );
                }
            }

            // check database for any untracked addons (not installed, AddonDir matches, and AddonDir not installed by another)
            let keys: Vec<String> = addon_versions.keys().cloned().collect();
            let placeholders = vec!["?"; keys.len()].join(", ");
            let sql = format!(
                r#"SELECT
	a.id,
	a.name,
	d.dir,
	a.download_monthly
from addon a
inner join addon_dir d on a.id = d.addon_id
inner join (
SELECT
	d.dir,
	max(cast(a.download_monthly as int)) download_monthly
from addon_dir d
inner join addon a on d.addon_id = a.id
where d.dir in ({})
GROUP by d.dir) m on d.dir = m.dir and a.download_monthly = m.download_monthly
left outer join installed_addon i on a.id = i.addon_id
where i.addon_id is null
    and d.dir not in (
        select d.dir
        from addon_dir d
        inner join installed_addon i on d.addon_id = i.addon_id
    )"#,
                placeholders
            );

            let db_results = db
                .query_all(Statement::from_sql_and_values(
                    sea_orm::DatabaseBackend::Sqlite,
                    &sql,
                    keys.iter().map(|k| k.into()).collect::<Vec<_>>(),
                ))
                .await
                .context(error::DbGetSnafu)?;
            let now = format!("{}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
            let mut inserts: Vec<InstalledAddon::ActiveModel> = Vec::new();
            let mut pending_by_addon_id: HashMap<i32, Vec<(String, String)>> = HashMap::new();

            for x in db_results.iter() {
                let addon_id: i32 = x.try_get_by(0).context(error::DbGetSnafu)?;
                let addon_name: String = x.try_get_by(1).context(error::DbGetSnafu)?;
                let dir: String = x.try_get_by(2).context(error::DbGetSnafu)?;

                pending_by_addon_id
                    .entry(addon_id)
                    .or_default()
                    .push((addon_name.clone(), dir.clone()));

                inserts.push(InstalledAddon::ActiveModel {
                    addon_id: ActiveValue::Set(addon_id),
                    version: ActiveValue::Set(
                        addon_versions
                            .get(&dir)
                            .unwrap_or(&"0".to_string())
                            .to_string(),
                    ),
                    date: ActiveValue::Set(now.to_string()),
                });
            }

            for (addon_id, matches) in pending_by_addon_id
                .iter()
                .filter(|(_, matches)| matches.len() > 1)
            {
                warn!(
                    "Duplicate installed-addon detection candidate for addon_id {}: {} matches",
                    addon_id,
                    matches.len()
                );

                for (addon_name, dir) in matches {
                    warn!(
                        "  duplicate candidate addon_id={} name={:?} dir={:?}",
                        addon_id, addon_name, dir
                    );
                }
            }
            // 2. insert as installed, update checks will handle the rest
            if !inserts.is_empty() {
                info!("Adding {} untracked addons", inserts.len());
                InstalledAddon::Entity::insert_many(inserts)
                    .exec(&db)
                    .await?;
            }

            // Register nested sub-addon dirs under the parent's addon_id. ESOUI's
            // file list only enumerates the top-level dir, so deps targeting bundled
            // sub-addons (e.g. CombatMetrics depending on CombatMetricsFightData)
            // would otherwise resolve as missing.
            if !nested_dirs.is_empty() {
                let parent_dirs: Vec<String> = nested_dirs.keys().cloned().collect();
                let parent_to_addon = resolve_dirs_to_addons(&db, &parent_dirs).await?;

                let mut dir_inserts: Vec<AddonDir::ActiveModel> = Vec::new();
                for (parent, subs) in nested_dirs {
                    let Some(&addon_id) = parent_to_addon.get(&parent) else {
                        continue;
                    };
                    for sub in subs {
                        dir_inserts.push(AddonDir::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            dir: ActiveValue::Set(sub),
                        });
                    }
                }
                if !dir_inserts.is_empty() {
                    let result = AddonDir::Entity::insert_many(dir_inserts)
                        .on_conflict(
                            OnConflict::columns([AddonDir::Column::AddonId, AddonDir::Column::Dir])
                                .do_nothing()
                                .to_owned(),
                        )
                        .exec(&db)
                        .await;
                    check_db_result(result)?;
                }
            }

            // p_install populates addon_dependency only on the download path, so
            // addons installed via Minion or by hand had no dep rows. Refresh from
            // manifests for everything we could parse.
            if !manifest_deps.is_empty() {
                let manifest_dirs: Vec<String> = manifest_deps.keys().cloned().collect();
                let addon_for_dir = resolve_dirs_to_addons(&db, &manifest_dirs).await?;

                let affected: Vec<i32> = addon_for_dir.values().copied().collect();
                if !affected.is_empty() {
                    AddonDep::Entity::delete_many()
                        .filter(AddonDep::Column::AddonId.is_in(affected))
                        .exec(&db)
                        .await
                        .context(error::DbDeleteSnafu)?;
                }

                let mut dep_inserts: Vec<AddonDep::ActiveModel> = Vec::new();
                for (dir, deps) in manifest_deps {
                    let Some(&addon_id) = addon_for_dir.get(&dir) else {
                        continue;
                    };
                    for dep_title in deps {
                        dep_inserts.push(AddonDep::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            dependency_dir: ActiveValue::Set(dep_title),
                        });
                    }
                }
                if !dep_inserts.is_empty() {
                    let result = AddonDep::Entity::insert_many(dep_inserts)
                        .on_conflict(
                            OnConflict::columns([
                                AddonDep::Column::AddonId,
                                AddonDep::Column::DependencyDir,
                            ])
                            .do_nothing()
                            .to_owned(),
                        )
                        .exec(&db)
                        .await;
                    check_db_result(result)?;
                }
            }

            info!("Getting installed addons");
            // 3. Get the full installed set along with installed version
            let results = DbAddon::Entity::find()
                .column_as(DbAddon::Column::Version, "version")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(Category::Column::Title, "category")
                .column_as(Expr::value("NULL"), "description")
                .column_as(Expr::value("NULL"), "change_log")
                .column_as(Expr::value("NULL"), "game_compat_version")
                .column_as(Expr::value("NULL"), "game_compat_name")
                .column_as(Category::Column::Icon, "category_icon")
                .inner_join(Category::Entity)
                .inner_join(InstalledAddon::Entity)
                .into_model::<AddonShowDetails>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            info!("Done getting addons!");
            Ok(results)
        })
    }

    pub fn get_missing_dependency_options(&self) -> ImmediateValuePromise<Vec<AddonDepOption>> {
        let db = self.db.clone();

        ImmediateValuePromise::new(async move {
            info!("Checking for missing dependencies");

            let results = AddonDepOption::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"select
            dependency_dir missing_dir,
            required_by,
            a.id option_id,
            a.name option_name
        from (
        select
            adp.dependency_dir,
            group_concat(a.name, ', ') required_by
        from installed_addon i
            inner join addon_dependency adp on i.addon_id = adp.addon_id
            inner join addon a on i.addon_id = a.id
        where
            adp.dependency_dir not in (
                SELECT
                    DISTINCT ad.dir
                FROM
                    installed_addon i2
                    inner join addon_dir ad on i2.addon_id = ad.addon_id
            )
        group by
            adp.dependency_dir
        )
        left outer join addon_dir ad on dependency_dir = ad.dir
        left outer join addon a on ad.addon_id = a.id
        left outer join (
            select addon_id, count(*) as dir_count
            from addon_dir
            group by addon_id
        ) dc on dc.addon_id = a.id
        where
            dependency_dir not in (select addon_dir from manual_dependency)
        order by
            missing_dir,
            (a.name = dependency_dir) desc,
            coalesce(dc.dir_count, 999) asc,
            cast(coalesce(nullif(a.download_monthly, ''), '0') as integer) desc,
            cast(coalesce(nullif(a.date, ''), '0') as integer) desc"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;
            Ok(results)
        })
    }

    pub fn get_addon_dependency_view(
        &self,
        addon_id: i32,
    ) -> ImmediateValuePromise<AddonDependencyView> {
        let db = self.db.clone();

        ImmediateValuePromise::new(async move {
            #[derive(FromQueryResult)]
            struct DirOwner {
                dir: String,
                id: i32,
                name: String,
            }
            #[derive(FromQueryResult)]
            struct SuggestionRow {
                dir: String,
                id: i32,
                name: String,
            }

            let dep_rows = AddonDep::Entity::find()
                .filter(AddonDep::Column::AddonId.eq(addon_id))
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let dep_dirs: Vec<String> = dep_rows.iter().map(|r| r.dependency_dir.clone()).collect();

            let dependents = AddonRef::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"select distinct a.id as id, a.name as name
                from addon_dir my_dir
                inner join addon_dependency adp on adp.dependency_dir = my_dir.dir
                inner join installed_addon i on i.addon_id = adp.addon_id
                inner join addon a on a.id = adp.addon_id
                where my_dir.addon_id = ? and adp.addon_id <> ?
                order by a.name"#,
                [addon_id.into(), addon_id.into()],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;

            let installed_addons = AddonRef::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"select a.id as id, a.name as name
                from installed_addon i
                inner join addon a on a.id = i.addon_id
                order by a.name"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;

            if dep_dirs.is_empty() {
                return Ok(AddonDependencyView {
                    forward: vec![],
                    dependents,
                    installed_addons,
                });
            }

            let manual_rows = ManualDependency::Entity::find()
                .filter(ManualDependency::Column::AddonDir.is_in(dep_dirs.clone()))
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let satisfied_ids: Vec<i32> =
                manual_rows.iter().filter_map(|m| m.satisfied_by).collect();
            let satisfied_addons = if satisfied_ids.is_empty() {
                vec![]
            } else {
                DbAddon::Entity::find()
                    .filter(DbAddon::Column::Id.is_in(satisfied_ids))
                    .all(&db)
                    .await
                    .context(error::DbGetSnafu)?
            };
            let satisfied_name_map: HashMap<i32, String> = satisfied_addons
                .iter()
                .map(|a| (a.id, a.name.clone()))
                .collect();

            let placeholders = vec!["?"; dep_dirs.len()].join(",");
            let owner_sql = format!(
                r#"select ad.dir as dir, a.id as id, a.name as name
                from addon_dir ad
                inner join installed_addon i on i.addon_id = ad.addon_id
                inner join addon a on a.id = ad.addon_id
                where ad.dir in ({placeholders})"#
            );
            let owner_values: Vec<sea_orm::Value> =
                dep_dirs.iter().map(|d| d.clone().into()).collect();
            let owner_rows = DirOwner::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                owner_sql,
                owner_values,
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;
            let installed_owner_map: HashMap<String, AddonRef> = owner_rows
                .into_iter()
                .map(|r| {
                    (
                        r.dir,
                        AddonRef {
                            id: r.id,
                            name: r.name,
                        },
                    )
                })
                .collect();

            let suggestion_sql = format!(
                r#"select ad.dir as dir, a.id as id, a.name as name
                from addon_dir ad
                inner join addon a on a.id = ad.addon_id
                left outer join (
                    select addon_id, count(*) as dir_count
                    from addon_dir
                    group by addon_id
                ) dc on dc.addon_id = a.id
                where ad.dir in ({placeholders})
                order by ad.dir,
                    (a.name = ad.dir) desc,
                    coalesce(dc.dir_count, 999) asc,
                    cast(coalesce(nullif(a.download_monthly, ''), '0') as integer) desc,
                    cast(coalesce(nullif(a.date, ''), '0') as integer) desc"#
            );
            let suggestion_values: Vec<sea_orm::Value> =
                dep_dirs.iter().map(|d| d.clone().into()).collect();
            let suggestion_rows = SuggestionRow::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                suggestion_sql,
                suggestion_values,
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;
            let mut suggestion_map: HashMap<String, Vec<AddonRef>> = HashMap::new();
            for row in suggestion_rows {
                suggestion_map.entry(row.dir).or_default().push(AddonRef {
                    id: row.id,
                    name: row.name,
                });
            }

            let forward: Vec<DepStatus> = dep_dirs
                .iter()
                .map(|dir| {
                    let resolution = if let Some(owner) = installed_owner_map.get(dir) {
                        Resolution::Installed(owner.clone())
                    } else if let Some(manual) = manual_rows.iter().find(|m| m.addon_dir == *dir) {
                        if manual.ignore.unwrap_or(false) {
                            Resolution::Ignored
                        } else if let Some(sb_id) = manual.satisfied_by {
                            Resolution::SatisfiedBy(AddonRef {
                                id: sb_id,
                                name: satisfied_name_map.get(&sb_id).cloned().unwrap_or_default(),
                            })
                        } else {
                            Resolution::Unresolved {
                                suggestions: suggestion_map.remove(dir).unwrap_or_default(),
                            }
                        }
                    } else {
                        Resolution::Unresolved {
                            suggestions: suggestion_map.remove(dir).unwrap_or_default(),
                        }
                    };
                    DepStatus {
                        dep_dir: dir.clone(),
                        resolution,
                    }
                })
                .collect();

            Ok(AddonDependencyView {
                forward,
                dependents,
                installed_addons,
            })
        })
    }

    pub fn set_dep_ignored(&self, dep_dir: String) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            ManualDependency::Entity::insert(ManualDependency::ActiveModel {
                addon_dir: ActiveValue::Set(dep_dir),
                ignore: ActiveValue::Set(Some(true)),
                satisfied_by: ActiveValue::Set(None),
            })
            .on_conflict(
                OnConflict::column(ManualDependency::Column::AddonDir)
                    .update_columns([
                        ManualDependency::Column::Ignore,
                        ManualDependency::Column::SatisfiedBy,
                    ])
                    .to_owned(),
            )
            .exec(&db)
            .await
            .context(error::DbPutSnafu)?;
            Ok(())
        })
    }

    pub fn set_dep_satisfied_by(
        &self,
        dep_dir: String,
        addon_id: i32,
    ) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            ManualDependency::Entity::insert(ManualDependency::ActiveModel {
                addon_dir: ActiveValue::Set(dep_dir),
                ignore: ActiveValue::Set(Some(false)),
                satisfied_by: ActiveValue::Set(Some(addon_id)),
            })
            .on_conflict(
                OnConflict::column(ManualDependency::Column::AddonDir)
                    .update_columns([
                        ManualDependency::Column::Ignore,
                        ManualDependency::Column::SatisfiedBy,
                    ])
                    .to_owned(),
            )
            .exec(&db)
            .await
            .context(error::DbPutSnafu)?;
            Ok(())
        })
    }

    pub fn revoke_dep_override(&self, dep_dir: String) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            ManualDependency::Entity::delete_by_id(dep_dir)
                .exec(&db)
                .await
                .context(error::DbDeleteSnafu)?;
            Ok(())
        })
    }

    pub fn install_dep_suggestions(&self, items: Vec<(String, i32)>) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            for (dep_dir, addon_id) in items {
                service.p_install(addon_id, false).await?;
                ManualDependency::Entity::insert(ManualDependency::ActiveModel {
                    addon_dir: ActiveValue::Set(dep_dir),
                    ignore: ActiveValue::Set(Some(false)),
                    satisfied_by: ActiveValue::Set(Some(addon_id)),
                })
                .on_conflict(
                    OnConflict::column(ManualDependency::Column::AddonDir)
                        .update_columns([
                            ManualDependency::Column::Ignore,
                            ManualDependency::Column::SatisfiedBy,
                        ])
                        .to_owned(),
                )
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;
            }
            Ok(())
        })
    }

    pub fn get_addon_details(
        &self,
        addon_id: i32,
    ) -> ImmediateValuePromise<Option<AddonShowDetails>> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            // check if we need to grab it, grab if needed
            if let Err(e) = service.p_update_addon_details(addon_id).await {
                let label = service.addon_label(addon_id).await;
                service.record_error(format!("Error updating details for {label}"), e);
            }

            // get the details we need
            info!("Loading addon details for id: {addon_id}");
            let result = DbAddon::Entity::find_by_id(addon_id)
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(Category::Column::Title, "category")
                .column_as(AddonDetail::Column::Description, "description")
                .column_as(AddonDetail::Column::ChangeLog, "change_log")
                .column_as(GameCompat::Column::Version, "game_compat_version")
                .column_as(GameCompat::Column::Name, "game_compat_name")
                .column_as(Category::Column::Icon, "category_icon")
                .inner_join(Category::Entity)
                .inner_join(AddonDetail::Entity)
                .left_join(InstalledAddon::Entity)
                .left_join(GameCompat::Entity)
                .filter(
                    Condition::any()
                        .add(GameCompat::Column::Id.is_null())
                        .add(GameCompat::Column::Id.eq(0)),
                )
                .into_model::<AddonShowDetails>()
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            if result.is_none() {
                warn!("No details found for addon: {addon_id}");
            }
            Ok(result)
        })
    }

    // region: Config

    pub fn save_config(&self) {
        if let Err(e) = self.config.save() {
            self.record_error("Saving config", e);
        }
    }

    pub fn get_addon_dir(&self) -> PathBuf {
        self.config.addon_dir.clone()
    }

    // endregion

    async fn base_fs_download_extract(
        &self,
        url: &str,
        path_addr: Option<&str>,
        md5: Option<String>,
    ) -> Result<ZipArchive<File>> {
        let response = self
            .api
            .download_file(url)
            .await?
            .bytes()
            .await
            .context(error::ApiParseResponseSnafu { url })?;

        let mut tmpfile = NamedTempFile::new().context(error::AddonDownloadTmpFileSnafu)?;
        let mut r_tmpfile = tmpfile
            .reopen()
            .context(error::AddonDownloadTmpFileReadSnafu)?;
        tmpfile
            .write_all(response.as_ref())
            .context(error::AddonDownloadTmpFileWriteSnafu)?;
        r_tmpfile
            .rewind()
            .context(error::AddonDownloadTmpFileReadSnafu)?;

        // check hash if present
        if let Some(md5) = md5
            && !md5.trim().is_empty()
        {
            let mut hasher = Md5::new();
            let mut reader = BufReader::new(&r_tmpfile);
            let mut buffer = [0; 8192];
            loop {
                let bytes_read = reader
                    .read(&mut buffer)
                    .context(error::AddonDownloadTmpFileReadSnafu)?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            let hash = hasher.finalize().to_vec();
            let mut hash_string = String::new();
            for x in hash.iter() {
                hash_string.push_str(format!("{x:02x}").as_str());
            }
            if md5 != hash_string {
                warn!("Expected file hash {md5}, got {hash_string}");
            }
            r_tmpfile
                .rewind()
                .context(error::AddonDownloadTmpFileReadSnafu)?;
        }

        let mut archive =
            zip::ZipArchive::new(r_tmpfile).context(error::AddonDownloadZipCreateSnafu)?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .context(error::AddonDownloadZipReadSnafu { file: i })?;
            let outpath = match file.enclosed_name() {
                Some(path) => {
                    let mut p = self.get_addon_dir().clone();
                    if let Some(x) = path_addr {
                        // append additional path if defined
                        p.push(x);
                    }
                    p.push(path);
                    p
                }

                None => continue,
            };

            if (file.name()).ends_with('/') {
                fs::create_dir_all(&outpath)
                    .context(error::AddonDownloadZipExtractSnafu { path: outpath })?;
            } else {
                if let Some(p) = outpath.parent()
                    && !p.exists()
                {
                    fs::create_dir_all(p)
                        .context(error::AddonDownloadZipExtractSnafu { path: p })?;
                }
                let mut outfile =
                    fs::File::create(&outpath).context(error::AddonDownloadZipExtractSnafu {
                        path: outpath.to_owned(),
                    })?;
                io::copy(&mut file, &mut outfile)
                    .context(error::AddonDownloadZipExtractSnafu { path: outpath })?;
            }
        }

        Ok(archive)
    }

    async fn fs_download_addon(&self, url: &str, md5: Option<String>) -> Result<Addon> {
        let mut archive = self.base_fs_download_extract(url, None, md5).await?;
        let mut addon_path = self.get_addon_dir();
        let addon_name = archive
            .by_index(0)
            .context(error::AddonDownloadZipReadSnafu { file: 0_usize })?;
        let addon_name = get_root_dir(&addon_name.mangled_name());
        addon_path.push(addon_name);

        fs_read_addon(&addon_path)
    }

    pub fn update_ttc_pricetable(&self) -> ImmediateValuePromise<TtcConfigUpdate> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Updating TTC PriceTable");
            let mut update = TtcConfigUpdate::default();
            let region = &service.config.ttc_region;

            let mut targets = vec![];
            if *region == TTCRegion::NA || *region == TTCRegion::ALL {
                targets.push((TTC_NA_DOMAIN, service.config.ttc_na_version));
            }
            if *region == TTCRegion::EU || *region == TTCRegion::ALL {
                targets.push((TTC_EU_DOMAIN, service.config.ttc_eu_version));
            }

            let mut downloaded = false;
            for (domain, local_version) in targets {
                // Conditional check: skip download when the server PriceTable
                // version matches what we last downloaded. Fall back to
                // downloading if the version can't be fetched.
                let server_version = match service.api.get_ttc_pricetable_version(domain).await {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warn!("Could not fetch TTC version for {domain}: {e}; downloading anyway");
                        None
                    }
                };
                if let (Some(server), Some(local)) = (server_version, local_version)
                    && server == local
                {
                    info!("TTC PriceTable {domain} unchanged (v{server}), skipping download");
                    continue;
                }

                let url = format!("https://{domain}/download/PriceTable");
                service
                    .base_fs_download_extract(&url, Some("TamrielTradeCentre"), None)
                    .await?;
                downloaded = true;
                if domain == TTC_NA_DOMAIN {
                    update.na_version = server_version;
                } else {
                    update.eu_version = server_version;
                }
            }

            if downloaded {
                update.download_last = Some(chrono::Utc::now());
            }
            Ok(update)
        })
    }

    pub fn import_minion_file(&mut self, file: &Path) -> ImmediateValuePromise<()> {
        // Takes a path to a minion backup file, it should be named something like `BU-addons.txt`
        // It should contain a single line of comma-separated addon IDs
        let service = self.clone();
        let filepath = file.to_path_buf();

        ImmediateValuePromise::new(async move {
            let line = fs::read_to_string(&filepath)?;
            let mut ids: Vec<i32> = Vec::new();
            for raw in line.split(',').filter(|x| !x.is_empty()) {
                match raw.trim().parse::<i32>() {
                    Ok(id) => ids.push(id),
                    Err(e) => {
                        service.record_error(
                            format!("Importing Minion backup {}", filepath.display()),
                            format!("Skipping non-integer addon id {raw:?}: {e}"),
                        );
                    }
                }
            }
            // workaround for weird behavior with promise in promise, slowly install addons one at a time
            for addon_id in ids.iter() {
                if let Err(e) = service.p_install(*addon_id, false).await {
                    let label = service.addon_label(*addon_id).await;
                    service.record_error(format!("Error installing {label}"), e);
                }
            }
            Ok(())
        })
    }

    pub fn get_categories(&self) -> ImmediateValuePromise<Vec<CategoryResult>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let categories = Category::Entity::find()
                .order_by_asc(Category::Column::Id)
                .into_model::<CategoryResult>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            Ok(categories)
        })
    }
    pub fn get_category_parents(&self) -> ImmediateValuePromise<Vec<ParentCategory>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let parents = Category::Entity::find()
                .join_rev(
                    JoinType::InnerJoin,
                    CategoryParent::Relation::Category2.def(),
                )
                .filter(CategoryParent::Column::ParentId.ne(0))
                .order_by_asc(Category::Column::Id)
                .group_by(CategoryParent::Column::ParentId)
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let mut results: Vec<ParentCategory> = vec![];
            for parent in parents.iter() {
                let children = Category::Entity::find()
                    .join_rev(
                        JoinType::InnerJoin,
                        CategoryParent::Relation::Category1.def(),
                    )
                    .filter(CategoryParent::Column::ParentId.eq(parent.id))
                    .order_by_asc(Category::Column::Id)
                    .all(&db)
                    .await
                    .context(error::DbGetSnafu)?;
                results.push(ParentCategory {
                    id: parent.id,
                    title: parent.title.to_string(),
                    child_categories: children,
                });
            }
            Ok(results)
        })
    }

    pub fn get_addons_by_category(
        &self,
        category_id: i32,
    ) -> ImmediateValuePromise<Vec<AddonShowDetails>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let addons = DbAddon::Entity::find()
                .column_as(DbAddon::Column::Version, "version")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(Category::Column::Title, "category")
                .column_as(Expr::value("NULL"), "description")
                .column_as(Expr::value("NULL"), "change_log")
                .column_as(Expr::value("NULL"), "game_compat_version")
                .column_as(Expr::value("NULL"), "game_compat_name")
                .column_as(Category::Column::Icon, "category_icon")
                .inner_join(Category::Entity)
                .join_rev(
                    JoinType::InnerJoin,
                    CategoryParent::Relation::Category1.def(),
                )
                .left_join(InstalledAddon::Entity)
                .filter(
                    Condition::any()
                        .add(Category::Column::Id.eq(category_id))
                        .add(CategoryParent::Column::ParentId.eq(category_id)),
                )
                .group_by(DbAddon::Column::Id)
                .into_model::<AddonShowDetails>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            Ok(addons)
        })
    }
    pub fn install_missing_dependencies(
        &self,
        dep_results: Vec<MissingDepView>,
    ) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            let mut dep_inserts = vec![];
            // install selected IDs if not installed
            for dep_opt in dep_results.iter() {
                let mut dep_insert = ManualDependency::ActiveModel {
                    addon_dir: ActiveValue::Set(dep_opt.missing_dir.clone()),
                    ignore: ActiveValue::Set(None),
                    satisfied_by: ActiveValue::Set(None),
                };
                if let Some(satisfied_by) = dep_opt.satisfied_by {
                    // if it's in the options, it means not installed
                    if dep_opt.options.contains_key(&satisfied_by) {
                        // install addon
                        if let Err(e) = service.p_install(satisfied_by, false).await {
                            let label = service.addon_label(satisfied_by).await;
                            service.record_error(format!("Error installing {label}"), e);
                        }
                    }
                    dep_insert.satisfied_by = ActiveValue::Set(Some(satisfied_by));
                }
                dep_insert.ignore = ActiveValue::Set(Some(dep_opt.ignore));
                dep_inserts.push(dep_insert);
            }
            // insert dep options
            ManualDependency::Entity::insert_many(dep_inserts)
                .on_conflict(
                    OnConflict::column(ManualDependency::Column::AddonDir)
                        .update_columns([
                            ManualDependency::Column::Ignore,
                            ManualDependency::Column::SatisfiedBy,
                        ])
                        .to_owned(),
                )
                .exec(&service.db)
                .await?;
            Ok(())
        })
    }

    pub fn update_hm_data(&self) -> ImmediateValuePromise<HmConfigUpdate> {
        let db = self.db.clone();
        let config = self.config.clone();
        let api = self.api.clone();
        ImmediateValuePromise::new(async move {
            info!("Updating HarvestMap data...");
            // ensure HarvestMap-Data installed (id: 3034)
            const HMD_ID: i32 = 3034;

            let hmd_addon = InstalledAddon::Entity::find_by_id(HMD_ID)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?;
            ensure!(hmd_addon.is_some(), error::HarvestMapDataNotInstalledSnafu);

            let base_dir = config.addon_dir.parent().unwrap_or(&config.addon_dir);
            let saved_var_dir = base_dir.join("SavedVariables");
            let addon_dir = config.addon_dir.join("HarvestMapData");
            let mut empty_file = addon_dir.join("Main");
            empty_file.push("emptyTable.lua");

            let empty_file_data = fs::read_to_string(empty_file)?;

            // Refresh a zone at least this often even when local data is
            // unchanged, so community data does not go stale.
            let max_age = chrono::Duration::days(1);
            let now = chrono::Utc::now();
            let mut update = HmConfigUpdate::default();

            // iterate over the different zones
            for zone in ["AD", "EP", "DC", "DLC", "NF"] {
                let file_name = format!("HarvestMap{zone}.lua");

                let sv_file = saved_var_dir.join(&file_name);
                let data = if sv_file.exists() {
                    fs::read_to_string(&sv_file)?
                } else {
                    format!("Harvest{zone}_SavedVars{}", empty_file_data.as_str())
                };

                // Hash the local data so we can skip the merge when nothing has
                // changed since our last sync.
                let mut hasher = Md5::new();
                hasher.update(data.as_bytes());
                let mut hash = String::new();
                for x in hasher.finalize().iter() {
                    hash.push_str(format!("{x:02x}").as_str());
                }

                let mut out_file = addon_dir.join("Modules");
                out_file.push(format!("HarvestMap{zone}"));
                out_file.push(&file_name);

                let fresh = config
                    .hm_zone_synced
                    .get(zone)
                    .is_some_and(|t| now.signed_duration_since(*t) < max_age);
                if config.hm_zone_hashes.get(zone).map(String::as_str) == Some(hash.as_str())
                    && out_file.exists()
                    && fresh
                {
                    info!("HarvestMap {zone} unchanged and fresh, skipping");
                    continue;
                }

                info!("Syncing HarvestMap {zone}...");
                let response = api.get_hm_data(data).await?;

                let out_dir = out_file.parent().unwrap();
                if !out_dir.exists() {
                    fs::create_dir_all(out_dir)?;
                }
                let mut tmp = NamedTempFile::new_in(out_dir)?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk =
                        chunk.context(error::ApiParseResponseSnafu { url: "harvestmap" })?;
                    tmp.write_all(&chunk)?;
                }
                tmp.persist(&out_file)
                    .map_err(|e| e.error)
                    .context(error::WriteResultSnafu { path: out_file })?;

                update.zone_hashes.insert(zone.to_string(), hash);
            }

            if !update.zone_hashes.is_empty() {
                update.synced_at = Some(now);
            }
            info!("Done HarvestMap data!");
            Ok(update)
        })
    }
    pub fn get_addons_by_author(
        &self,
        author: String,
    ) -> ImmediateValuePromise<Vec<AddonShowDetails>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            info!("Getting addons by author: {author}");
            let results = DbAddon::Entity::find()
                .column_as(DbAddon::Column::Version, "version")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(Category::Column::Title, "category")
                .column_as(Expr::value("NULL"), "description")
                .column_as(Expr::value("NULL"), "change_log")
                .column_as(Expr::value("NULL"), "game_compat_version")
                .column_as(Expr::value("NULL"), "game_compat_name")
                .column_as(Category::Column::Icon, "category_icon")
                .inner_join(Category::Entity)
                .left_join(InstalledAddon::Entity)
                .filter(DbAddon::Column::AuthorName.eq(author))
                .into_model::<AddonShowDetails>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            info!("Done getting addons!");
            Ok(results)
        })
    }

    pub fn get_addon_images(&self, addon_id: i32) -> ImmediateValuePromise<Vec<AddonImageResult>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let results = AddonImage::Entity::find()
                .filter(AddonImage::Column::AddonId.eq(addon_id))
                .order_by_asc(AddonImage::Column::Index)
                .into_model::<AddonImageResult>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            Ok(results)
        })
    }

    // region: Backup/restore data

    /// Backup installed addon data to file
    pub fn backup_data(&self, file: PathBuf) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let installed_addons = InstalledAddon::Entity::find()
                .column(InstalledAddon::Column::AddonId)
                .column(InstalledAddon::Column::Version)
                .into_model::<BackupInstalledAddon>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let manual_deps = ManualDependency::Entity::find()
                .into_model::<BackupManualDependency>()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let backup_data = BackupData {
                installed_addons,
                manual_dependencies: manual_deps,
            };

            serde_json::to_writer(&File::create(file)?, &backup_data)?;
            Ok(())
        })
    }

    /// Restore backed up data from file to database
    pub fn restore_backup(&self, file: PathBuf) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let mut f = File::open(file)?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;

            let data: BackupData = serde_json::from_str(&buf)?;

            if !data.installed_addons.is_empty() {
                // remove existing installed data
                InstalledAddon::Entity::delete_many().exec(&db).await?;

                // import installed addon data
                let mut installed_addons = vec![];
                for x in data.installed_addons {
                    installed_addons.push(InstalledAddon::ActiveModel {
                        addon_id: ActiveValue::Set(x.addon_id),
                        version: ActiveValue::Set("0".to_owned()),
                        date: ActiveValue::Set(x.date),
                    })
                }
                InstalledAddon::Entity::insert_many(installed_addons)
                    .exec(&db)
                    .await?;
            }

            if !data.manual_dependencies.is_empty() {
                // remove existing manual dep data
                ManualDependency::Entity::delete_many().exec(&db).await?;

                // import manual dep data
                let mut dep_inserts = vec![];
                for x in data.manual_dependencies {
                    dep_inserts.push(ManualDependency::ActiveModel {
                        addon_dir: ActiveValue::Set(x.addon_dir),
                        satisfied_by: ActiveValue::Set(x.satisfied_by),
                        ignore: ActiveValue::Set(x.ignore),
                    });
                }
                ManualDependency::Entity::insert_many(dep_inserts)
                    .exec(&db)
                    .await?;
            }

            Ok(())
        })
    }

    // endregion

    pub fn clear_cache(&self) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            // clear download urls
            DbAddon::Entity::update_many()
                .col_expr(DbAddon::Column::Md5, Expr::value(Value::String(None)))
                .col_expr(DbAddon::Column::FileName, Expr::value(Value::String(None)))
                .col_expr(DbAddon::Column::Download, Expr::value(Value::String(None)))
                .exec(&db)
                .await?;
            // clear addon details
            AddonDetail::Entity::delete_many().exec(&db).await?;
            Ok(())
        })
    }
}

async fn resolve_dirs_to_addons<C: ConnectionTrait>(
    db: &C,
    dirs: &[String],
) -> Result<HashMap<String, i32>> {
    if dirs.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; dirs.len()].join(", ");
    let sql = format!(
        r#"select distinct ad.addon_id, ad.dir
        from installed_addon i
        inner join addon_dir ad on ad.addon_id = i.addon_id
        where ad.dir in ({placeholders})"#
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            dirs.iter().map(|k| k.into()).collect::<Vec<_>>(),
        ))
        .await
        .context(error::DbGetSnafu)?;
    Ok(rows
        .iter()
        .map(|row| {
            let id: i32 = row.try_get_by(0).expect("query selects addon_id as i32");
            let dir: String = row.try_get_by(1).expect("query selects dir as text");
            (dir, id)
        })
        .collect())
}

/// Use for inserts where no updates/inserts OK
/// sea_orm now returns DbErr::RecordNotInserted when no inserts
fn check_db_result<T>(result: Result<T, DbErr>) -> Result<()> {
    match result {
        Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
        Err(e) => Err(e).context(error::DbPutSnafu),
    }
}
