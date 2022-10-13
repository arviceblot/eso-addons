use clap::Parser;
use colored::*;
use entity::addon as DbAddon;
use entity::addon_dependency as AddonDep;
use entity::installed_addon as InstalledAddon;
use eso_addons_api::ApiClient;
use eso_addons_core::{addons::Manager, config::Config};
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveModelTrait;
use sea_orm::DatabaseBackend;
use sea_orm::FromQueryResult;
use sea_orm::Statement;
use sea_orm::{ActiveValue, DatabaseConnection, EntityTrait};

use super::{Error, Result};

#[derive(Parser)]
pub struct AddCommand {
    addon_id: i32,
}

impl AddCommand {
    pub async fn run(
        &self,
        cfg: &mut Config,
        addon_manager: &Manager,
        client: &mut ApiClient,
        db: &DatabaseConnection,
    ) -> Result<()> {
        // update endpoints from config
        client.file_details_url = cfg.file_details.to_owned();

        let installed = install_addon(self.addon_id, db, client, addon_manager, false).await;
        match installed {
            Ok(()) => (),
            Err(installed) => return Err(installed),
        };

        // check all addons installed from dependency dirs
        // don't auto-install depends, they are only directory based and there are duplicates,
        // instead, search addon_dirs for possible addons to install
        let _need_installs = get_missing_dependency_options(db).await;

        Ok(())
    }
}

pub async fn install_addon(
    addon_id: i32,
    db: &DatabaseConnection,
    client: &ApiClient,
    addon_manager: &Manager,
    update: bool,
) -> Result<()> {
    let entry = DbAddon::Entity::find_by_id(addon_id)
        .one(db)
        .await
        .map_err(|err| Error::Other(Box::new(err)))?;
    let mut entry: DbAddon::ActiveModel = entry.unwrap().into();
    let installed_entry = InstalledAddon::Entity::find_by_id(addon_id)
        .one(db)
        .await
        .map_err(|err| Error::Other(Box::new(err)))?;
    let file_details = client
        .get_file_details(addon_id.try_into().unwrap())
        .await?;

    match installed_entry {
        Some(installed_entry) => {
            if installed_entry.date as u64 == file_details.date {
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

    let installed = addon_manager
        .download_addon(&file_details.download_url, &client.client)
        .await?;
    let installed_entry = InstalledAddon::ActiveModel {
        addon_id: ActiveValue::Set(addon_id),
        version: ActiveValue::Set(file_details.version),
        date: ActiveValue::Set(file_details.date.try_into().unwrap()),
    };

    match InstalledAddon::Entity::insert(installed_entry)
        .on_conflict(
            OnConflict::column(InstalledAddon::Column::AddonId)
                .update_columns([
                    InstalledAddon::Column::Date,
                    InstalledAddon::Column::Version,
                ])
                .to_owned(),
        )
        .exec(db)
        .await
    {
        Ok(_) => {
            if !update {
                println!("🎊 Installed {}!", entry.name.unwrap());
            } else {
                println!("{} Updated {}!", "✔".green(), entry.name.unwrap());
            }
        }
        Err(error) => return Err(Error::Other(Box::new(error))),
    }

    // get addon IDs from dependency dirs, there may be more than on for each directory
    if installed.depends_on.len() > 0 {
        let deps = installed.depends_on.iter().map(|x| AddonDep::ActiveModel {
            addon_id: ActiveValue::Set(addon_id),
            dependency_dir: ActiveValue::Set(x.to_owned()),
        });
        // insert all dependencies
        AddonDep::Entity::insert_many(deps)
            .on_conflict(
                OnConflict::columns([AddonDep::Column::AddonId, AddonDep::Column::DependencyDir])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
    }
    Ok(())
}

async fn get_missing_dependency_options(db: &DatabaseConnection) -> Vec<AddonDepOption> {
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
        .all(db)
        .await
        .map_err(|err| Error::Other(Box::new(err)))
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

#[derive(FromQueryResult)]
struct AddonDepOption {
    id: i32,
    name: String,
    dir: String,
}
