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
        println!("{} :", "Name".bold());
        println!("{} :", "ID".bold());
        println!("{} :", "Category".bold());
        println!("{} :", "Version".bold());
        println!("{} :", "Installed".bold());
        Ok(())
    }
}
