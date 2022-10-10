#[macro_use]
extern crate clap;
extern crate dirs;
#[macro_use]
extern crate prettytable;

use clap::Parser;
use eso_addons_api::ApiClient;
use eso_addons_core::addons;
use eso_addons_core::config;
use eso_addons_core::config::{EAM_CONF, EAM_DATA_DIR, EAM_DB};
use migration::{Migrator, MigratorTrait};
use std::fs::File;
use std::path::PathBuf;

mod add;
mod clean;
mod errors;
mod list;
mod remove;
mod search;
mod update;

use errors::{Error, Result};

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
enum SubCommand {
    #[clap(about = "List status of addons")]
    List(list::ListCommand),
    #[clap(about = "Update addons")]
    Update(update::UpdateCommand),
    #[clap(about = "Uninstall not managed addons")]
    Clean(clean::CleanCommand),
    #[clap(about = "Add a new addon")]
    Add(add::AddCommand),
    #[clap(about = "Uninstall addon")]
    Remove(remove::RemoveCommand),
    #[clap(about = "Search addons")]
    Search(search::SearchCommand),
}

pub async fn run() -> Result<()> {
    let opts: Opts = Opts::parse();

    let config_dir = dirs::config_dir().unwrap();

    let default_config_filepath = config_dir.join(EAM_DATA_DIR).join(EAM_CONF);
    let config_filepath = opts
        .config
        .map(|x| PathBuf::from(&x))
        .unwrap_or(default_config_filepath);

    let mut config = config::parse_config(&config_filepath)?;

    let addon_manager = addons::Manager::new(&config.addon_dir);

    let mut client = ApiClient::new("https://api.mmoui.com/v3");

    // create db file if not exists
    let db_file = config_dir.join(EAM_DATA_DIR).join(EAM_DB);
    if !db_file.exists() {
        File::create(db_file.to_owned()).unwrap();
    }
    // setup database connection and apply migrations if needed
    let database_url = format!("sqlite://{}", db_file.to_string_lossy());
    let db = sea_orm::Database::connect(&database_url).await.unwrap();
    Migrator::up(&db, None).await.unwrap();

    match opts.subcmd {
        SubCommand::List(list) => list.run(&addon_manager, &config),
        SubCommand::Update(update) => {
            update
                .run(
                    &mut config,
                    &config_filepath,
                    &addon_manager,
                    &mut client,
                    &db,
                )
                .await
        }
        SubCommand::Clean(mut clean) => clean
            .run(&config, &addon_manager)
            .map_err(|err| Error::Other(err)),
        SubCommand::Add(add) => add.run(&mut config, &addon_manager, &mut client, &db).await,
        SubCommand::Remove(remove) => remove.run(&mut config, &config_filepath, &addon_manager),
        SubCommand::Search(mut search) => search.run(&db).await,
    }
}
