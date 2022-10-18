use std::path::Path;

use super::add::{get_missing_dependency_options, install_addon};
use entity::addon as DbAddon;
use entity::addon_dir as AddonDir;
use entity::installed_addon as InstalledAddon;
use eso_addons_api::ApiClient;
use eso_addons_core::{addons::Manager, config::Config};
use sea_orm::sea_query::OnConflict;
use sea_orm::ColumnTrait;
use sea_orm::DatabaseBackend;
use sea_orm::QueryFilter;
use sea_orm::Statement;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};

use super::config;
use super::errors::*;

#[derive(Parser)]
pub struct UpdateCommand {}

impl UpdateCommand {
    pub async fn run(
        &self,
        config: &mut Config,
        config_filepath: &Path,
        addon_manager: &Manager,
        client: &mut ApiClient,
        db: &DatabaseConnection,
    ) -> Result<()> {
        // update endpoints from api
        client
            .update_endpoints()
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        let file_list = client
            .get_file_list()
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;

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
            .exec(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        // delete existing addon directories in case any are removed
        AddonDir::Entity::delete_many()
            .filter(AddonDir::Column::AddonId.is_in(addon_ids))
            .exec(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        // Add addon directories for dependency checks
        AddonDir::Entity::insert_many(insert_addon_dirs)
            .exec(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;

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
            .all(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        for update in updates.iter() {
            install_addon(update.addon_id, db, client, addon_manager, true).await?;
        }
        if updates.len() == 0 {
            println!("Everything up to date!");
        }

        let _need_installs = get_missing_dependency_options(db).await;

        config.file_details = client.file_details_url.to_owned();
        config.file_list = client.file_list_url.to_owned();
        config.list_files = client.list_files_url.to_owned();

        config::save_config(config_filepath, &config).unwrap();

        Ok(())
    }
}
