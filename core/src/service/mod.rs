use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use self::backup::{BackupData, BackupInstalledAddon, BackupManualDependency};
use self::fs_util::{
    fs_delete_addon, fs_has_addon_manifest, fs_read_addon, parse_full_manifest, parse_manifest_file,
};
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
use entity::git_addon as GitAddon;
use entity::git_repo as GitRepo;
use entity::installed_addon as InstalledAddon;
use entity::installed_git_addon as InstalledGitAddon;
use entity::installed_local_addon as InstalledLocalAddon;
use entity::local_addon as LocalAddon;
use entity::manual_dependency as ManualDependency;
use migration::{Condition, Migrator, MigratorTrait};

use futures::StreamExt;
use lazy_async_promise::ImmediateValuePromise;
use md5::{Digest, Md5};
use sea_orm::sea_query::{Expr, ExprTrait, OnConflict};
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
mod git;
pub mod result;

const TTC_NA_DOMAIN: &str = "us.tamrieltradecentre.com";
const TTC_EU_DOMAIN: &str = "eu.tamrieltradecentre.com";

/// Safe upper bound for SQLite bound parameters per statement (SQLITE_MAX_VARIABLE_NUMBER).
/// Defaults to 32766 on 3.32.0+ (bundled by libsqlite3-sys); 32000 leaves headroom.
/// Used for both `IN (?, ?, ...)` clauses and batch `INSERT … VALUES (…), (…), …`.
const SQLITE_MAX_VARS: usize = 32000;

/// Maximum rows per batched `insert_many()` chunk.
/// Based on the widest table (addon: 13 columns) so every entity stays under SQLITE_MAX_VARS
/// even if the schema adds columns later. SeaORM doesn't expose column counts at compile time.
const INSERT_CHUNK: usize = SQLITE_MAX_VARS / 13;

/// Maximum IDs per `DELETE … WHERE addon_id IN (?, ?, …)` chunk.
/// Far smaller than `SQLITE_MAX_VARS` — large IN clauses cause expensive query plan
/// compilation and memory pressure in SQLite. 500 IDs per batch keeps each DELETE
/// fast while still batching effectively.
const DELETE_CHUNK: usize = 500;

#[derive(Debug, Clone, Default)]
pub struct AddonService {
    pub api: ApiClient,
    pub config: config::Config,
    pub db: DatabaseConnection,
    pub errors: Arc<Mutex<Vec<ErrorRecord>>>,
    pub git_available: bool,
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
            git_available: git::is_available(),
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

