use eso_addons_core::error::Result;
use eso_addons_core::service::AddonService;

use colored::Colorize;

#[derive(Parser)]
pub struct ShowCommand {
    addon_id: i32,
}

impl ShowCommand {
    pub async fn run(&self, service: &AddonService) -> Result<()> {
        let addon = service.get_addon_details(self.addon_id).await?;
        if addon.is_none() {
            println!("No addon found with id: {}", self.addon_id);
            return Ok(());
        }
        let addon = addon.unwrap();
        print!("{}     : {}", "Name".bold(), addon.name);
        if addon.installed {
            println!(" (installed)")
        } else {
            println!()
        }
        println!("{}       : {}", "ID".bold(), addon.id);
        println!("{} : {}", "Category".bold(), addon.category);
        println!("{}  : {}", "Version".bold(), addon.version);
        Ok(())
    }
}
