use std::error::Error;
use std::fs::{self, File};
use std::path::PathBuf;

use crate::addons::Manager;
use crate::config::{self, EAM_CONF, EAM_DATA_DIR, EAM_DB};
use entity::addon as DbAddon;
use entity::addon_dependency as AddonDep;
use entity::addon_dir as AddonDir;
use entity::installed_addon as InstalledAddon;
use eso_addons_api::ApiClient;
use migration::{Migrator, MigratorTrait};
use sea_orm::sea_query::OnConflict;
use sea_orm::EntityTrait;
use sea_orm::FromQueryResult;
use sea_orm::Statement;
use sea_orm::{ActiveValue, DatabaseBackend};
use sea_orm::{ColumnTrait, DatabaseConnection, QueryFilter};

#[derive(FromQueryResult)]
pub struct AddonDepOption {
    id: i32,
    name: String,
    dir: String,
}

pub struct AddonService {
    api: ApiClient,
    addons: Manager,
    config: config::Config,
    config_filepath: PathBuf,
    db: DatabaseConnection,
}

impl AddonService {
    pub async fn new() -> AddonService {
        let config_dir = dirs::config_dir().unwrap().join(EAM_DATA_DIR);
        if !config_dir.exists() {
            fs::create_dir_all(config_dir.to_owned()).unwrap();
        }
        let config_filepath = config_dir.join(EAM_CONF);
        let config = config::parse_config(&config_filepath).unwrap();
        let addon_manager = Manager::new(&config.addon_dir);

        let client = ApiClient::new("https://api.mmoui.com/v3");

        // create db file if not exists
        let db_file = config_dir.join(EAM_DB);
        if !db_file.exists() {
            File::create(db_file.to_owned()).unwrap();
        }
        // setup database connection and apply migrations if needed
        let database_url = format!("sqlite://{}", db_file.to_string_lossy());
        let db = sea_orm::Database::connect(&database_url).await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        AddonService {
            api: client,
            addons: addon_manager,
            config: config,
            config_filepath: config_filepath,
            db: db,
        }
    }

    pub async fn install(&self, addon_id: i32, update: bool) -> Result<(), Box<dyn Error>> {
        let entry = DbAddon::Entity::find_by_id(addon_id).one(&self.db).await?;
        let mut entry: DbAddon::ActiveModel = entry.unwrap().into();
        let installed_entry = InstalledAddon::Entity::find_by_id(addon_id)
            .one(&self.db)
            .await?;
        let file_details = self
            .api
            .get_file_details(addon_id.try_into().unwrap())
            .await?;

        match installed_entry {
            Some(installed_entry) => {
                if installed_entry.date as u64 == file_details.date && !update {
                    println!(
                        "Addon {} is already installed and up to date",
                        entry.name.unwrap()
                    );
                    return Ok(());
                }
            }
            None => (),
        }

        entry.download = ActiveValue::Set(Some(file_details.download_url.to_owned()));
        entry.version = ActiveValue::Set(file_details.version.to_owned());
        entry.date = ActiveValue::Set(file_details.date.try_into().unwrap());

        let installed = self
            .addons
            .download_addon(&file_details.download_url, &self.api.client)
            .await?;
        let installed_entry = InstalledAddon::ActiveModel {
            addon_id: ActiveValue::Set(addon_id),
            version: ActiveValue::Set(file_details.version),
            date: ActiveValue::Set(file_details.date.try_into().unwrap()),
        };

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
            .await?;

        // get addon IDs from dependency dirs, there may be more than on for each directory
        if installed.depends_on.len() > 0 {
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
                .await?;
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<(), Box<dyn Error>> {
        // update endpoints from api
        self.api.update_endpoints().await?;
        let file_list = self.api.get_file_list().await?;

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
                date: ActiveValue::Set(list_item.date.try_into().unwrap()),
                name: ActiveValue::Set(list_item.name.to_owned()),
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
                    ])
                    .to_owned(),
            )
            .exec(&self.db)
            .await?;
        // delete existing addon directories in case any are removed
        AddonDir::Entity::delete_many()
            .filter(AddonDir::Column::AddonId.is_in(addon_ids))
            .exec(&self.db)
            .await?;
        // Add addon directories for dependency checks
        AddonDir::Entity::insert_many(insert_addon_dirs)
            .exec(&self.db)
            .await?;

        // update all addons that have a newer date than installed date
        // TODO: maybe rewrite this using query builder
        let updates = InstalledAddon::Entity::find()
            .from_raw_sql(Statement::from_string(
                DatabaseBackend::Sqlite,
                r#"SELECT
                i.*
            FROM installed_addon i
            inner join addon a on i.addon_id  = a.id
            where i.date < a.date"#
                    .to_string(),
            ))
            .into_model::<InstalledAddon::Model>()
            .all(&self.db)
            .await?;
        for update in updates.iter() {
            self.install(update.addon_id, true).await?;
        }
        if updates.len() == 0 {
            println!("Everything up to date!");
        }

        let _need_installs = self.get_missing_dependency_options().await;

        self.config.file_details = self.api.file_details_url.to_owned();
        self.config.file_list = self.api.file_list_url.to_owned();
        self.config.list_files = self.api.list_files_url.to_owned();

        config::save_config(&self.config_filepath, &self.config).unwrap();

        Ok(())
    }

    pub fn remove(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    pub async fn get_missing_dependency_options(&self) -> Vec<AddonDepOption> {
        // TODO: maybe rewrite this using query builder
        let need_installs = AddonDep::Entity::find()
            .from_raw_sql(Statement::from_string(
                DatabaseBackend::Sqlite,
                r#"SELECT
            a.id,
            a.name,
            adr.dir
        from addon_dependency ad
        inner join addon_dir adr on ad.dependency_dir = adr.dir
        inner join addon a on adr.addon_id = a.id
        where adr.dir not in (
            select
                DISTINCT dir
            from addon_dir adr
            inner join installed_addon i on adr.addon_id = i.addon_id
        )
        order by adr.dir"#
                    .to_string(),
            ))
            .into_model::<AddonDepOption>()
            .all(&self.db)
            .await
            .map_err(|err| Box::new(err))
            .unwrap();

        if need_installs.len() > 0 {
            println!("Missing dependencies! Founds some options:");
            for need_install in need_installs.iter() {
                println!(
                    "{} - {} ({})",
                    need_install.dir, need_install.name, need_install.id
                );
            }
        }

        need_installs
    }
}
