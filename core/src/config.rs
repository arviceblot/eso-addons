use crate::error::{self, Result};
use chrono::{DateTime, Utc};
use serde::ser::SerializeStruct;
use serde_derive::{Deserialize, Serialize};
use snafu::ResultExt;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use version_compare::Version;

use tracing::log::info;

pub const EAM_DATA_DIR: &str = "eso-addons";
pub const EAM_CONF: &str = "config.json";
pub const EAM_DB: &str = "addons.db";

const STEAMDECK_DEFAULT_ADDON_DIR: &str = ".local/share/Steam/steamapps/compatdata/306130/pfx/drive_c/users/steamuser/My Documents/Elder Scrolls Online/live/AddOns";

#[cfg(target_os = "linux")]
const DEFAULT_ADDON_DIR: &str = "drive_c/users/user/My Documents/Elder Scrolls Online/live/AddOns";

#[cfg(target_os = "macos")]
const DEFAULT_ADDON_DIR: &str = "drive_c/users/user/My Documents/Elder Scrolls Online/live/AddOns";

#[cfg(target_os = "windows")]
const DEFAULT_ADDON_DIR: &str = "Documents/Elder Scrolls Online/live/AddOns";

// service crate version
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum TTCRegion {
    #[default] // sorry, EU!
    NA,
    EU,
    ALL,
}

/// Delta of TTC download state produced by a PriceTable update.
///
/// The download promise runs on a clone of the service, so it returns this for
/// the caller to merge into the live config (only `Some` fields are applied);
/// persisting from inside the clone would be lost on save.
#[derive(Debug, Clone, Default)]
pub struct TtcConfigUpdate {
    pub na_version: Option<u64>,
    pub eu_version: Option<u64>,
    pub download_last: Option<DateTime<Utc>>,
}

/// Delta of HarvestMap sync state produced by a data update.
///
/// Like [`TtcConfigUpdate`], the update promise runs on a clone of the service,
/// so it returns this for the caller to merge into the live config.
#[derive(Debug, Clone, Default)]
pub struct HmConfigUpdate {
    pub zone_hashes: HashMap<String, String>,
    pub synced_at: Option<DateTime<Utc>>,
}

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum Style {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: String,
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
    /// Last PriceTable version downloaded per region, used to skip redundant
    /// downloads when the server version is unchanged.
    #[serde(default)]
    pub ttc_na_version: Option<u64>,
    #[serde(default)]
    pub ttc_eu_version: Option<u64>,
    #[serde(default)]
    pub ttc_download_last: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub update_on_launch: bool,
    #[serde(default = "default_true")]
    pub onboard: bool,
    #[serde(default)]
    pub update_hm_data: bool,
    /// md5 of the last HarvestMap saved-var data synced per zone, used to skip
    /// the merge when local data is unchanged.
    #[serde(default)]
    pub hm_zone_hashes: HashMap<String, String>,
    /// Time of the last successful merge per zone, used to force a refresh once
    /// a zone's data exceeds the max age even if the local data is unchanged.
    #[serde(default)]
    pub hm_zone_synced: HashMap<String, DateTime<Utc>>,
    #[serde(default)]
    pub hm_last_sync: Option<DateTime<Utc>>,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub ttc_region: TTCRegion,
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
        let mut config: Config = match config_filepath.exists() {
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
                    .truncate(true)
                    .write(true)
                    .open(&config_filepath)
                    .unwrap();
                Config {
                    onboard: true,
                    ..Default::default()
                }
            }
        };

        // check conf version upgrades
        let conf_version = Version::from(&config.version).unwrap();
        if conf_version < Version::from("0.1.2").unwrap() {
            // set auto update true as default when updating conf version, previous default was false
            config.update_on_launch = true;
        }

        // update conf version
        if conf_version < Version::from(VERSION).unwrap() {
            config.version = VERSION.to_string();
        }

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
    pub fn apply_ttc_update(&mut self, update: TtcConfigUpdate) {
        if update.na_version.is_some() {
            self.ttc_na_version = update.na_version;
        }
        if update.eu_version.is_some() {
            self.ttc_eu_version = update.eu_version;
        }
        if update.download_last.is_some() {
            self.ttc_download_last = update.download_last;
        }
    }
    pub fn apply_hm_update(&mut self, update: HmConfigUpdate) {
        for (zone, hash) in update.zone_hashes {
            if let Some(ts) = update.synced_at {
                self.hm_zone_synced.insert(zone.clone(), ts);
            }
            self.hm_zone_hashes.insert(zone, hash);
        }
        if update.synced_at.is_some() {
            self.hm_last_sync = update.synced_at;
        }
    }
    pub fn default_config_dir() -> PathBuf {
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

fn default_version() -> String {
    "0.1.1".to_string()
}

fn default_addon_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(DEFAULT_ADDON_DIR)
}

pub fn detect_addon_dir() -> PathBuf {
    let addon_dir = dirs::home_dir().unwrap();
    for ext_path in [STEAMDECK_DEFAULT_ADDON_DIR, DEFAULT_ADDON_DIR] {
        let path_opt = addon_dir.join(ext_path);
        if path_opt.exists() {
            return path_opt;
        }
    }
    addon_dir
}
