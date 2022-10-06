use colored::*;
use entity::addon as DbAddon;
use eso_addons_api::ApiClient;
use eso_addons_core::{addons::Manager, config::Config};
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};

use super::errors::*;

#[derive(Parser)]
pub struct UpdateCommand {}

impl UpdateCommand {
    pub async fn run(
        &self,
        config: &Config,
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
        for list_item in file_list.iter() {
            let addon = DbAddon::ActiveModel {
                id: ActiveValue::Set(list_item.id.parse().unwrap()),
                category_id: ActiveValue::Set(list_item.category.to_owned()),
                version: ActiveValue::Set(list_item.version.to_owned()),
                date: ActiveValue::Set(list_item.date),
                name: ActiveValue::Set(list_item.name.to_owned()),
                ..Default::default()
            };

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

        // write to app data

        let desired_addons = &config.addons;

        for addon in desired_addons.iter() {
            let installed = if let Some(ref url) = addon.url {
                let installed = addon_manager.download_addon(&url)?;
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

        Ok(())
    }
}
