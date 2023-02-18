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
        let mut lines = vec![];
        let name_line = if addon.installed {
            format!("{} (installed)", addon.name)
        } else {
            addon.name
        };
        lines.push(("Name", name_line));
        lines.push(("ID", addon.id.to_string()));
        lines.push(("Author", addon.author_name));
        lines.push(("Category", addon.category));
        lines.push(("Version", addon.version));
        if addon.download_total.is_some() {
            lines.push(("Downloads", addon.download_total.unwrap()));
        }
        lines.push(("URL", addon.file_info_url));
        if addon.download.is_some() {
            lines.push(("Download", addon.download.unwrap()));
        }
        if addon.file_name.is_some() {
            lines.push(("File", addon.file_name.unwrap()));
        }
        if addon.md5.is_some() {
            lines.push(("MD5", addon.md5.unwrap()));
        }
        let heading_size = lines.iter().map(|x| x.0.chars().count()).max().unwrap();
        for (heading, data) in lines.iter() {
            println!(
                "{:<width$} : {}",
                heading.bold(),
                data,
                width = heading_size
            );
        }

        Ok(())
    }
}
