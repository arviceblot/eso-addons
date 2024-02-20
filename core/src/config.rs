use crate::error::{self, Result};
use serde::ser::SerializeStruct;
use serde_derive::{Deserialize, Serialize};
use snafu::ResultExt;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use tracing::log::info;

pub const EAM_DATA_DIR: &str = "eso-addons";
pub const EAM_CONF: &str = "config.json";
pub const EAM_DB: &str = "addons.db";

const STEAMDECK_DEFAULT_ADDON_DIR: &str = ".local/share/Steam/steamapps/compatdata/306130/pfx/drive_c/users/steamuser/My Documents/Elder Scrolls Online/live/AddOns";
//const STEAMDECK_DEFAULT_CONFIG_DIR: &str = "/home/deck/.config";
const WINDOWS_DEFAULT_ADDON_DIR: &str = "Documents/Elder Scrolls Online/live/AddOns";
const LINUX_DEFAULT_ADDON_DIR: &str =
    "drive_c/users/user/My Documents/Elder Scrolls Online/live/AddOns";

#[derive(Deserialize, Debug, Clone)]
pub struct AddonEntry {
    pub name: String,
    pub url: Option<String>,
    #[serde(default)]
    pub dependency: bool,
}
impl serde::Serialize for AddonEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("AddonEntry", 0)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("url", &self.url)?;
        state.serialize_field("dependency", &self.dependency)?;

        state.end()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Style {
    Light,
    Dark,
    System,
}
impl Default for Style {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Config {
    #[serde(default = "default_addon_dir")]
    pub addon_dir: PathBuf,
    #[serde(default = "default_str")]
    pub file_list: String,
    #[serde(default = "default_str")]
    pub file_details: String,
    #[serde(default = "default_str")]
    pub list_files: String,
    #[serde(default = "default_str")]
    pub category_list: String,
    #[serde(default)]
    pub update_ttc_pricetable: bool,
    #[serde(default)]
    pub update_on_launch: bool,
    #[serde(default = "default_true")]
    pub onboard: bool,
    #[serde(default)]
    pub update_hm_data: bool,
    #[serde(default)]
    pub style: Style,
}
impl Config {
    pub fn load() -> Config {
        // check config dir exists
        let config_dir = Self::default_config_dir();
        if !config_dir.exists() {
            info!("Creating config directory: {}", config_dir.display());
            fs::create_dir_all(&config_dir).unwrap();
        }
        let config_filepath = Self::default_config_path();
        // create config file if not exists, with defaults
        let config: Config = match config_filepath.exists() {
            true => {
                let config_data = fs::read_to_string(&config_filepath)
                    .context(error::ConfigLoadSnafu {
                        path: &config_filepath,
                    })
                    .unwrap();
                if config_data.is_empty() {
                    // load defaults
                    info!(
                        "Empty config data, loading defaults to: {}",
                        config_filepath.display()
                    );
                    Config {
                        onboard: true,
                        ..Default::default()
                    }
                } else {
                    info!("Loading config data at: {}", config_filepath.display());
                    serde_json::from_str(&config_data)
                        .context(error::ConfigParseSnafu {
                            path: &config_filepath,
                        })
                        .unwrap()
                }
            }
            false => {
                info!("No config file, creating at: {}", config_filepath.display());
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&config_filepath)
                    .unwrap();
                Config {
                    onboard: true,
                    ..Default::default()
                }
            }
        };

        // write defaults for immediate use
        config.save().unwrap();
        config
    }
    pub fn save(&self) -> Result<()> {
        let path = Self::default_config_path();
        let config_str = serde_json::to_string_pretty(self)
            .context(error::ConfigWriteFormatSnafu { path: &path })?;
        fs::write(&path, config_str).context(error::ConfigWriteSnafu { path: &path })?;
        Ok(())
    }
    fn default_config_dir() -> PathBuf {
        dirs::config_dir().unwrap().join(EAM_DATA_DIR)
    }
    fn default_config_path() -> PathBuf {
        Self::default_config_dir().join(EAM_CONF)
    }
    pub fn default_db_path() -> PathBuf {
        Self::default_config_dir().join(EAM_DB)
    }
}

fn default_str() -> String {
    "".to_string()
}
fn default_true() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn default_addon_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(LINUX_DEFAULT_ADDON_DIR)
}

#[cfg(target_os = "windows")]
fn default_addon_dir() -> PathBuf {
    let addon_dir = dirs::home_dir().unwrap();
    addon_dir.join("Documents/Elder Scrolls Online/live/AddOns")
}

#[cfg(target_os = "macos")]
fn default_addon_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(LINUX_DEFAULT_ADDON_DIR)
}

pub fn detect_addon_dir() -> PathBuf {
    let addon_dir = dirs::home_dir().unwrap();
    for ext_path in [STEAMDECK_DEFAULT_ADDON_DIR, WINDOWS_DEFAULT_ADDON_DIR] {
        let path_opt = addon_dir.join(ext_path);
        if path_opt.exists() {
            return path_opt;
        }
    }
    addon_dir
}