    pub fn install(&self, addon_id: i64, update: bool) -> ImmediateValuePromise<()> {
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
    async fn addon_label(&self, addon_id: i64) -> String {
        match DbAddon::Entity::find_by_id(addon_id).one(&self.db).await {
            Ok(Some(a)) => format!("{} (#{addon_id})", a.name),
            _ => format!("addon {addon_id}"),
        }
    }
    async fn p_install(&self, addon_id: i64, update: bool) -> Result<()> {
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

            let mut insert_addons = Vec::with_capacity(file_list.len());
            let mut insert_addon_dirs = vec![];
            let mut insert_compats = vec![];
            let mut insert_imgs = vec![];
            let mut addon_ids = Vec::with_capacity(file_list.len());
            for list_item in file_list.into_iter() {
                let addon_id: i64 = match list_item.id.parse() {
                    Ok(id) => id,
                    Err(e) => {
                        warn!("Skipping addon with non-integer id {:?}: {e}", list_item.id);
                        continue;
                    }
                };
                addon_ids.push(addon_id);
                let addon = DbAddon::ActiveModel {
                    id: ActiveValue::Set(addon_id),
                    category_id: ActiveValue::Set(list_item.category),
                    version: ActiveValue::Set(list_item.version),
                    date: ActiveValue::Set(list_item.date.to_string()),
                    name: ActiveValue::Set(list_item.name),
                    author_name: ActiveValue::Set(Some(list_item.author_name)),
                    file_info_url: ActiveValue::Set(Some(list_item.file_info_url)),
                    download_total: ActiveValue::Set(Some(list_item.download_total)),
                    download_monthly: ActiveValue::Set(Some(list_item.download_monthly)),
                    favorite_total: ActiveValue::Set(Some(list_item.favorite_total)),
                    ..Default::default()
                };

                // AddOn Directories
                for addon_dir in list_item.directories.into_iter() {
                    let addon_dir_model = AddonDir::ActiveModel {
                        addon_id: ActiveValue::Set(addon_id),
                        dir: ActiveValue::Set(addon_dir),
                    };
                    insert_addon_dirs.push(addon_dir_model);
                }

                // Game Compatibility
                if let Some(compats) = list_item.compatibility {
                    for (index, item) in compats.into_iter().enumerate() {
                        let Ok(idx) = index.try_into() else {
                            warn!(
                                "Skipping compat entry {index} for addon {addon_id} (index out of range)"
                            );
                            continue;
                        };
                        insert_compats.push(GameCompat::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            id: ActiveValue::Set(idx),
                            version: ActiveValue::Set(item.version),
                            name: ActiveValue::Set(item.name),
                        });
                    }
                }

                // AddOn Images
                if let (Some(thumbs), Some(imgs)) = (list_item.image_thumbnails, list_item.images) {
                    let it = thumbs.into_iter().zip(imgs);
                    for (i, (thumb, img)) in it.enumerate() {
                        let Ok(idx) = i.try_into() else {
                            warn!("Skipping image {i} for addon {addon_id} (index out of range)");
                            continue;
                        };
                        insert_imgs.push(AddonImage::ActiveModel {
                            addon_id: ActiveValue::Set(addon_id),
                            index: ActiveValue::Set(idx),
                            thumbnail: ActiveValue::Set(thumb),
                            image: ActiveValue::Set(img),
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
            // Chunk batch inserts to stay under SQLITE_MAX_VARS.
            for chunk in insert_addons.chunks(INSERT_CHUNK) {
                DbAddon::Entity::insert_many(chunk.to_vec())
                    .on_conflict(addon_on_conflict.clone())
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }

            // delete + re-insert dirs/compat/images for the IDs we just touched.
            // Use a smaller chunk size for IN clauses to avoid expensive query plan compilation.
            for id_chunk in addon_ids.chunks(DELETE_CHUNK) {
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

            for chunk in insert_addon_dirs.chunks(INSERT_CHUNK) {
                AddonDir::Entity::insert_many(chunk.to_vec())
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }
            for chunk in insert_compats.chunks(INSERT_CHUNK) {
                GameCompat::Entity::insert_many(chunk.to_vec())
                    .exec(&txn)
                    .await
                    .context(error::DbPutSnafu)?;
            }
            for chunk in insert_imgs.chunks(INSERT_CHUNK) {
                AddonImage::Entity::insert_many(chunk.to_vec())
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

    async fn p_update_addon_details(&self, id: i64) -> Result<()> {
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
    pub fn update_addon_details(&self, id: i64) -> ImmediateValuePromise<()> {
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
        let insert_categories = Vec::with_capacity(categories.len());
        let mut insert_categories = insert_categories;
        let mut category_parents = vec![];
        for category in categories.into_iter() {
            let Ok(cat_id) = category.id.parse() else {
                warn!("Skipping category with non-integer id {:?}", category.id);
                continue;
            };
            let file_count = category.file_count.parse().ok();
            let db_category = Category::ActiveModel {
                id: ActiveValue::Set(cat_id),
                title: ActiveValue::Set(category.title),
                icon: ActiveValue::Set(Some(category.icon)),
                file_count: ActiveValue::Set(file_count),
            };
            insert_categories.push(db_category);

            for parent_id in category.parent_ids.into_iter() {
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

    pub fn remove(&self, addon_id: i64) -> ImmediateValuePromise<()> {
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
            fetch_installed_esoui_addons(&db, &addon_dir)
                .await
                .map_err(Into::into)
        })
    }

    pub fn get_missing_dependency_options(&self) -> ImmediateValuePromise<Vec<AddonDepOption>> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();

        ImmediateValuePromise::new(async move {
            info!("Checking for missing dependencies");

            // 1. ESOUI dependencies (existing SQL query)
            let mut results = AddonDepOption::find_by_statement(Statement::from_sql_and_values(
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

            // 2. Non-ESOUI addon dependencies (git + local)
            //    Parse manifests and check against ALL installed directories.
            let all_dirs: HashSet<String> = get_all_installed_dirs(&addon_dir);
            let mut seen: HashSet<String> = results.iter().map(|r| r.missing_dir.clone()).collect();

            // Check git addon dependencies
            let git_addons = GitAddon::Entity::find()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            check_non_esoui_deps(
                &db,
                &addon_dir,
                &git_addons,
                |ga| (&ga.directory, &ga.directory),
                &all_dirs,
                &mut seen,
                &mut results,
            )
            .await;

            // Check local addon dependencies
            let local_addons = LocalAddon::Entity::find()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            check_non_esoui_deps(
                &db,
                &addon_dir,
                &local_addons,
                |la| (&la.directory, &la.title),
                &all_dirs,
                &mut seen,
                &mut results,
            )
            .await;

            Ok(results)
        })
    }

    pub fn get_addon_dependency_view(
        &self,
        addon_id: i64,
    ) -> ImmediateValuePromise<AddonDependencyView> {
        let db = self.db.clone();

        ImmediateValuePromise::new(async move {
            #[derive(FromQueryResult)]
            struct DirOwner {
                dir: String,
                id: i64,
                name: String,
            }
            #[derive(FromQueryResult)]
            struct SuggestionRow {
                dir: String,
                id: i64,
                name: String,
            }

            let dep_rows = AddonDep::Entity::find()
                .filter(AddonDep::Column::AddonId.eq(addon_id))
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let dep_dirs: Vec<String> = dep_rows.into_iter().map(|r| r.dependency_dir).collect();

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
            let satisfied_ids: Vec<i64> =
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
            let satisfied_name_map: HashMap<i64, String> = satisfied_addons
                .into_iter()
                .map(|a| (a.id, a.name))
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
            let mut suggestion_map: HashMap<String, Vec<AddonRef>> =
                HashMap::with_capacity(suggestion_rows.len());
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
        addon_id: i64,
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

    pub fn install_dep_suggestions(&self, items: Vec<(String, i64)>) -> ImmediateValuePromise<()> {
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
        addon_id: i64,
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
            let hash = hasher.finalize();
            let mut hash_string = String::with_capacity(32);
            for x in hash.iter() {
                write!(hash_string, "{x:02x}").unwrap();
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

            if file.is_dir() {
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
                match service
                    .base_fs_download_extract(&url, Some("TamrielTradeCentre"), None)
                    .await
                {
                    Ok(_) => {
                        downloaded = true;
                        if domain == TTC_NA_DOMAIN {
                            update.na_version = server_version;
                        } else {
                            update.eu_version = server_version;
                        }
                    }
                    Err(e) => {
                        warn!("TTC PriceTable {domain} download/extract failed: {e}");
                        service.record_error(
                            format!("Updating TTC PriceTable ({domain})"),
                            e.to_string(),
                        );
                        // Still advance the version tracking so a stale local copy
                        // doesn't cause a permanent retry loop on every launch.
                        if domain == TTC_NA_DOMAIN {
                            update.na_version = server_version;
                        } else {
                            update.eu_version = server_version;
                        }
                    }
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
            let ids: Vec<i64> = line
                .split(',')
                .filter(|x| !x.is_empty())
                .filter_map(|raw| {
                    raw.trim()
                        .parse::<i64>()
                        .map_err(|e| {
                            service.record_error(
                                format!("Importing Minion backup {}", filepath.display()),
                                format!("Skipping non-integer addon id {raw:?}: {e}"),
                            );
                        })
                        .ok()
                })
                .collect();
            // workaround for weird behavior with promise in promise, slowly install addons one at a time
            for addon_id in ids {
                if let Err(e) = service.p_install(addon_id, false).await {
                    let label = service.addon_label(addon_id).await;
                    service.record_error(format!("Error installing {label}"), e);
                }
            }
            Ok(())
        })
    }

    /// Scan all discovered Minion backup directories for `BU-*.zip` files.
    ///
    /// Returns a list of [`MinionBackupEntry`] sorted by file name descending
    /// (newest first).
    pub fn scan_minion_backups(&self) -> ImmediateValuePromise<Vec<MinionBackupEntry>> {
        ImmediateValuePromise::new(async {
            let game_dirs = config::detect_minion_backup_dirs();
            let mut entries: Vec<MinionBackupEntry> = Vec::new();

            for game_dir in game_dirs {
                let environment = match game_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_lowercase())
                    .as_deref()
                {
                    Some("eso-1") => MinionEnvironment::Live,
                    Some("eso-2") => MinionEnvironment::Beta,
                    _ => MinionEnvironment::Beta,
                };

                let Ok(read_dir) = fs::read_dir(&game_dir) else {
                    warn!(
                        "Unable to read Minion backup directory: {}",
                        game_dir.display()
                    );
                    continue;
                };

                for entry in read_dir.filter_map(|e| e.ok()) {
                    let file_name = entry.file_name();
                    let name = file_name.to_string_lossy();
                    if name.starts_with("BU-") && name.ends_with(".zip") {
                        entries.push(MinionBackupEntry {
                            path: entry.path(),
                            label: name.trim_end_matches(".zip").to_string(),
                            environment,
                        });
                    }
                }
            }

            // Sort by label descending so newest backups appear first.
            entries.sort_by(|a, b| b.label.cmp(&a.label));
            Ok(entries)
        })
    }

    /// Import addons from a Minion backup `.zip` file.
    ///
    /// Extracts `BU-addons.txt` from the archive and feeds the comma-separated
    /// addon IDs into the existing install pipeline.
    pub fn import_minion_backup(&mut self, zip_path: &Path) -> ImmediateValuePromise<()> {
        let service = self.clone();
        let filepath = zip_path.to_path_buf();

        ImmediateValuePromise::new(async move {
            let file = File::open(&filepath).context(error::IoSnafu {
                path: filepath.as_path(),
            })?;
            let mut archive = ZipArchive::new(file).context(error::MinionZipArchiveSnafu {
                path: filepath.as_path(),
            })?;

            // Locate BU-addons.txt inside the zip.
            let txt_index = archive
                .file_names()
                .position(|name| name.contains("BU-addons.txt"))
                .context(error::MinionZipFileNotFoundSnafu {
                    name: "BU-addons.txt",
                    path: filepath.as_path(),
                })?;

            let mut txt_file =
                archive
                    .by_index(txt_index)
                    .context(error::MinionZipFileReadSnafu {
                        name: "BU-addons.txt",
                        path: filepath.as_path(),
                    })?;

            let mut line = String::new();
            txt_file.read_to_string(&mut line).context(error::IoSnafu {
                path: filepath.as_path(),
            })?;

            let ids: Vec<i64> = line
                .split(',')
                .filter(|x| !x.is_empty())
                .filter_map(|raw| {
                    raw.trim()
                        .parse::<i64>()
                        .map_err(|e| {
                            service.record_error(
                                format!("Importing Minion backup {}", filepath.display()),
                                format!("Skipping non-integer addon id {raw:?}: {e}"),
                            );
                        })
                        .ok()
                })
                .collect();

            for addon_id in ids {
                if let Err(e) = service.p_install(addon_id, false).await {
                    let label = service.addon_label(addon_id).await;
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
            let mut results: Vec<ParentCategory> = Vec::with_capacity(parents.len());
            for parent in parents {
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
                    title: parent.title,
                    child_categories: children,
                });
            }
            Ok(results)
        })
    }

    pub fn get_addons_by_category(
        &self,
        category_id: i64,
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
            let mut dep_inserts = Vec::with_capacity(dep_results.len());
            // install selected IDs if not installed
            for dep_opt in dep_results {
                let mut dep_insert = ManualDependency::ActiveModel {
                    addon_dir: ActiveValue::Set(dep_opt.missing_dir),
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
            const HMD_ID: i64 = 3034;

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
                let mut hash = String::with_capacity(32);
                for x in hasher.finalize().iter() {
                    write!(hash, "{x:02x}").unwrap();
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

    pub fn pull_git_repositories(&self) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Pulling Git repositories");
            tokio::task::spawn_blocking(move || service.p_pull_git_repositories()).await?;
            Ok(())
        })
    }

    fn p_find_git_repositories(&self, addon_dir: &Path) -> Vec<PathBuf> {
        let mut repositories = Vec::new();
        let mut walker = WalkDir::new(addon_dir).into_iter();
        while let Some(entry) = walker.next() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.record_error(
                        format!(
                            "Scanning Git repositories at {}",
                            error.path().unwrap_or(addon_dir).display()
                        ),
                        error,
                    );
                    continue;
                }
            };
            let path = entry.path();

            if (entry.depth() == 0 || entry.file_type().is_dir()) && path.join(".jj").exists() {
                info!("Skipping Jujutsu workspace {}", path.display());
                walker.skip_current_dir();
                continue;
            }

            if !entry.file_type().is_dir() {
                continue;
            }

            if path.join(".git").exists() {
                repositories.push(path.to_owned());
                walker.skip_current_dir();
                continue;
            }

            match fs_has_addon_manifest(path) {
                Ok(true) => walker.skip_current_dir(),
                Ok(false) => {}
                Err(error) => {
                    self.record_error(
                        format!("Scanning Git repositories at {}", path.display()),
                        error,
                    );
                    walker.skip_current_dir();
                }
            }
        }

        repositories
    }

    fn p_pull_git_repositories(&self) {
        let repositories = self.p_find_git_repositories(&self.get_addon_dir());
        for path in repositories {
            if let Err(error) = git::pull_repository(&path) {
                self.record_error(format!("Pulling Git repository {}", path.display()), error);
            }
        }
        info!("Done pulling Git repositories!");
    }

    // region: Multi-source addon scanning

    /// Scan the AddOns directory for locally installed (non-ESOUI, non-git) addons.
    /// Upserts findings into the `local_addon` and `installed_local_addon` tables.
    pub fn scan_local_addons(&self) -> ImmediateValuePromise<Vec<LocalAddonResult>> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        let git_repos = self.p_find_git_repositories(&addon_dir);
        ImmediateValuePromise::new(async move {
            info!("Scanning for local addons");

            // Get directories already tracked as local addons
            let existing: Vec<LocalAddon::Model> = LocalAddon::Entity::find()
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let existing_map: HashMap<String, i64> = existing
                .iter()
                .map(|a| (a.directory.clone(), a.id))
                .collect();

            // Build a lookup of directory -> ESOUI addon_id from addon_dir table
            #[derive(FromQueryResult)]
            struct AddonDirRow {
                dir: String,
                addon_id: i64,
            }
            let addon_dir_rows: Vec<AddonDirRow> =
                AddonDirRow::find_by_statement(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT dir, addon_id FROM addon_dir",
                    [],
                ))
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let esoui_match: HashMap<String, i64> = addon_dir_rows
                .iter()
                .map(|r| (r.dir.clone(), r.addon_id))
                .collect();

            // Build a set of directories that are currently managed by ESOUI (installed).
            // These should NOT appear as local addons even if their directory exists on disk.
            #[derive(FromQueryResult)]
            struct InstalledAddonDir {
                dir: String,
            }
            let installed_dirs: Vec<InstalledAddonDir> =
                InstalledAddonDir::find_by_statement(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT d.dir FROM addon_dir d \
                     INNER JOIN installed_addon i ON d.addon_id = i.addon_id \
                     WHERE d.dir != ''",
                    [],
                ))
                .all(&db)
                .await
                .context(error::DbGetSnafu)?;
            let installed_dir_names: HashSet<String> =
                installed_dirs.iter().map(|r| r.dir.clone()).collect();

            // Get directories tracked by git repos
            let git_dirs: HashSet<String> = git_repos
                .iter()
                .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
                .collect();

            // Walk AddOns/ top-level directories
            let walker = WalkDir::new(&addon_dir)
                .min_depth(1)
                .max_depth(1)
                .into_iter();

            let mut results = Vec::new();

            for entry in walker.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
                let dir_path = entry.path();
                let Some(dir_name) = dir_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let dir_name = dir_name.to_string();

                // Skip git repos
                if git_dirs.contains(&dir_name) {
                    continue;
                }

                // Skip directories managed by ESOUI (installed addons)
                if installed_dir_names.contains(&dir_name) {
                    continue;
                }

                // Parse manifest (free function, just file I/O)
                let manifest = parse_full_manifest(dir_path, &dir_name);
                let title = manifest.as_ref().map_or_else(
                    || dir_name.to_string(),
                    |m| {
                        if m.title.is_empty() {
                            dir_name.to_string()
                        } else {
                            m.title.clone()
                        }
                    },
                );
                let author = manifest.as_ref().and_then(|m| {
                    if m.author.is_empty() {
                        None
                    } else {
                        Some(m.author.clone())
                    }
                });
                let version = manifest.as_ref().and_then(|m| m.version.clone());
                let add_on_version = manifest
                    .as_ref()
                    .and_then(|m| m.addon_version.map(|v| v as i64));
                let api_version = manifest.as_ref().map(|m| m.api_version);
                let depends_on = manifest.as_ref().map_or_else(Vec::new, |m| {
                    m.depends_on.iter().map(|d| d.title.clone()).collect()
                });
                let optional_depends_on = manifest.as_ref().map_or_else(Vec::new, |m| {
                    m.optional_depends_on
                        .iter()
                        .map(|d| d.title.clone())
                        .collect()
                });
                let description = manifest.as_ref().and_then(|m| m.description.clone());
                let category = manifest.as_ref().and_then(|m| match m.is_library {
                    Some(true) => Some("Libraries".to_string()),
                    Some(false) | None => None,
                });

                // Upsert into local_addon table
                let new_id = if let Some(id) = existing_map.get(&dir_name) {
                    *id
                } else {
                    let active = LocalAddon::ActiveModel {
                        id: ActiveValue::NotSet,
                        directory: ActiveValue::Set(dir_name.clone()),
                        title: ActiveValue::Set(title.clone()),
                        author: ActiveValue::Set(author.clone()),
                        version: ActiveValue::Set(version.clone()),
                        add_on_version: ActiveValue::Set(add_on_version),
                        description: ActiveValue::Set(description.clone()),
                    };
                    active.insert(&db).await.context(error::DbPutSnafu)?.id
                };

                // Insert/update installed_local_addon via upsert
                let _ = InstalledLocalAddon::Entity::insert(InstalledLocalAddon::ActiveModel {
                    local_addon_id: ActiveValue::Set(new_id),
                    installed_version: ActiveValue::Set(version.clone()),
                    installed_add_on_version: ActiveValue::Set(add_on_version),
                })
                .on_conflict(
                    OnConflict::column(InstalledLocalAddon::Column::LocalAddonId)
                        .update_columns([
                            InstalledLocalAddon::Column::InstalledVersion,
                            InstalledLocalAddon::Column::InstalledAddOnVersion,
                        ])
                        .to_owned(),
                )
                .exec(&db)
                .await;

                results.push(LocalAddonResult {
                    id: new_id,
                    directory: dir_name.clone(),
                    title,
                    author,
                    version,
                    add_on_version,
                    api_version,
                    depends_on,
                    optional_depends_on,
                    description,
                    category,
                    esoui_match_id: esoui_match.get(&dir_name).copied(),
                });
            }

            info!("Done scanning local addons: {} found", results.len());
            Ok(results)
        })
    }

    /// Scan git repositories in the AddOns directory and upsert metadata.
    pub fn scan_git_addons(&self) -> ImmediateValuePromise<Vec<GitAddonResult>> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            info!("Scanning Git addon repositories");

            let repositories = {
                let db_clone = db.clone();
                let addon_dir_clone = addon_dir.clone();
                tokio::task::spawn_blocking(move || {
                    let service = AddonService {
                        db: db_clone,
                        ..Default::default()
                    };
                    service.p_find_git_repositories(&addon_dir_clone)
                })
                .await
                .unwrap_or_default()
            };

            let mut results = Vec::new();

            for repo_path in &repositories {
                // Determine the remote URL
                let url = git::get_remote_url(repo_path).unwrap_or_else(|_| {
                    repo_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                });

                // Upsert git_repo
                let existing_repo = GitRepo::Entity::find()
                    .filter(GitRepo::Column::Url.eq(url.clone()))
                    .one(&db)
                    .await
                    .context(error::DbGetSnafu)?;

                let repo_id = if let Some(repo) = existing_repo {
                    repo.id
                } else {
                    let path_str = repo_path
                        .strip_prefix(&addon_dir)
                        .unwrap_or(repo_path)
                        .to_string_lossy()
                        .to_string();
                    let now = chrono::Utc::now().to_rfc3339();
                    let active = GitRepo::ActiveModel {
                        id: ActiveValue::NotSet,
                        url: ActiveValue::Set(url.clone()),
                        branch: ActiveValue::Set(String::from("main")),
                        path: ActiveValue::Set(path_str),
                        last_updated: ActiveValue::Set(Some(now.clone())),
                        created_at: ActiveValue::Set(now),
                    };
                    active.insert(&db).await.context(error::DbPutSnafu)?.id
                };

                // Determine addon directories within the repo
                let addon_paths: Vec<PathBuf> = if fs_has_addon_manifest(repo_path).unwrap_or(false)
                {
                    vec![repo_path.clone()]
                } else {
                    let mut subdirs = Vec::new();
                    if let Ok(entries) = fs::read_dir(repo_path) {
                        for entry in entries.filter_map(|e| e.ok()) {
                            let p = entry.path();
                            if p.is_dir() && fs_has_addon_manifest(&p).unwrap_or(false) {
                                subdirs.push(p);
                            }
                        }
                    }
                    if subdirs.is_empty() {
                        vec![repo_path.clone()]
                    } else {
                        subdirs
                    }
                };

                for addon_path in addon_paths {
                    let dir_name = addon_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Parse manifest
                    let manifest = parse_full_manifest(&addon_path, &dir_name);
                    let title = manifest.as_ref().map_or_else(
                        || dir_name.to_string(),
                        |m| {
                            if m.title.is_empty() {
                                dir_name.to_string()
                            } else {
                                m.title.clone()
                            }
                        },
                    );
                    let author = manifest.as_ref().and_then(|m| {
                        if m.author.is_empty() {
                            None
                        } else {
                            Some(m.author.clone())
                        }
                    });
                    let version = manifest.as_ref().and_then(|m| m.version.clone());
                    let add_on_version = manifest
                        .as_ref()
                        .and_then(|m| m.addon_version.map(|v| v as i64));
                    let api_version = manifest.as_ref().map(|m| m.api_version);
                    let depends_on = manifest.as_ref().map_or_else(Vec::new, |m| {
                        m.depends_on.iter().map(|d| d.title.clone()).collect()
                    });
                    let optional_depends_on = manifest.as_ref().map_or_else(Vec::new, |m| {
                        m.optional_depends_on
                            .iter()
                            .map(|d| d.title.clone())
                            .collect()
                    });
                    let description = manifest.as_ref().and_then(|m| m.description.clone());
                    let category = manifest.as_ref().and_then(|m| match m.is_library {
                        Some(true) => Some("Libraries".to_string()),
                        Some(false) | None => None,
                    });

                    // Upsert git_addon
                    let existing_addon = GitAddon::Entity::find()
                        .filter(GitAddon::Column::GitRepoId.eq(repo_id))
                        .filter(GitAddon::Column::Directory.eq(dir_name.clone()))
                        .one(&db)
                        .await
                        .context(error::DbGetSnafu)?;

                    let addon_id = if let Some(addon) = existing_addon {
                        addon.id
                    } else {
                        let active = GitAddon::ActiveModel {
                            id: ActiveValue::NotSet,
                            git_repo_id: ActiveValue::Set(repo_id),
                            directory: ActiveValue::Set(dir_name.clone()),
                            title: ActiveValue::Set(title.clone()),
                            author: ActiveValue::Set(author.clone()),
                            version: ActiveValue::Set(version.clone()),
                            add_on_version: ActiveValue::Set(add_on_version),
                            description: ActiveValue::Set(description.clone()),
                            remote_version: ActiveValue::Set(version.clone()),
                            remote_add_on_version: ActiveValue::Set(add_on_version),
                        };
                        active.insert(&db).await.context(error::DbPutSnafu)?.id
                    };

                    // Insert/update installed_git_addon via upsert
                    let _ = InstalledGitAddon::Entity::insert(InstalledGitAddon::ActiveModel {
                        git_addon_id: ActiveValue::Set(addon_id),
                        installed_version: ActiveValue::Set(version.clone()),
                        installed_add_on_version: ActiveValue::Set(add_on_version),
                    })
                    .on_conflict(
                        OnConflict::column(InstalledGitAddon::Column::GitAddonId)
                            .update_columns([
                                InstalledGitAddon::Column::InstalledVersion,
                                InstalledGitAddon::Column::InstalledAddOnVersion,
                            ])
                            .to_owned(),
                    )
                    .exec(&db)
                    .await;

                    results.push(GitAddonResult {
                        id: addon_id,
                        repo_id,
                        repo_url: url.clone(),
                        directory: dir_name,
                        title,
                        author,
                        remote_version: version.clone(),
                        remote_add_on_version: add_on_version,
                        version,
                        add_on_version,
                        api_version,
                        depends_on,
                        optional_depends_on,
                        description,
                        is_upgradable: false,
                        category,
                    });
                }
            }

            info!(
                "Done scanning Git addons: {} found across {} repos",
                results.len(),
                repositories.len()
            );
            Ok(results)
        })
    }

    /// Get all installed addons from all sources (ESOUI, Git, Local).
    pub fn get_all_installed_addons(&self) -> ImmediateValuePromise<Vec<InstalledAddonDetails>> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            let mut all = Vec::new();

            // 1. ESOUI addons (reuse the existing logic)
            let esoui_addons = fetch_installed_esoui_addons(&db, &addon_dir).await?;
            for addon in esoui_addons {
                all.push(InstalledAddonDetails::from_esoui(addon));
            }

            // 2. Local addons
            let local_addons = fetch_local_addons(&db).await?;
            for addon in local_addons {
                let version = addon.version.clone().unwrap_or_default();
                let installed = addon.installed_version.clone();
                let is_upgradable = version != installed.as_deref().unwrap_or_default();
                let category = if addon.title.starts_with("Lib") {
                    "Libraries".to_string()
                } else {
                    "Addons".to_string()
                };
                all.push(InstalledAddonDetails {
                    source: AddonSource::Local,
                    id: addon.id,
                    name: addon.title,
                    author: addon.author,
                    version,
                    installed_version: installed,
                    is_upgradable,
                    category,
                    description: addon.description,
                    ..Default::default()
                });
            }

            // 3. Git addons
            let git_addons = fetch_git_addons(&db).await?;
            for addon in git_addons {
                let version = addon.version.clone().unwrap_or_default();
                let installed = addon.installed_version.clone();
                let is_upg = version != installed.as_deref().unwrap_or_default();
                let category = if addon.title.starts_with("Lib") {
                    "Libraries".to_string()
                } else {
                    "Addons".to_string()
                };
                all.push(InstalledAddonDetails {
                    source: AddonSource::Git {
                        repo_id: addon.repo_id,
                        repo_url: addon.repo_url,
                    },
                    id: addon.id,
                    name: addon.title,
                    author: addon.author,
                    version,
                    installed_version: installed,
                    is_upgradable: is_upg,
                    category,
                    description: addon.description,
                    ..Default::default()
                });
            }

            Ok(all)
        })
    }

    /// Get all installed addon directory names across all sources (ESOUI, Git, Local).
    /// Used for resolving manifest-based dependencies.
    pub fn get_installed_addon_directories(&self) -> ImmediateValuePromise<HashSet<String>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            use sea_orm::Statement;

            #[derive(FromQueryResult)]
            struct DirRow {
                directory: String,
            }

            // ESOUI: from addon_dir joined with installed_addon
            let esoui_dirs = DirRow::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"SELECT d.dir as directory
                   FROM addon_dir d
                   INNER JOIN installed_addon i ON i.addon_id = d.addon_id"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;

            // Local: from local_addon joined with installed_local_addon
            let local_dirs = DirRow::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"SELECT la.directory as directory
                   FROM local_addon la
                   INNER JOIN installed_local_addon il ON il.local_addon_id = la.id"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;

            // Git: from git_addon joined with installed_git_addon
            let git_dirs = DirRow::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                r#"SELECT ga.directory as directory
                   FROM git_addon ga
                   INNER JOIN installed_git_addon ig ON ig.git_addon_id = ga.id"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)?;

            let mut dirs = HashSet::new();
            for row in esoui_dirs {
                dirs.insert(row.directory);
            }
            for row in local_dirs {
                dirs.insert(row.directory);
            }
            for row in git_dirs {
                dirs.insert(row.directory);
            }
            Ok(dirs)
        })
    }

    /// Get local (manually installed) addons with installation state.
    pub fn get_local_addons(&self) -> ImmediateValuePromise<Vec<LocalInstalledAddon>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move { Ok(fetch_local_addons(&db).await?) })
    }

    /// Get git-tracked addons with installation state and repo info.
    pub fn get_git_addons(&self) -> ImmediateValuePromise<Vec<GitInstalledAddon>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move { Ok(fetch_git_addons(&db).await?) })
    }

    /// Uninstall a locally installed addon by removing its directory and DB records.
    pub fn uninstall_local_addon(&self, local_addon_id: i64) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            // Get the addon directory name
            let addon = LocalAddon::Entity::find_by_id(local_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: local_addon_id })?;

            // Delete from filesystem
            let full_path = addon_dir.join(&addon.directory);
            if full_path.exists() {
                fs::remove_dir_all(&full_path).context(error::AddonDeleteSnafu {
                    dir: full_path.clone(),
                })?;
            }

            // DB records are cascade-deleted when we delete the local_addon row
            LocalAddon::Entity::find_by_id(local_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: local_addon_id })?
                .into_active_model()
                .delete(&db)
                .await
                .context(error::DbDeleteSnafu)?;

            info!(
                "Uninstalled local addon '{}' ({})",
                addon.title, addon.directory
            );
            Ok(())
        })
    }

    /// Uninstall a git-tracked addon by removing its directory and DB records.
    pub fn uninstall_git_addon(&self, git_addon_id: i64) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            // Get the addon directory name
            let addon = GitAddon::Entity::find_by_id(git_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: git_addon_id })?;

            // Delete from filesystem
            let full_path = addon_dir.join(&addon.directory);
            if full_path.exists() {
                fs::remove_dir_all(&full_path).context(error::AddonDeleteSnafu {
                    dir: full_path.clone(),
                })?;
            }

            // DB records are cascade-deleted when we delete the git_addon row
            GitAddon::Entity::find_by_id(git_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: git_addon_id })?
                .into_active_model()
                .delete(&db)
                .await
                .context(error::DbDeleteSnafu)?;

            info!(
                "Uninstalled git addon '{}' ({})",
                addon.title, addon.directory
            );
            Ok(())
        })
    }

    /// Promote a local addon to a managed ESOUI addon by linking its directory to
    /// an existing ESOUI addon entry. This creates an `installed_addon` record and
    /// removes the local addon records. The directory itself is NOT moved or renamed.
    pub fn promote_local_to_esoui(
        &self,
        local_addon_id: i64,
        esoui_addon_id: i64,
    ) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            // Verify the ESOUI addon exists and fetch its details
            let esoui_addon = DbAddon::Entity::find_by_id(esoui_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: esoui_addon_id })?;

            // Create the installed_addon record (upsert on conflict)
            InstalledAddon::Entity::insert(InstalledAddon::ActiveModel {
                addon_id: ActiveValue::Set(esoui_addon_id),
                version: ActiveValue::Set(esoui_addon.version.clone()),
                date: ActiveValue::Set(esoui_addon.date.clone()),
            })
            .on_conflict(
                OnConflict::column(InstalledAddon::Column::AddonId)
                    .update_columns([
                        InstalledAddon::Column::Date,
                        InstalledAddon::Column::Version,
                    ])
                    .to_owned(),
            )
            .exec(&db)
            .await
            .context(error::DbPutSnafu)?;

            // Delete installed_local_addon record (if any)
            InstalledLocalAddon::Entity::delete_many()
                .filter(InstalledLocalAddon::Column::LocalAddonId.eq(local_addon_id))
                .exec(&db)
                .await
                .context(error::DbDeleteSnafu)?;

            // Fetch and delete local_addon record (cascade handles children)
            let local_addon = LocalAddon::Entity::find_by_id(local_addon_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: local_addon_id })?;

            let local_title = local_addon.title.clone();
            let local_directory = local_addon.directory.clone();

            local_addon
                .into_active_model()
                .delete(&db)
                .await
                .context(error::DbDeleteSnafu)?;

            info!(
                "Promoted local addon '{}' ({}) to ESOUI addon {} (#{})",
                local_title, local_directory, esoui_addon.name, esoui_addon_id
            );
            Ok(())
        })
    }

    /// Pull a specific git repository and update metadata.
    pub fn pull_git_repo(&self, repo_id: i64) -> ImmediateValuePromise<()> {
        let db = self.db.clone();
        let addon_dir = self.get_addon_dir().clone();
        ImmediateValuePromise::new(async move {
            let repo = GitRepo::Entity::find_by_id(repo_id)
                .one(&db)
                .await
                .context(error::DbGetSnafu)?
                .context(error::AddonNotFoundSnafu { id: repo_id })?;

            let repo_path = addon_dir.join(&repo.path);
            if !repo_path.exists() {
                Err(error::Error::AddonNotFound { id: repo_id })?;
            }

            // Pull the repository
            git::pull_repository(&repo_path)?;

            // Update last_updated timestamp
            let now = chrono::Utc::now().to_rfc3339();
            let repo_url = repo.url.clone();
            let mut active: GitRepo::ActiveModel = repo.into_active_model();
            active.last_updated = ActiveValue::Set(Some(now));
            active.update(&db).await.context(error::DbPutSnafu)?;

            info!("Pulled Git repository '{}' (id={})", repo_url, repo_id);
            Ok(())
        })
    }

    // endregion

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

    pub fn get_addon_images(&self, addon_id: i64) -> ImmediateValuePromise<Vec<AddonImageResult>> {
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

            // Restore installed addons.
            // Addons may have been removed from the site since the backup was created,
            // so we only insert entries whose addon_id still exists in the addon table.
            if !data.installed_addons.is_empty() {
                let backup_ids: Vec<i64> =
                    data.installed_addons.iter().map(|a| a.addon_id).collect();

                // Batch-check which backup IDs still exist in the addon table
                let existing_ids: HashSet<i64> = DbAddon::Entity::find()
                    .filter(DbAddon::Column::Id.is_in(backup_ids.clone()))
                    .all(&db)
                    .await?
                    .into_iter()
                    .map(|a| a.id)
                    .collect();

                // Report addon IDs from the backup that no longer exist
                let missing_ids: Vec<i64> = backup_ids
                    .into_iter()
                    .filter(|id| !existing_ids.contains(id))
                    .collect();
                if !missing_ids.is_empty() {
                    warn!(
                        "Backup contains {} installed addon ID(s) that no longer exist in the addon table: {:?}",
                        missing_ids.len(),
                        missing_ids
                    );
                }

                // Delete existing installed addon data
                InstalledAddon::Entity::delete_many().exec(&db).await?;

                // Insert only entries whose addon still exists
                let installed_addons: Vec<InstalledAddon::ActiveModel> = data
                    .installed_addons
                    .into_iter()
                    .filter(|a| existing_ids.contains(&a.addon_id))
                    .map(|x| InstalledAddon::ActiveModel {
                        addon_id: ActiveValue::Set(x.addon_id),
                        version: ActiveValue::Set("0".to_owned()),
                        date: ActiveValue::Set(x.date),
                    })
                    .collect();

                if !installed_addons.is_empty() {
                    InstalledAddon::Entity::insert_many(installed_addons)
                        .exec(&db)
                        .await?;
                }
            }

            if !data.manual_dependencies.is_empty() {
                // Delete existing manual dep data
                ManualDependency::Entity::delete_many().exec(&db).await?;

                // Batch-check which satisfied_by IDs still exist in the addon table
                let satisfied_by_ids: Vec<i64> = data
                    .manual_dependencies
                    .iter()
                    .filter_map(|x| x.satisfied_by)
                    .collect();

                let existing_ids: HashSet<i64> = DbAddon::Entity::find()
                    .filter(DbAddon::Column::Id.is_in(satisfied_by_ids.clone()))
                    .all(&db)
                    .await?
                    .into_iter()
                    .map(|a| a.id)
                    .collect();

                // Report satisfied_by IDs from the backup that no longer exist
                let missing_ids: Vec<i64> = satisfied_by_ids
                    .into_iter()
                    .filter(|id| !existing_ids.contains(id))
                    .collect();
                if !missing_ids.is_empty() {
                    warn!(
                        "Backup contains {} manual dependency satisfied_by ID(s) that no longer exist in the addon table: {:?}",
                        missing_ids.len(),
                        missing_ids
                    );
                }

                // Insert only entries whose referenced addon still exists.
                // Entries with no satisfied_by or marked as ignored always pass.
                let dep_inserts: Vec<ManualDependency::ActiveModel> = data
                    .manual_dependencies
                    .into_iter()
                    .filter(|x| {
                        if x.ignore.unwrap_or(false) || x.satisfied_by.is_none() {
                            return true;
                        }
                        existing_ids.contains(&x.satisfied_by.unwrap())
                    })
                    .map(|x| ManualDependency::ActiveModel {
                        addon_dir: ActiveValue::Set(x.addon_dir),
                        satisfied_by: ActiveValue::Set(x.satisfied_by),
                        ignore: ActiveValue::Set(x.ignore),
                    })
                    .collect();

                if !dep_inserts.is_empty() {
                    ManualDependency::Entity::insert_many(dep_inserts)
                        .exec(&db)
                        .await?;
                }
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
) -> Result<HashMap<String, i64>> {
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
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            dirs.iter().map(|k| k.into()).collect::<Vec<_>>(),
        ))
        .await
        .context(error::DbGetSnafu)?;
    Ok(rows
        .iter()
        .map(|row| {
            let id: i64 = row.try_get_by(0).expect("query selects addon_id as i64");
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

// ---------------------------------------------------------------------------
// Private async helpers for multi-source addon queries
// ---------------------------------------------------------------------------

/// Fetch local addon rows (join with installed state).
async fn fetch_local_addons(db: &DatabaseConnection) -> Result<Vec<LocalInstalledAddon>> {
    LocalAddon::Entity::find()
        .column_as(LocalAddon::Column::Id, "id")
        .column_as(LocalAddon::Column::Directory, "directory")
        .column_as(LocalAddon::Column::Title, "title")
        .column_as(LocalAddon::Column::Author, "author")
        .column_as(LocalAddon::Column::Version, "version")
        .column_as(LocalAddon::Column::AddOnVersion, "add_on_version")
        .column_as(LocalAddon::Column::Description, "description")
        .column_as(
            InstalledLocalAddon::Column::InstalledVersion,
            "installed_version",
        )
        .column_as(
            InstalledLocalAddon::Column::InstalledAddOnVersion,
            "installed_add_on_version",
        )
        .left_join(InstalledLocalAddon::Entity)
        .into_model::<LocalInstalledAddon>()
        .all(db)
        .await
        .context(error::DbGetSnafu)
}

/// Fetch git addon rows (join with installed state and repo info).
async fn fetch_git_addons(db: &DatabaseConnection) -> Result<Vec<GitInstalledAddon>> {
    GitAddon::Entity::find()
        .column_as(GitAddon::Column::Id, "id")
        .column_as(GitAddon::Column::GitRepoId, "repo_id")
        .column_as(GitAddon::Column::Directory, "directory")
        .column_as(GitAddon::Column::Title, "title")
        .column_as(GitAddon::Column::Author, "author")
        .column_as(GitAddon::Column::Version, "version")
        .column_as(GitAddon::Column::AddOnVersion, "add_on_version")
        .column_as(GitAddon::Column::Description, "description")
        .column_as(GitAddon::Column::RemoteVersion, "remote_version")
        .column_as(
            GitAddon::Column::RemoteAddOnVersion,
            "remote_add_on_version",
        )
        .column_as(
            InstalledGitAddon::Column::InstalledVersion,
            "installed_version",
        )
        .column_as(
            InstalledGitAddon::Column::InstalledAddOnVersion,
            "installed_add_on_version",
        )
        .column_as(GitRepo::Column::Url, "repo_url")
        .left_join(InstalledGitAddon::Entity)
        .inner_join(GitRepo::Entity)
        .into_model::<GitInstalledAddon>()
        .all(db)
        .await
        .context(error::DbGetSnafu)
}

/// Collect all installed addon directory names from the AddOns folder.
fn get_all_installed_dirs(addon_dir: &Path) -> HashSet<String> {
    let mut dirs = HashSet::new();

    if let Ok(entries) = std::fs::read_dir(addon_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                && let Some(name) = entry.file_name().to_str()
            {
                dirs.insert(name.to_string());
            }
        }
    }

    dirs
}

/// Parse the manifest for a non-ESOUI addon and return its hard dependencies
/// as (dependency_dir, required_by_addon_name) pairs.
fn parse_manifest_deps(
    addon_dir: &Path,
    addon_directory: &str,
    addon_name: &str,
) -> Vec<(String, String)> {
    let addon_path = addon_dir.join(addon_directory);
    let manifest_path = addon_path.join(format!("{addon_directory}.txt"));
    let manifest_lower = addon_path.join(format!("{}.txt", addon_directory.to_lowercase()));

    let mut result = Vec::new();

    if let Some(m) =
        parse_manifest_file(&manifest_path).or_else(|| parse_manifest_file(&manifest_lower))
    {
        for dep in &m.depends_on {
            let dir_name = dep.title.clone();
            result.push((dir_name, format!("{} ({})", addon_name, addon_directory)));
        }
    }

    result
}

/// Find ESOUI addons that match a given dependency name (for suggestions).
async fn find_esoui_suggestions(db: &DatabaseConnection, dep_name: &str) -> Vec<(i64, String)> {
    let like_pattern = format!("%{}%", dep_name);
    let suggestions = DbAddon::Entity::find()
        .filter(DbAddon::Column::Name.like(&like_pattern))
        .limit(5)
        .all(db)
        .await;

    match suggestions {
        Ok(rows) => rows.into_iter().map(|a| (a.id, a.name)).collect(),
        Err(e) => {
            warn!(
                "Failed to query ESOUI suggestions for '{}': {}",
                dep_name, e
            );
            Vec::new()
        }
    }
}

/// Check dependencies for a collection of non-ESOUI addons (git or local).
/// Uses a closure to extract the directory and display name from each addon.
async fn check_non_esoui_deps<T, F>(
    db: &DatabaseConnection,
    addon_dir: &Path,
    addons: &[T],
    mut dir_name: F,
    all_dirs: &HashSet<String>,
    seen: &mut HashSet<String>,
    results: &mut Vec<AddonDepOption>,
) where
    F: FnMut(&T) -> (&str, &str),
{
    for addon in addons {
        let (dir, display_name) = dir_name(addon);
        let deps = parse_manifest_deps(addon_dir, dir, display_name);
        for (dep_name, req_by) in deps {
            if seen.contains(&dep_name) || all_dirs.contains(&dep_name) {
                continue;
            }
            seen.insert(dep_name.clone());
            let suggestions = find_esoui_suggestions(db, &dep_name).await;
            results.push(AddonDepOption {
                missing_dir: dep_name.clone(),
                required_by: req_by.clone(),
                option_id: None,
                option_name: None,
            });
            for (sid, sname) in suggestions {
                results.push(AddonDepOption {
                    missing_dir: dep_name.clone(),
                    required_by: req_by.clone(),
                    option_id: Some(sid),
                    option_name: Some(sname),
                });
            }
        }
    }
}

/// Fetch ESOUI installed addons. Scans the filesystem for manifests, detects
/// untracked addons, registers nested sub-directories and dependencies, then
/// returns the full installed set.
async fn fetch_installed_esoui_addons(
    db: &DatabaseConnection,
    addon_dir: &Path,
) -> Result<Vec<AddonShowDetails>> {
    // 1. Check for untracked installed addons
    info!("Checking for untracked addons");
    // grab every dir under Addons/
    let walker = WalkDir::new(addon_dir)
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
    let mut addon_versions: HashMap<_, _> = addon_dirs
        .into_iter()
        .map(|d| (d, String::from("0")))
        .collect();

    // now check every txt and addon file matching the directory name to get the installed version
    let parser = eso_addon_manifest::AddonManifestParser::default();
    let walker = WalkDir::new(addon_dir)
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
            .strip_prefix(addon_dir)
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
        let new_version = manifest.version.unwrap_or_else(|| "0".to_string());
        if let Some(version) = addon_versions.get(&dir_name) {
            let should_update = if version == "0" {
                !new_version.trim().is_empty() && new_version != "0"
            } else {
                match (Version::from(version), Version::from(&new_version)) {
                    (Some(current), Some(candidate)) => current < candidate,
                    (None, Some(_)) => true,
                    _ => false,
                }
            };
            if should_update {
                addon_versions.insert(dir_name.clone(), new_version);
            }
        }
        if !manifest.depends_on.is_empty() {
            manifest_deps.insert(
                dir_name,
                manifest.depends_on.into_iter().map(|d| d.title).collect(),
            );
        }
    }

    // check database for any untracked addons
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
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &sql,
            keys.iter().map(|k| k.into()).collect::<Vec<_>>(),
        ))
        .await
        .context(error::DbGetSnafu)?;
    let now = format!("{}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
    let mut detected_versions: HashMap<i64, String> = HashMap::with_capacity(db_results.len());

    for x in db_results.iter() {
        let addon_id: i64 = x.try_get_by(0).context(error::DbGetSnafu)?;
        let dir: String = x.try_get_by(2).context(error::DbGetSnafu)?;

        let candidate_version = addon_versions
            .get(&dir)
            .cloned()
            .unwrap_or_else(|| "0".to_string());

        let selected_version = detected_versions
            .entry(addon_id)
            .or_insert_with(|| "0".to_string());

        if candidate_version.trim().is_empty() || candidate_version == "0" {
            continue;
        }

        let should_update = if selected_version == "0" {
            true
        } else {
            match (
                Version::from(&*selected_version),
                Version::from(&candidate_version),
            ) {
                (Some(current), Some(candidate)) => current < candidate,
                (None, Some(_)) => true,
                _ => false,
            }
        };

        if should_update {
            *selected_version = candidate_version;
        }
    }

    let inserts: Vec<InstalledAddon::ActiveModel> = detected_versions
        .into_iter()
        .map(|(addon_id, version)| InstalledAddon::ActiveModel {
            addon_id: ActiveValue::Set(addon_id),
            version: ActiveValue::Set(version),
            date: ActiveValue::Set(now.to_string()),
        })
        .collect();

    // 2. insert as installed, update checks will handle the rest
    if !inserts.is_empty() {
        info!("Adding {} untracked addons", inserts.len());
        InstalledAddon::Entity::insert_many(inserts)
            .exec(db)
            .await
            .context(error::DbPutSnafu)?;
    }

    // Register nested sub-addon dirs under the parent's addon_id.
    if !nested_dirs.is_empty() {
        let parent_dirs: Vec<String> = nested_dirs.keys().cloned().collect();
        let parent_to_addon = resolve_dirs_to_addons(db, &parent_dirs).await?;

        let mut dir_inserts: Vec<AddonDir::ActiveModel> =
            Vec::with_capacity(nested_dirs.values().map(|v| v.len()).sum());
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
                .exec(db)
                .await;
            check_db_result(result)?;
        }
    }

    // Refresh dependency rows from manifests
    if !manifest_deps.is_empty() {
        let manifest_dirs: Vec<String> = manifest_deps.keys().cloned().collect();
        let addon_for_dir = resolve_dirs_to_addons(db, &manifest_dirs).await?;

        let affected: Vec<i64> = addon_for_dir.values().copied().collect();
        if !affected.is_empty() {
            AddonDep::Entity::delete_many()
                .filter(AddonDep::Column::AddonId.is_in(affected))
                .exec(db)
                .await
                .context(error::DbDeleteSnafu)?;
        }

        let mut dep_inserts: Vec<AddonDep::ActiveModel> =
            Vec::with_capacity(manifest_deps.values().map(|v| v.len()).sum());
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
                .exec(db)
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
        .all(db)
        .await
        .context(error::DbGetSnafu)?;
    info!("Done getting addons!");
    Ok(results)
}
