use crate::error::{self, Result};
use chrono::{DateTime, Utc};
use serde::ser::SerializeStruct;
use serde_derive::{Deserialize, Serialize};
use snafu::ResultExt;
use std::collections::{HashMap, HashSet};
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

/// Minion backup subdirectory (same across all platforms).
const MINION_BACKUP_SUBDIR: &str = ".minion/minion-backups";

/// Minion backup subdirectory inside a Flatpak sandbox.
const FLATPAK_MINION_BACKUP_SUBDIR: &str = ".var/app/gg.minion.Minion/.minion/minion-backups";

/// ESO game folder names used by Minion.
/// `ESO-1` = live, `ESO-2` = beta/PTS.
const MINION_ESO_GAME_FOLDERS: &[&str] = &["ESO-1", "ESO-2"];

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
    pub pull_git_repositories: bool,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub ttc_region: TTCRegion,
    #[serde(default = "default_1")]
    pub pixels_per_point: f32,
    /// ID of the currently active addon folder.
    /// When `None`, falls back to the folder where `is_default = 1` in the DB.
    #[serde(default)]
    pub active_addon_folder_id: Option<i64>,
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

        // Validate pixels_per_point: must be a positive finite value in a reasonable range.
        // A corrupted config file (e.g. pixels_per_point: 0.0) causes dpi crate assertion
        // failures in egui-winit's update_viewport_info, since scale_factor of 0 or NaN
        // rejects the `validate_scale_factor` check in dpi::Size::Physical::to_logical().
        if config.pixels_per_point <= 0.0
            || config.pixels_per_point > 4.0
            || !config.pixels_per_point.is_finite()
        {
            info!(
                "Invalid pixels_per_point ({:.1}) in config, resetting to default",
                config.pixels_per_point
            );
            config.pixels_per_point = default_1();
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
fn default_1() -> f32 {
    1.0
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

/// Returns Minion backup game directories that exist on disk.
///
/// Checks the native `~/.minion/minion-backups/` location and, on Linux,
/// the Flatpak sandbox equivalent `~/.var/app/gg.minion.Minion/.minion/minion-backups/`.
/// Within each root it looks for `ESO-1` (live) and `ESO-2` (beta/PTS) subdirectories.
///
/// Returns at most one entry per game folder name, preferring the native path over Flatpak.
pub fn detect_minion_backup_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    // Collect candidate backup roots, preferring native over Flatpak.
    let candidates: Vec<PathBuf> = vec![
        home.join(MINION_BACKUP_SUBDIR),
        // Always add the Flatpak path as a candidate; it simply won't exist for
        // non-Flatpak users. This avoids needing runtime sandbox detection.
        home.join(FLATPAK_MINION_BACKUP_SUBDIR),
    ];

    let mut found = Vec::new();
    let mut seen_game_folders: HashSet<String> = HashSet::new();

    for root in candidates {
        if !root.is_dir() {
            continue;
        }
        for game_folder in MINION_ESO_GAME_FOLDERS {
            let key = game_folder.to_lowercase();
            if !seen_game_folders.insert(key) {
                continue;
            }
            let game_path = root.join(game_folder);
            if game_path.is_dir() {
                found.push(game_path);
            }
        }
    }

    found
}
