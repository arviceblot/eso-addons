use crate::error::{self, Result};
use std::fs::{self, File};

use crate::addons::{get_root_dir, Addon};
use crate::api::ApiClient;
use crate::config::{self, get_config_dir, EAM_CONF, EAM_DB};
use entity::addon as DbAddon;
use entity::addon_dependency as AddonDep;
use entity::addon_detail as AddonDetail;
use entity::addon_dir as AddonDir;
use entity::category as Category;
use entity::category_parent as CategoryParent;
use entity::installed_addon as InstalledAddon;
use migration::{Condition, Migrator, MigratorTrait};
use sea_orm::sea_query::{Expr, OnConflict, Query};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, JoinType,
    ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
};
use snafu::ResultExt;
use std::io::{self, Seek, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use zip::ZipArchive;

use self::fs_util::{fs_delete_addon, fs_get_addons, fs_read_addon};
use self::result::*;

mod fs_util;
pub mod result;

const TTC_URL: &str = "https://us.tamrieltradecentre.com/download/PriceTable";

#[derive(Debug)]
pub struct AddonService {
    pub api: ApiClient,
    pub config: config::Config,
    config_filepath: PathBuf,
    pub db: DatabaseConnection,
}
impl AddonService {
    pub async fn new() -> AddonService {
        // setup config
        let config_dir = get_config_dir();
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).unwrap();
        }
        let config_filepath = config_dir.join(EAM_CONF);
        let config = config::parse_config(&config_filepath).unwrap();

        // init api/download client
        // TODO: consider moving endpoint_url to config as default value
        let mut client = ApiClient::new("https://api.mmoui.com/v3");
        client.update_endpoints_from_config(&config);

        // create db file if not exists
        let db_file = config_dir.join(EAM_DB);
        if !db_file.exists() {
            File::create(&db_file).unwrap();
        }
        // setup database connection and apply migrations if needed
        let database_url = format!("sqlite://{}", db_file.to_string_lossy());
        let db = sea_orm::Database::connect(&database_url).await.unwrap();
        Migrator::up(&db, None).await.unwrap();

        AddonService {
            api: client,
            config,
            config_filepath,
            db,
        }
    }

    pub async fn install(&self, addon_id: i32, update: bool) -> Result<()> {
        let entry = DbAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        let mut entry: DbAddon::ActiveModel = entry.unwrap().into();
        let installed_entry = InstalledAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        let file_details = self.api.get_file_details(addon_id).await?;

        if let Some(installed_entry) = installed_entry {
            if installed_entry.date == file_details.date.to_string() && !update {
                println!(
                    "Addon {} is already installed and up to date",
                    entry.name.unwrap()
                );
                return Ok(());
            }
        }

        entry.download = ActiveValue::Set(Some(file_details.download_url.to_owned()));
        entry.version = ActiveValue::Set(file_details.version.to_owned());
        entry.date = ActiveValue::Set(file_details.date.to_string());
        entry.md5 = ActiveValue::Set(Some(file_details.md5));
        entry.file_name = ActiveValue::Set(Some(file_details.file_name));
        entry.download = ActiveValue::Set(Some(file_details.download_url.to_owned()));

        let installed = self
            .fs_download_addon(&file_details.download_url.to_owned())
            .await
            .unwrap();
        let installed_entry = InstalledAddon::ActiveModel {
            addon_id: ActiveValue::Set(addon_id),
            version: ActiveValue::Set(file_details.version),
            date: ActiveValue::Set(file_details.date.to_string()),
        };
        entry.update(&self.db).await.unwrap();

        InstalledAddon::Entity::insert(installed_entry)
            .on_conflict(
                OnConflict::column(InstalledAddon::Column::AddonId)
                    .update_columns([
                        InstalledAddon::Column::Date,
                        InstalledAddon::Column::Version,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await
            .context(error::DbPutSnafu)?;

        let addon_detail = AddonDetail::ActiveModel {
            id: ActiveValue::Set(addon_id),
            description: ActiveValue::Set(Some(file_details.description)),
            change_log: ActiveValue::Set(Some(file_details.change_log)),
        };
        AddonDetail::Entity::insert(addon_detail)
            .on_conflict(
                OnConflict::column(AddonDetail::Column::Id)
                    .update_columns([
                        AddonDetail::Column::Description,
                        AddonDetail::Column::ChangeLog,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await
            .context(error::DbPutSnafu)?;

        // get addon IDs from dependency dirs, there may be more than on for each directory
        if !installed.depends_on.is_empty() {
            let deps = installed.depends_on.iter().map(|x| AddonDep::ActiveModel {
                addon_id: ActiveValue::Set(addon_id),
                dependency_dir: ActiveValue::Set(x.to_owned()),
            });
            // insert all dependencies
            AddonDep::Entity::insert_many(deps)
                .on_conflict(
                    OnConflict::columns([
                        AddonDep::Column::AddonId,
                        AddonDep::Column::DependencyDir,
                    ])
                    .do_nothing()
                    .to_owned(),
                )
                .exec(&self.db)
                .await
                .context(error::DbPutSnafu)?;
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<UpdateResult> {
        // update endpoints from api
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
                Expr::tbl(InstalledAddon::Entity, InstalledAddon::Column::Date)
                    .less_than(Expr::tbl(DbAddon::Entity, DbAddon::Column::Date)),
            )
            .into_model::<AddonDetails>()
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        for update in updates.iter() {
            self.install(update.id, true).await.unwrap();
        }

        let need_installs = self.get_missing_dependency_options().await;

        self.config.file_details = self.api.file_details_url.to_owned();
        self.config.file_list = self.api.file_list_url.to_owned();
        self.config.list_files = self.api.list_files_url.to_owned();
        self.config.category_list = self.api.category_list_url.to_owned();

        config::save_config(&self.config_filepath, &self.config).unwrap();

        Ok(UpdateResult {
            addons_updated: updates,
            missing_deps: need_installs,
        })
    }

    async fn update_categories(&self) -> Result<()> {
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
        Category::Entity::insert_many(insert_categories)
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
            .await
            .context(error::DbPutSnafu)?;
        CategoryParent::Entity::insert_many(category_parents)
            .on_conflict(
                OnConflict::columns([CategoryParent::Column::Id, CategoryParent::Column::ParentId])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&self.db)
            .await
            .context(error::DbPutSnafu)?;
        Ok(())
    }

    pub async fn remove(&self, addon_id: i32) -> Result<()> {
        // check if valid addon ID
        let addon = DbAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
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
            .one(&self.db)
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
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        // delete from installed
        installed_addon
            .unwrap()
            .delete(&self.db)
            .await
            .context(error::DbDeleteSnafu)?;
        // delete installed addon directories
        fs_delete_addon(&self.get_addon_dir(), &installed_dirs).unwrap();

        Ok(())
    }

    pub async fn search(&self, search_string: &String) -> Result<Vec<SearchDbAddon>> {
        let mut results = vec![];
        let addons = DbAddon::Entity::find()
            .find_also_related(InstalledAddon::Entity)
            .filter(DbAddon::Column::Name.like(format!("%{search_string}%").as_str()))
            .order_by_desc(DbAddon::Column::Date)
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        for (addon, installed) in addons.iter() {
            let mut search_addon: SearchDbAddon = addon.into();
            if installed.is_some() {
                search_addon.installed = true;
            }
            results.push(search_addon);
        }
        Ok(results)
    }

    pub async fn get_installed_addon_count(&self) -> Result<i32> {
        let count = InstalledAddon::Entity::find()
            .count(&self.db)
            .await
            .context(error::DbGetSnafu)? as i32;
        Ok(count)
    }

    pub async fn get_installed_addons(&self) -> Result<Vec<AddonShowDetails>> {
        // let mut return_results = vec![];
        let results = DbAddon::Entity::find()
            .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
            .column_as(Category::Column::Title, "category")
            .inner_join(Category::Entity)
            .inner_join(InstalledAddon::Entity)
            .into_model::<AddonShowDetails>()
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)?;
        Ok(results)
    }

    pub async fn get_missing_dependency_options(&self) -> Vec<AddonDepOption> {
        let need_installs = InstalledAddon::Entity::find()
            .columns([DbAddon::Column::Id, DbAddon::Column::Name])
            .column(AddonDir::Column::Dir)
            .join(JoinType::InnerJoin, InstalledAddon::Relation::Addon.def())
            .join(JoinType::InnerJoin, DbAddon::Relation::AddonDir.def())
            .join(
                JoinType::InnerJoin,
                DbAddon::Relation::AddonDependency.def(),
            )
            // ^^^ might have to replace with manual join, as the relation is set up in the other direction
            .filter(
                Condition::any().add(
                    AddonDir::Column::Dir.not_in_subquery(
                        Query::select()
                            .column(AddonDir::Column::Dir)
                            .distinct()
                            .from(AddonDir::Entity)
                            .inner_join(
                                InstalledAddon::Entity,
                                Expr::tbl(AddonDir::Entity, AddonDir::Column::AddonId).equals(
                                    InstalledAddon::Entity,
                                    InstalledAddon::Column::AddonId,
                                ),
                            )
                            .to_owned(),
                    ),
                ),
            )
            .order_by_asc(AddonDir::Column::Dir)
            .into_model::<AddonDepOption>()
            .all(&self.db)
            .await
            .context(error::DbGetSnafu)
            .unwrap();

        need_installs
    }

    pub async fn get_addon_details(&self, addon_id: i32) -> Result<Option<AddonShowDetails>> {
        // TODO: Move update API call out of get details
        // first, update the details from API
        let details = self.api.get_file_details(addon_id).await?;
        // now update the db record
        let addon = DbAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)
            .unwrap();
        let mut addon: DbAddon::ActiveModel = addon.unwrap().into();
        addon.md5 = Set(Some(details.md5));
        addon.file_name = Set(Some(details.file_name));
        addon.download = Set(Some(details.download_url));
        addon.update(&self.db).await.unwrap();
        // and update the details record
        let addon_dets = AddonDetail::ActiveModel {
            id: ActiveValue::set(addon_id),
            description: ActiveValue::set(Some(details.description)),
            change_log: ActiveValue::set(Some(details.change_log)),
        };
        AddonDetail::Entity::insert(addon_dets)
            .on_conflict(
                OnConflict::column(AddonDetail::Column::Id)
                    .update_columns([
                        AddonDetail::Column::Description,
                        AddonDetail::Column::ChangeLog,
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await
            .context(error::DbPutSnafu)?;
        let result = DbAddon::Entity::find_by_id(addon_id)
            .column_as(InstalledAddon::Column::AddonId.is_not_null(), "installed")
            .column_as(Category::Column::Title, "category")
            .inner_join(Category::Entity)
            .inner_join(AddonDetail::Entity)
            .left_join(InstalledAddon::Entity)
            .into_model::<AddonShowDetails>()
            .one(&self.db)
            .await
            .context(error::DbGetSnafu)
            .unwrap();
        Ok(result)
    }

    fn get_addon_dir(&self) -> PathBuf {
        self.config.addon_dir.clone()
    }

    async fn base_fs_download_extract(
        &self,
        url: &str,
        path_addr: Option<&str>,
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

        let mut archive =
            zip::ZipArchive::new(r_tmpfile).context(error::AddonDownloadZipCreateSnafu)?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .context(error::AddonDownloadZipReadSnafu { file: i })?;
            let outpath = match file.enclosed_name() {
                Some(path) => {
                    let mut p = self.get_addon_dir().clone();
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

    pub async fn fs_download_addon(&self, url: &str) -> Result<Addon> {
        let mut archive = self.base_fs_download_extract(url, None).await.unwrap();
        let mut addon_path = self.get_addon_dir();
        let addon_name = archive
            .by_index(0)
            .context(error::AddonDownloadZipReadSnafu { file: 0_usize })?;
        let addon_name = get_root_dir(&addon_name.mangled_name());
        addon_path.push(addon_name);

        let addon = fs_read_addon(&addon_path);

        Ok(addon.unwrap())
    }

    pub async fn update_ttc_pricetable(&self) -> Result<()> {
        self.base_fs_download_extract(TTC_URL, Some("TamrielTradeCentre"))
            .await
            .unwrap();
        Ok(())
    }

    pub fn save_config(&self) {
        config::save_config(&self.config_filepath, &self.config).unwrap();
    }
}
