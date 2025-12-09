use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use self::backup::{BackupData, BackupInstalledAddon, BackupManualDependency};
use self::fs_util::{fs_delete_addon, fs_read_addon};
use self::result::*;
use crate::addons::{get_root_dir, Addon};
use crate::api::ApiClient;
use crate::config::{self, Config, TTCRegion};
use crate::error::{self, AddonDownloadHashSnafu, Result};
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

use bbcode_tagger::{BBCode, BBTree};
use lazy_async_promise::ImmediateValuePromise;
use md5::{Digest, Md5};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions, DatabaseConnection, DbBackend,
    DbErr, EntityTrait, FromQueryResult, IntoActiveModel, JoinType, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set, Statement,
};
use snafu::{ensure, ResultExt};
use tempfile::NamedTempFile;
use tracing::log::{self, error, info, warn};
use zip::ZipArchive;

mod backup;
mod fs_util;
pub mod result;

const TTC_URL: &str = "https://us.tamrieltradecentre.com/download/PriceTable";
const TTC_EU_URL: &str = "https://eu.tamrieltradecentre.com/download/PriceTable";

#[derive(Debug, Clone, Default)]
pub struct AddonService {
    pub api: ApiClient,
    pub config: config::Config,
    pub db: DatabaseConnection,
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
        }
    }

    pub fn install(&self, addon_id: i32, update: bool) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            service.p_install(addon_id, update).await.unwrap();
            Ok(())
        })
    }
    async fn p_install(&self, addon_id: i32, update: bool) -> Result<()> {
        self.p_update_addon_details(addon_id).await.unwrap();
        let entry = DbAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        let entry = entry.unwrap();
        let installed_entry = InstalledAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;

        if let Some(installed_entry) = installed_entry {
            if installed_entry.version == entry.version && !update {
                info!("Addon {} is already installed and up to date", entry.name);
                return Ok(());
            }
        }

        if update {
            info!("Updating addon: {addon_id}");
        } else {
            info!("Installing addon: {addon_id}");
        }

        let installed = self
            .fs_download_addon(entry.download.as_ref().unwrap().as_str(), entry.md5)
            .await?;
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
            let file_list = service.api.get_file_list().await.unwrap();

            let mut insert_addons = vec![];
            let mut insert_addon_dirs = vec![];
            let mut insert_compats = vec![];
            let mut insert_imgs = vec![];
            let mut addon_ids = vec![];
            for list_item in file_list.iter() {
                let addon_id: i32 = list_item.id.parse().unwrap();
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
                        addon_id: ActiveValue::Set(addon.id.to_owned().unwrap()),
                        dir: ActiveValue::Set(addon_dir.to_string()),
                    };
                    insert_addon_dirs.push(addon_dir_model);
                }

                // Game Compatibilty
                if let Some(compats) = &list_item.compatibility {
                    for (index, item) in compats.iter().enumerate() {
                        insert_compats.push(GameCompat::ActiveModel {
                            addon_id: ActiveValue::Set(addon.id.to_owned().unwrap()),
                            id: ActiveValue::Set(index.try_into().unwrap()),
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
                        insert_imgs.push(AddonImage::ActiveModel {
                            addon_id: ActiveValue::Set(addon.id.to_owned().unwrap()),
                            index: ActiveValue::Set(i.try_into().unwrap()),
                            thumbnail: ActiveValue::Set(thumb.to_owned()),
                            image: ActiveValue::Set(img.to_owned()),
                        })
                    }
                }

                insert_addons.push(addon);
            }
            DbAddon::Entity::insert_many(insert_addons)
                .on_conflict(
                    OnConflict::column(DbAddon::Column::Id)
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
                        .to_owned(),
                )
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;
            // delete existing addon directories in case any are removed
            AddonDir::Entity::delete_many()
                .filter(AddonDir::Column::AddonId.is_in(addon_ids.to_owned()))
                .exec(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // Add addon directories for dependency checks
            AddonDir::Entity::insert_many(insert_addon_dirs)
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;

            // Game Compatibility version
            // delete existing entries for replacement
            GameCompat::Entity::delete_many()
                .filter(GameCompat::Column::AddonId.is_in(addon_ids.to_owned()))
                .exec(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // insert new game compatibility records
            GameCompat::Entity::insert_many(insert_compats)
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;

            // AddOn Images
            // delete existing entries for replacement
            AddonImage::Entity::delete_many()
                .filter(AddonImage::Column::AddonId.is_in(addon_ids))
                .exec(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // insert new addon image URLs
            AddonImage::Entity::insert_many(insert_imgs)
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;

            let mut result = UpdateResult::default();
            if upgrade_all {
                result = service.upgrade().await.unwrap();
            }
            Ok(result)
        })
    }

    async fn p_update_addon_details(&self, id: i32) -> Result<()> {
        // check addon_detail not present or out of date
        let addon = DbAddon::Entity::find_by_id(id).one(&self.db).await.unwrap();
        let addon_detail = AddonDetail::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .unwrap();
        if addon.is_some()
            && addon_detail.is_some()
            && addon.unwrap().version == addon_detail.unwrap().version.unwrap_or_default()
        {
            // no need to update
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

        let addon = DbAddon::Entity::find_by_id(id).one(&self.db).await.unwrap();
        let mut active: DbAddon::ActiveModel = addon.unwrap().into_active_model();
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
            service.p_update_addon_details(id).await.unwrap();
            Ok(())
        })
    }

    async fn update_categories(&self) -> Result<()> {
        info!("Updating categories");
        let categories = self.api.get_categories().await?;
        let mut insert_categories = vec![];
        let mut category_parents = vec![];
        for category in categories.iter() {
            let db_category = Category::ActiveModel {
                id: ActiveValue::Set(category.id.parse().unwrap()),
                title: ActiveValue::Set(category.title.to_owned()),
                icon: ActiveValue::Set(Some(category.icon.to_owned())),
                file_count: ActiveValue::Set(Some(category.file_count.parse().unwrap())),
            };
            insert_categories.push(db_category);

            for parent_id in category.parent_ids.iter() {
                let db_parent = CategoryParent::ActiveModel {
                    id: ActiveValue::Set(category.id.parse().unwrap()),
                    parent_id: ActiveValue::Set(parent_id.parse().unwrap()),
                };
                category_parents.push(db_parent);
            }
        }
        let result = Category::Entity::insert_many(insert_categories)
            .on_conflict(
                OnConflict::column(Category::Column::Id)
                    .update_columns([
                        Category::Column::Title,
                        Category::Column::Icon,
                        Category::Column::FileCount,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await;
        check_db_result(result)?;
        let result = CategoryParent::Entity::insert_many(category_parents)
            .on_conflict(
                OnConflict::columns([CategoryParent::Column::Id, CategoryParent::Column::ParentId])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&self.db)
            .await;
        // for some reason the ensure check for the previous Category insert result check doesn't work here
        check_db_result(result)?;
        Ok(())
    }

    pub fn remove(&self, addon_id: i32) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Removing addon with id: {addon_id}");
            // check if valid addon ID
            let addon = DbAddon::Entity::find_by_id(addon_id)
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            match addon {
                Some(_) => {}
                None => {
                    warn!("Not a valid addon ID!");
                    return Ok(());
                }
            }
            // check if installed before removing
            let addon = addon.unwrap();
            let installed_addon = addon
                .find_related(InstalledAddon::Entity)
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            match installed_addon {
                Some(_) => {}
                None => {
                    error!("Addon not installed!");
                    return Ok(());
                }
            }
            // get installed dirs
            let installed_dirs = addon
                .find_related(AddonDir::Entity)
                .all(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            // delete from installed
            installed_addon
                .unwrap()
                .delete(&service.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // delete installed addon directories
            fs_delete_addon(&service.get_addon_dir(), &installed_dirs).unwrap();
            info!("Removed addon {}", addon.name);

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
        // let mut return_results = vec![];
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            info!("Getting installed addons");
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
        left outer join manual_dependency m on dependency_dir = m.addon_dir
        where
            dependency_dir not in (select addon_dir from manual_dependency)"#,
                [],
            ))
            .all(&db)
            .await
            .context(error::DbGetSnafu)
            .unwrap();
            Ok(results)
        })
    }

    pub fn get_addon_details(
        &self,
        addon_id: i32,
    ) -> ImmediateValuePromise<Option<AddonShowDetails>> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            // check if we need to grab it, grab if needed
            service.p_update_addon_details(addon_id).await.unwrap();

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
                    Condition::any().add(
                        GameCompat::Column::Id
                            .eq(0)
                            .add(GameCompat::Column::Id.is_null()),
                    ),
                )
                .into_model::<AddonShowDetails>()
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)
                .unwrap();
            Ok(result)
        })
    }

    pub fn parse_bbcode(&self, text: String) -> ImmediateValuePromise<BBTree> {
        ImmediateValuePromise::new(async move {
            let parser = BBCode::default();
            let result = parser.parse(&text);
            Ok(result)
        })
    }

    // region: Config

    pub fn save_config(&self) {
        self.config.save().unwrap();
    }

    fn get_addon_dir(&self) -> PathBuf {
        self.config.addon_dir.clone()
    }

    // endregion

    async fn base_fs_download_extract(
        &self,
        url: &str,
        path_addr: Option<&str>,
        md5: Option<String>,
    ) -> Result<ZipArchive<File>> {
        let response = tokio::join!(async move {
            self.api
                .download_file(url)
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap()
        })
        .0;

        let mut tmpfile = NamedTempFile::new().context(error::AddonDownloadTmpFileSnafu)?;
        let mut r_tmpfile = tmpfile
            .reopen()
            .context(error::AddonDownloadTmpFileReadSnafu)?;
        tmpfile
            .write_all(response.as_ref())
            .context(error::AddonDownloadTmpFileWriteSnafu)?;
        r_tmpfile.rewind().unwrap();

        // check hash if present
        if let Some(md5) = md5 {
            if !md5.trim().is_empty() {
                let mut hasher = Md5::new();
                io::copy(&mut r_tmpfile, &mut hasher).unwrap();
                let hash = hasher.finalize().to_vec();
                let mut hash_string = String::new();
                for x in hash.iter() {
                    hash_string.push_str(format!("{x:02x}").as_str());
                }
                ensure!(
                    md5 == hash_string,
                    AddonDownloadHashSnafu {
                        file_name: String::from(url),
                        expected_hash: md5,
                        actual_hash: hash_string
                    }
                );
                r_tmpfile.rewind().unwrap();
            }
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
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p)
                            .context(error::AddonDownloadZipExtractSnafu { path: p })?;
                    }
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

        let addon = fs_read_addon(&addon_path);

        Ok(addon.unwrap())
    }

    pub fn update_ttc_pricetable(&self) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Updating TTC PriceTable");
            if service.config.ttc_region == TTCRegion::NA
                || service.config.ttc_region == TTCRegion::ALL
            {
                service
                    .base_fs_download_extract(TTC_URL, Some("TamrielTradeCentre"), None)
                    .await?;
            }
            if service.config.ttc_region == TTCRegion::EU
                || service.config.ttc_region == TTCRegion::ALL
            {
                service
                    .base_fs_download_extract(TTC_EU_URL, Some("TamrielTradeCentre"), None)
                    .await?;
            }
            Ok(())
        })
    }

    pub fn import_minion_file(&mut self, file: &Path) -> ImmediateValuePromise<()> {
        // Takes a path to a minion backup file, it should be named something like `BU-addons.txt`
        // It should contain a single line of comma-separated addon IDs
        let service = self.clone();
        let filepath = file.to_path_buf();

        ImmediateValuePromise::new(async move {
            // Update should already be called on app init, so main addon table should be populated
            // If called on a new database, the main addon table will be empty. As a workaround, call `update()`.
            // self.update(false).await.unwrap();

            let line = fs::read_to_string(filepath).unwrap();
            let ids: Vec<i32> = line
                .split(',')
                .filter(|&x| !x.is_empty())
                .map(|x| x.parse::<i32>().unwrap())
                .collect();
            // workaround for weird behavior with promise in promise, slowly install addons one at a time
            for addon_id in ids.iter() {
                service.p_install(*addon_id, false).await.unwrap();
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
                .context(error::DbGetSnafu)
                .unwrap();
            Ok(categories)
        })
    }
    pub fn get_category_parents(&self) -> ImmediateValuePromise<Vec<ParentCategory>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            // select on Category instead
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
                .context(error::DbGetSnafu)
                .unwrap();
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
                    .context(error::DbGetSnafu)
                    .unwrap();
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
                .context(error::DbGetSnafu)
                .unwrap();
            // addons.truncate(100);
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
                        service.p_install(satisfied_by, false).await.unwrap();
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

    pub fn update_hm_data(&self) -> ImmediateValuePromise<()> {
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

            let base_dir = config.addon_dir.parent().unwrap();
            let saved_var_dir = base_dir.join("SavedVariables");
            let addon_dir = config.addon_dir.join("HarvestMapData");
            let mut empty_file = addon_dir.join("Main");
            empty_file.push("emptyTable.lua");

            let empty_file_data = fs::read_to_string(empty_file)?;

            // iterate over the different zones
            for zone in ["AD", "EP", "DC", "DLC", "NF"] {
                let file_name = format!("HarvestMap{zone}.lua");
                info!("Working on {file_name}...");

                let sv_fn1 = saved_var_dir.join(file_name.clone());
                let sv_fn2 = saved_var_dir.join(format!("{file_name}~"));

                // if save var file exists, create backup...
                if sv_fn1.exists() {
                    fs::copy(sv_fn1, sv_fn2.clone())?;
                } else {
                    // ... else, use empty table to create a placeholder
                    let file_data = format!("Harvest{zone}_SavedVars{}", empty_file_data.as_str());
                    let mut output = File::create(sv_fn2.clone())?;
                    write!(output, "{file_data}")?;
                }

                let sv_fn2_data = fs::read_to_string(sv_fn2).unwrap();
                let result = api.get_hm_data(sv_fn2_data).await.unwrap();

                let mut out_file = addon_dir.join("Modules");
                out_file.push(format!("HarvestMap{zone}"));
                if !out_file.exists() {
                    // create modules zone dir
                    fs::create_dir(out_file.clone()).unwrap();
                }
                out_file.push(file_name);
                let mut output = File::create(out_file)?;
                write!(output, "{}", result.text().await.unwrap()).unwrap();
            }
            info!("Done HarvestMap data!");
            Ok(())
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
}

/// Use for inserts where no updates/inserts OK
/// sea_orm now returns DbErr::RecordNotInserted when no inserts
fn check_db_result<T>(result: Result<T, DbErr>) -> Result<()> {
    match result {
        Ok(r) => Ok(Some(r)),
        Err(DbErr::RecordNotInserted) => Ok(None),
        Err(e) => Err(e),
    }
    .unwrap();
    Ok(())
}
