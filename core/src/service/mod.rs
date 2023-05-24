use crate::error::{self, Result};
use std::collections::HashMap;
use std::fs::{self, File};

use crate::addons::{get_root_dir, Addon};
use crate::api::ApiClient;
use crate::config::{self, Config};
use entity::addon as DbAddon;
use entity::addon_dependency as AddonDep;
use entity::addon_detail as AddonDetail;
use entity::addon_dir as AddonDir;
use entity::category as Category;
use entity::category_parent as CategoryParent;
use entity::installed_addon as InstalledAddon;
use lazy_async_promise::{DirectCacheAccess, ImmediateValuePromise, ImmediateValueState};
use migration::{Condition, Migrator, MigratorTrait};
use sea_orm::sea_query::{Expr, OnConflict, Query};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions, DatabaseConnection, DbBackend,
    DbErr, EntityTrait, FromQueryResult, IntoActiveModel, JoinType, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set, Statement,
};
use snafu::ResultExt;
use std::io::{self, Seek, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;
use tracing::log::{self, info};
use zip::ZipArchive;

use self::fs_util::{fs_delete_addon, fs_read_addon};
use self::result::*;

use bbcode_tagger::{BBCode, BBTree};

mod fs_util;
pub mod result;

const TTC_URL: &str = "https://us.tamrieltradecentre.com/download/PriceTable";

pub enum ServiceResult {
    Default(()),
    AddonShowDetails(Vec<AddonShowDetails>),
    ZipFile(ZipArchive<File>),
}

#[derive(Default)]
pub struct AddonService {
    pub api: ApiClient,
    pub config: config::Config,
    pub db: DatabaseConnection,
    promises: Arc<Mutex<HashMap<i32, ImmediateValuePromise<ServiceResult>>>>,
    last_promise: i32,
}
impl AddonService {
    pub fn new() -> ImmediateValuePromise<AddonService> {
        ImmediateValuePromise::new(async move {
            // setup config
            let config = Config::default();

            // init api/download client
            // TODO: consider moving endpoint_url to config as default value
            let mut client = ApiClient::new("https://api.mmoui.com/v3");
            client.update_endpoints_from_config(&config);

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

            Ok(AddonService {
                api: client,
                config,
                db,
                ..Default::default()
            })
        })
    }

    pub fn poll(&mut self) {
        for promise in self.promises.try_lock().unwrap().values() {
            promise.poll_state();
        }
    }

    pub fn is_polling(&self, id: i32) -> bool {
        if let Some(promise) = self.promises.try_lock().unwrap().get(&id) {
            match promise.get_state() {
                ImmediateValueState::Updating => return true,
                _ => return false,
            }
        }
        false
    }

    pub fn handle(&mut self, id: i32) -> Option<&ServiceResult> {
        if let Some(promise) = self.promises.try_lock().unwrap().get(&id) {
            match promise.get_state() {
                ImmediateValueState::Success(_) => {
                    self.promises.try_lock().unwrap().remove(&id);
                    return promise.get_value();
                }
                _ => return None,
            }
        }
        None
    }

    pub fn install(&mut self, addon_id: i32, update: bool) -> ImmediateValuePromise<()> {
        ImmediateValuePromise::new(async move {
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
                info!("Updating addon: {}", addon_id);
            } else {
                info!("Installing addon: {}", addon_id);
            }

            let mut installed_promise =
                self.fs_download_addon(entry.download.as_ref().unwrap().as_str());
            loop {
                let state = installed_promise.poll_state();
                match state {
                    ImmediateValueState::Success(_) => break,
                    ImmediateValueState::Error(_) => break,
                    _ => continue,
                }
            }
            let installed = installed_promise.get_value().unwrap();
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
            Ok(())
        })
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
        for update in updates.iter() {
            // self.install(update.id, true).await.unwrap(); // TODO: add back?
        }
        let need_installs = self.get_missing_dependency_options().await;

        let mut result = UpdateResult {
            missing_deps: need_installs,
            ..Default::default()
        };

        Ok(result)
    }

    pub fn update(&mut self, upgrade_all: bool) -> ImmediateValuePromise<UpdateResult> {
        ImmediateValuePromise::new(async move {
            // update endpoints from api
            info!("Updating endpoints");
            self.api.update_endpoints().await.unwrap();

            // update categories
            self.update_categories().await?;

            // update addons
            let file_list = self.api.get_file_list().await.unwrap();

            let mut insert_addons = vec![];
            let mut insert_addon_dirs = vec![];
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
                for addon_dir in list_item.directories.iter() {
                    let addon_dir_model = AddonDir::ActiveModel {
                        addon_id: ActiveValue::Set(addon.id.to_owned().unwrap()),
                        dir: ActiveValue::Set(addon_dir.to_string()),
                    };
                    insert_addon_dirs.push(addon_dir_model);
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
                .exec(&self.db)
                .await
                .context(error::DbPutSnafu)?;
            // delete existing addon directories in case any are removed
            AddonDir::Entity::delete_many()
                .filter(AddonDir::Column::AddonId.is_in(addon_ids))
                .exec(&self.db)
                .await
                .context(error::DbDeleteSnafu)?;
            // Add addon directories for dependency checks
            AddonDir::Entity::insert_many(insert_addon_dirs)
                .exec(&self.db)
                .await
                .context(error::DbPutSnafu)?;

            let mut result = UpdateResult::default();
            if upgrade_all {
                result = self.upgrade().await.unwrap();
            } else {
                let need_installs = self.get_missing_dependency_options().await;
                result.missing_deps = need_installs;
            }

            // find addon details where we have the older version
            result.missing_details = self.get_missing_addon_detail_ids().await?;

            info!("Saving config");
            self.config.file_details = self.api.file_details_url.to_owned();
            self.config.file_list = self.api.file_list_url.to_owned();
            self.config.list_files = self.api.list_files_url.to_owned();
            self.config.category_list = self.api.category_list_url.to_owned();

            self.config.save()?;

            Ok(result)
        })
    }

    async fn get_missing_addon_detail_ids(&self) -> Result<Vec<i32>> {
        let mut results = vec![];
        info!("Getting addons with missing or outdated details");
        let addons = DbAddon::Entity::find()
            .left_join(AddonDetail::Entity)
            .filter(
                Condition::any()
                    .add(AddonDetail::Column::Version.is_null())
                    .add(
                        Expr::col((DbAddon::Entity, DbAddon::Column::Version)).ne(Expr::col((
                            AddonDetail::Entity,
                            AddonDetail::Column::Version,
                        ))),
                    )
                    .add(DbAddon::Column::Md5.is_null())
                    .add(DbAddon::Column::FileName.is_null())
                    .add(DbAddon::Column::Download.is_null())
                    .to_owned(),
            )
            .all(&self.db)
            .await
            .context(error::DbPutSnafu)?;
        if addons.is_empty() {
            return Ok(results);
        }
        results = addons.iter().map(|x| x.id).collect();
        Ok(results)
    }

    pub fn update_addon_details(&self, id: i32) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            info!("Updating addon details for addon: {}", id);

            let file_details = service.api.get_file_details(id).await?;
            let record = AddonDetail::ActiveModel {
                id: ActiveValue::Set(id),
                description: ActiveValue::Set(Some(file_details.description)),
                change_log: ActiveValue::Set(Some(file_details.change_log)),
                version: ActiveValue::Set(Some(file_details.version)),
            };

            let addon = DbAddon::Entity::find_by_id(id).one(&service.db).await?;
            let mut active: DbAddon::ActiveModel = addon.unwrap().into_active_model();
            active.md5 = Set(Some(file_details.md5.to_owned()));
            active.file_name = Set(Some(file_details.file_name.to_owned()));
            active.download = Set(Some(file_details.download_url.to_owned()));
            active
                .update(&service.db)
                .await
                .context(error::DbPutSnafu)?;

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
                .exec(&service.db)
                .await
                .context(error::DbPutSnafu)?;

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
            // check if valid addon ID
            let addon = DbAddon::Entity::find_by_id(addon_id)
                .one(&service.db)
                .await
                .context(error::DbGetSnafu)?;
            match addon {
                Some(_) => {}
                None => {
                    println!("Not a valid addon ID!");
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
                    println!("Addon not installed!");
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

    pub async fn get_installed_addon_count(&self) -> Result<i32> {
        let count = InstalledAddon::Entity::find()
            .count(&self.db)
            .await
            .context(error::DbGetSnafu)? as i32;
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

    pub async fn get_missing_dependency_options(&self) -> Vec<AddonDepOption> {
        info!("Checking for missing dependencies");

        AddonDepOption::find_by_statement(Statement::from_sql_and_values(
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
            m.addon_dir is NULL
            or m.ignore <> 1"#,
            [],
        ))
        .all(&self.db)
        .await
        .context(error::DbGetSnafu)
        .unwrap()
    }

    pub fn get_addon_details(
        &self,
        addon_id: i32,
    ) -> ImmediateValuePromise<Option<AddonShowDetails>> {
        let db = self.db.clone();
        ImmediateValuePromise::new(async move {
            let result = DbAddon::Entity::find_by_id(addon_id)
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(Category::Column::Title, "category")
                .column_as(AddonDetail::Column::Description, "description")
                .column_as(AddonDetail::Column::ChangeLog, "change_log")
                .inner_join(Category::Entity)
                .inner_join(AddonDetail::Entity)
                .left_join(InstalledAddon::Entity)
                .into_model::<AddonShowDetails>()
                .one(&db)
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

    fn get_addon_dir(&self) -> PathBuf {
        self.config.addon_dir.clone()
    }

    fn base_fs_download_extract(&self, url: &str, path_addr: Option<&str>) -> i32 {
        let api = &self.api;
        let addon_dir = self.get_addon_dir().clone();
        let promise = ImmediateValuePromise::new(async move {
            let response = api.download_file(url).await.unwrap().bytes().await.unwrap();

            let mut tmpfile = NamedTempFile::new().context(error::AddonDownloadTmpFileSnafu)?;
            let mut r_tmpfile = tmpfile
                .reopen()
                .context(error::AddonDownloadTmpFileReadSnafu)?;
            tmpfile
                .write_all(response.as_ref())
                .context(error::AddonDownloadTmpFileWriteSnafu)?;
            r_tmpfile.rewind().unwrap();

            let mut archive =
                zip::ZipArchive::new(r_tmpfile).context(error::AddonDownloadZipCreateSnafu)?;

            for i in 0..archive.len() {
                let mut file = archive
                    .by_index(i)
                    .context(error::AddonDownloadZipReadSnafu { file: i })?;
                let outpath = match file.enclosed_name() {
                    Some(path) => {
                        let mut p = addon_dir.clone();
                        if path_addr.is_some() {
                            // append additional path if defined
                            p.push(path_addr.unwrap());
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
                    let mut outfile = fs::File::create(&outpath).context(
                        error::AddonDownloadZipExtractSnafu {
                            path: outpath.to_owned(),
                        },
                    )?;
                    io::copy(&mut file, &mut outfile)
                        .context(error::AddonDownloadZipExtractSnafu { path: outpath })?;
                }
            }

            Ok(ServiceResult::ZipFile(archive))
        });
        self.last_promise += 1;
        let id = self.last_promise;
        self.promises.try_lock().unwrap().insert(id, promise);
        id
    }

    fn fs_download_addon(&self, url: &str) -> ImmediateValuePromise<Addon> {
        ImmediateValuePromise::new(async move {
            let archive = self.base_fs_download_extract(url, None);
            loop {
                let state = archive.poll_state();
                match state {
                    ImmediateValueState::Success(_) => break,
                    ImmediateValueState::Error(_) => break,
                    _ => continue,
                }
            }
            let mut addon_path = self.get_addon_dir();
            let addon_name = archive
                .get_value()
                .unwrap()
                .by_index(0)
                .context(error::AddonDownloadZipReadSnafu { file: 0_usize })
                .unwrap();
            let addon_name = get_root_dir(&addon_name.mangled_name());
            addon_path.push(addon_name);

            let addon = fs_read_addon(&addon_path);

            Ok(addon.unwrap())
        })
    }

    pub fn update_ttc_pricetable(&self) -> i32 {
        let promise = ImmediateValuePromise::new(async move {
            info!("Updating TTC PriceTable");
            let promise = self.base_fs_download_extract(TTC_URL, Some("TamrielTradeCentre"));
            let result: Option<&ServiceResult>;
            loop {
                if !self.is_polling(promise) {
                    result = Some(self.handle(promise).unwrap());
                    break;
                }
            }
            // match result {}
            Ok(ServiceResult::Default(()))
        });
        self.last_promise += 1;
        let id = self.last_promise;
        self.promises.try_lock().unwrap().insert(id, promise);
        id
    }

    pub fn save_config(&self) {
        self.config.save().unwrap();
    }

    pub fn import_minion_file(&mut self, file: &PathBuf) -> ImmediateValuePromise<()> {
        // Takes a path to a minion backup file, it should be named something like `BU-addons.txt`
        // It should contain a single line of comma-separated addon IDs
        let filepath = file.clone();

        ImmediateValuePromise::new(async move {
            // Update should already be called on app init, so main addon table should be populated
            // If called on a new database, the main addon table will be empty. As a workaround, call `update()`.
            // self.update(false).await.unwrap();

            let mut install_promises = HashMap::new();

            let line = fs::read_to_string(filepath).unwrap();
            let ids: Vec<i32> = line
                .split(',')
                .filter(|&x| !x.is_empty())
                .map(|x| x.parse::<i32>().unwrap())
                .collect();
            for addon_id in ids.iter() {
                install_promises.insert(*addon_id, self.install(*addon_id, false));
            }

            while !install_promises.is_empty() {
                let mut remove_ids: Vec<i32> = vec![];
                for (addon_id, promise) in install_promises.iter_mut() {
                    let state = promise.poll_state();
                    if let ImmediateValueState::Success(_) = state {
                        remove_ids.push(addon_id.to_owned());
                    }
                }
                for addon_id in remove_ids.iter() {
                    install_promises.remove(addon_id);
                }
            }
            Ok(())
        })
    }

    pub fn get_category_parents(&self) -> ImmediateValuePromise<Vec<ParentCategory>> {
        let db = &self.db;
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
                .all(db)
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
                    .all(db)
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

    pub fn get_addons_by_category(&self, category_id: i32) -> i32 {
        let db = &self.db;
        let promise = ImmediateValuePromise::new(async move {
            let mut addons = DbAddon::Entity::find()
                .column_as(DbAddon::Column::Version, "version")
                .column_as(InstalledAddon::Column::Version, "installed_version")
                .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
                .column_as(Category::Column::Title, "category")
                .column_as(Expr::value("NULL"), "description")
                .column_as(Expr::value("NULL"), "change_log")
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
                .all(db)
                .await
                .context(error::DbGetSnafu)
                .unwrap();
            addons.truncate(100);
            Ok(ServiceResult::AddonShowDetails(addons))
        });
        self.last_promise += 1;
        let id = self.last_promise;
        self.promises.try_lock().unwrap().insert(id, promise);
        id
    }
    pub fn install_missing_dependencies(
        &self,
        dep_results: HashMap<String, MissingDepView>,
    ) -> ImmediateValuePromise<()> {
        let service = self.clone();
        ImmediateValuePromise::new(async move {
            // install selected IDs if not installed
            for (dir, dep_opt) in dep_results.iter() {
                if let Some(satisfied_by) = dep_opt.satisfied_by {
                    // if it's in the options, it means not installed
                    if dep_opt.options.contains_key(&satisfied_by) {
                        // install addon
                        let mut promise = service.install(satisfied_by, false);
                        loop {
                            let state = promise.poll_state();
                            if let ImmediateValueState::Success(_) = state {
                                break;
                            }
                        }
                    }
                }
            }
            // insert dep options
            Ok(())
        })
    }
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
