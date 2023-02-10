#[macro_use]
extern crate clap;
extern crate dirs;
extern crate prettytable;

use crate::show::ShowCommand;
use clap::Parser;
use colored::*;
use eso_addons_core::error::Result;
use eso_addons_core::service::AddonService;

// mod clean;
// mod list;
mod show;

#[derive(Parser)]
#[clap(
    version = crate_version!(),
    author = crate_authors!(),
    about = "CLI tool for managing addons for The Elder Scrolls Online"
)]
struct Opts {
    #[clap(short, long, help = "Path to TOML config file")]
    config: Option<String>,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
struct AddCommand {
    addon_id: i32,
}

impl AddCommand {
    pub async fn run(&self, service: &mut AddonService) -> Result<()> {
        // update endpoints from config
        service.api.file_details_url = service.config.file_details.to_owned();

        let installed = service.install(self.addon_id, false).await;
        match installed {
            Ok(()) => (),
            Err(installed) => return Err(installed),
        };

        // check all addons installed from dependency dirs
        // don't auto-install depends, they are only directory based and there are duplicates,
        // instead, search addon_dirs for possible addons to install
        let _need_installs = service.get_missing_dependency_options().await;

        Ok(())
    }
}

#[derive(Parser)]
struct UpdateCommand {}

impl UpdateCommand {
    pub async fn run(&self, service: &mut AddonService) -> Result<()> {
        service.update().await?;
        Ok(())
    }
}

#[derive(Parser)]
struct RemoveCommand {
    addon_id: i32,
}

impl RemoveCommand {
    pub async fn run(&self, service: &mut AddonService) -> Result<()> {
        service.remove(self.addon_id).await?;
        println!("{} Uninstalled {}!", "✔".green(), self.addon_id);
        Ok(())
    }
}

#[derive(Parser)]
struct SearchCommand {
    search_string: String,
}

impl SearchCommand {
    pub async fn run(&self, service: &AddonService) -> Result<()> {
        let results = service.search(&self.search_string).await?;
        if results.is_empty() {
            println!("No results for \"{}\"", self.search_string);
            return Ok(());
        }
        for addon in results.iter() {
            let mut output = format!("{:>4} {}", addon.id, addon.name);
            if addon.installed {
                output.push_str(&format!(" {}", "(installed)".green().bold()));
            }
            println!("{output}");
        }
        Ok(())
    }
}

#[derive(Parser)]
enum SubCommand {
    // #[clap(about = "List status of addons")]
    // List(list::ListCommand),
    #[clap(about = "Update addons")]
    Update(UpdateCommand),
    // #[clap(about = "Uninstall not managed addons")]
    // Clean(clean::CleanCommand),
    #[clap(about = "Add a new addon")]
    Add(AddCommand),
    #[clap(about = "Uninstall addon")]
    Remove(RemoveCommand),
    #[clap(about = "Search addons")]
    Search(SearchCommand),
    #[clap(about = "Show addon details")]
    Show(ShowCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    let mut service = AddonService::new().await;

    match opts.subcmd {
        // SubCommand::List(list) => list.run(&addon_manager, &config),
        SubCommand::Update(update) => update.run(&mut service).await,
        // SubCommand::Clean(mut clean) => clean.run(&config, &addon_manager),
        SubCommand::Add(add) => add.run(&mut service).await,
        SubCommand::Remove(remove) => remove.run(&mut service).await,
        SubCommand::Search(search) => search.run(&service).await,
        SubCommand::Show(show) => show.run(&service).await,
    }
}
