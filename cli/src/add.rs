use clap::Parser;
use entity::addon as DbAddon;
use entity::installed_addon as InstalledAddon;
use eso_addons_api::ApiClient;
use eso_addons_core::{addons::Manager, config::Config};
use sea_orm::ActiveModelTrait;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};
use std::path::Path;

use super::{Error, Result};

#[derive(Parser)]
pub struct AddCommand {
    addon_id: i32,
}

impl AddCommand {
    pub async fn run(
        &mut self,
        cfg: &mut Config,
        config_filepath: &Path,
        addon_manager: &Manager,
        client: &mut ApiClient,
        db: &DatabaseConnection,
    ) -> Result<()> {
        // update endpoints from config
        client.file_details_url = cfg.file_details.to_owned();

        let entry = DbAddon::Entity::find_by_id(self.addon_id)
            .one(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        let mut entry: DbAddon::ActiveModel = entry.unwrap().into();
        let installed_entry = InstalledAddon::Entity::find_by_id(self.addon_id)
            .one(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        let file_details = client
            .get_file_details(self.addon_id.try_into().unwrap())
            .await?;

        match installed_entry {
            Some(installed_entry) => {
                if installed_entry.date as u64 == file_details.date {
                    println!("Addon {} is already installed", entry.name.unwrap());
                    return Ok(());
                }
            }
            None => (),
        }

        entry.download = ActiveValue::Set(Some(file_details.download_url.to_owned()));
        entry.version = ActiveValue::Set(file_details.version.to_owned());
        entry.date = ActiveValue::Set(file_details.date.try_into().unwrap());

        let installed = addon_manager.download_addon(&file_details.download_url);
        let installed_entry = InstalledAddon::ActiveModel {
            addon_id: ActiveValue::Set(self.addon_id),
            version: ActiveValue::Set(file_details.version),
            date: ActiveValue::Set(file_details.date.try_into().unwrap()),
        };
        installed_entry.insert(db).await;

        // cfg.addons.push(entry.clone());

        // config::save_config(config_filepath, &cfg)?;

        println!("🎊 Installed {}!", entry.name.unwrap());

        Ok(())
    }
}
