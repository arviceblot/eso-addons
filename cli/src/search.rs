use clap::Parser;
use colored::*;
use entity::addon as DbAddon;
use entity::installed_addon;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, ModelTrait, QueryFilter};

use super::errors::*;

#[derive(Parser)]
pub struct SearchCommand {
    search_string: String,
}

impl SearchCommand {
    pub async fn run(&mut self, db: &DatabaseConnection) -> Result<()> {
        let addons = DbAddon::Entity::find()
            .filter(DbAddon::Column::Name.like(format!("%{}%", &self.search_string).as_str()))
            .all(db)
            .await
            .map_err(|err| Error::Other(Box::new(err)))?;
        for addon in addons.iter() {
            let installed = addon
                .find_related(installed_addon::Entity)
                .all(db)
                .await
                .map_err(|err| Error::Other(Box::new(err)))?;
            print!("{} {}", addon.id, addon.name);
            if installed.len() > 0 {
                println!(" {}", "(installed)".green().bold());
            } else {
                println!();
            }
        }
        Ok(())
    }
}
