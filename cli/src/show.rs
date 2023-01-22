use eso_addons_core::error::Result;
use eso_addons_core::service::AddonService;

#[derive(Parser)]
pub struct ShowCommand {
    addon_id: i32,
}

impl ShowCommand {
    pub async fn run(&self, service: &AddonService) -> Result<()> {
        println!("Name:");
        println!("ID:");
        println!("Category:");
        println!("Version:");
        println!("Updated:");
        service.remove(self.addon_id).await?;
        Ok(())
    }
}
