use std::path::Path;

use colored::*;
use entity::addon as DbAddon;
use entity::addon_dir as AddonDir;
use eso_addons_api::ApiClient;
use eso_addons_core::config;
use eso_addons_core::{addons::Manager, config::Config};
use sea_orm::sea_query::OnConflict;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};

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

        // write to app data

        let desired_addons = &config.addons;

        for addon in desired_addons.iter() {
            let installed = if let Some(ref url) = addon.url {
                let installed = match addon_manager.download_addon(&url) {
                    Ok(installed) => installed,
                    Err(e) => {
                        println!("{} Failed {}!", "☒".red(), addon.name);
                        println!("{}", e.to_string());
                        continue;
                    }
                };
                Some(installed)
            } else {
                addon_manager.get_addon(&addon.name)?
            };

            if let Some(installed) = installed {
                if installed.name == addon.name {
                    println!("{} Updated {}!", "✔".green(), addon.name);
                } else {
                    println!(
                        // TODO: change the name in the config automatically
                        "⚠ Installed {}, but is called {} is config file. Verify the addon name in the config file.",
                        installed.name, addon.name
                    );
                }
            } else {
                println!(
                    "⚠ {} is set to be manually installed, but not present",
                    addon.name
                )
            }
        }

        let installed_addons_list = addon_manager.get_addons()?;
        let missing_addons: Vec<String> =
            eso_addons_core::get_missing_dependencies(&installed_addons_list.addons).collect();

        if missing_addons.len() > 0 {
            println!(
                "\n{} There are missing dependencies! Please install the following addons to resolve the dependencies:",
                "⚠".red()
            );

            for missing in eso_addons_core::get_missing_dependencies(&installed_addons_list.addons)
            {
                println!("- {}", missing);
            }
        }

        let unused_addons =
            eso_addons_core::get_unused_dependencies(&installed_addons_list.addons, desired_addons);

        if unused_addons.len() > 0 {
            println!("\nThere are unused dependencies:");

            for unused in unused_addons {
                println!("- {}", unused);
            }
        }

        config.file_details = client.file_details_url.to_owned();
        config.file_list = client.file_list_url.to_owned();
        config.list_files = client.list_files_url.to_owned();

        config::save_config(config_filepath, &config);

        Ok(())
    }
}
