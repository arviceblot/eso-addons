use chrono::{DateTime, Utc};
use entity::addon as DbAddon;
use entity::category::Model as Category;
use sea_orm::FromQueryResult;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Multi-source addon types
// ---------------------------------------------------------------------------

/// Where an installed addon came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddonSource {
    /// Managed via ESOUI.com
    Esoui,
    /// Managed via a Git repository (repo_id, repo_url)
    Git { repo_id: i64, repo_url: String },
    /// Manually installed with no remote source
    Local,
}

/// Unified display struct for installed addons across all sources.
#[derive(Debug, Clone)]
pub struct InstalledAddonDetails {
    /// Source of the addon
    pub source: AddonSource,
    /// Identifier: ESOUI id, git_addon.id, or local_addon.id
    pub id: i64,
    /// Addon display name
    pub name: String,
    /// Author (from manifest or ESOUI)
    pub author: Option<String>,
    /// Remote / latest version
    pub version: String,
    /// Currently installed version (None if unknown)
    pub installed_version: Option<String>,
    /// Whether an update is available
    pub is_upgradable: bool,
    /// Category string ("Addons" or "Libraries")
    pub category: String,
    /// Description from manifest or ESOUI detail
    pub description: Option<String>,
    /// Change log (ESOUI only)
    pub change_log: Option<String>,
    // -- ESOUI-only fields (None for non-ESOUI addons) --
    pub esoui_date: Option<String>,
    pub esoui_download_total: Option<String>,
    pub esoui_download_monthly: Option<String>,
    pub esoui_favorite_total: Option<String>,
    pub esoui_file_info_url: Option<String>,
    pub esoui_game_compat_version: Option<String>,
    pub esoui_game_compat_name: Option<String>,
    pub esoui_category_icon: Option<String>,
}

impl Default for InstalledAddonDetails {
    fn default() -> Self {
        Self {
            source: AddonSource::Esoui,
            id: 0,
            name: String::new(),
            author: None,
            version: String::new(),
            installed_version: None,
            is_upgradable: false,
            category: "Addons".to_string(),
            description: None,
            change_log: None,
            esoui_date: None,
            esoui_download_total: None,
            esoui_download_monthly: None,
            esoui_favorite_total: None,
            esoui_file_info_url: None,
            esoui_game_compat_version: None,
            esoui_game_compat_name: None,
            esoui_category_icon: None,
        }
    }
}

impl InstalledAddonDetails {
    /// Build from an ESOUI `AddonShowDetails` row.
    pub fn from_esoui(details: AddonShowDetails) -> Self {
        let is_upgradable = details.is_upgradable();
        Self {
            source: AddonSource::Esoui,
            id: details.id,
            name: details.name,
            author: if details.author_name.is_empty() {
                None
            } else {
                Some(details.author_name)
            },
            version: details.version,
            installed_version: details.installed_version,
            is_upgradable,
            category: details.category,
            description: details.description,
            change_log: details.change_log,
            esoui_date: Some(details.date),
            esoui_download_total: details.download_total,
            esoui_download_monthly: details.download_monthly,
            esoui_favorite_total: details.favorite_total,
            esoui_file_info_url: if details.file_info_url.is_empty() {
                None
            } else {
                Some(details.file_info_url)
            },
            esoui_game_compat_version: details.game_compat_version,
            esoui_game_compat_name: details.game_compat_name,
            esoui_category_icon: details.category_icon,
        }
    }
}

/// Parsed manifest metadata for a non-ESOUI addon directory.
#[derive(Debug, Clone, Default)]
pub struct LocalAddonResult {
    pub id: i64,
    pub directory: String,
    pub title: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub add_on_version: Option<i64>,
    pub api_version: Option<u32>,
    pub depends_on: Vec<String>,
    pub optional_depends_on: Vec<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    /// ESOUI addon ID if the directory matches an entry in addon_dir.
    /// When present, the user can promote this local addon to managed ESOUI.
    pub esoui_match_id: Option<i64>,
}

/// Parsed manifest metadata for a git-tracked addon.
#[derive(Debug, Clone, Default)]
pub struct GitAddonResult {
    pub id: i64,
    pub repo_id: i64,
    pub repo_url: String,
    pub directory: String,
    pub title: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub add_on_version: Option<i64>,
    pub api_version: Option<u32>,
    pub depends_on: Vec<String>,
    pub optional_depends_on: Vec<String>,
    pub remote_version: Option<String>,
    pub remote_add_on_version: Option<i64>,
    pub description: Option<String>,
    pub is_upgradable: bool,
    pub category: Option<String>,
}

/// Unified manifest-based detail data for local/git addons.
/// Used by the detail view to display non-ESOUI addon information.
#[derive(Debug, Clone)]
pub struct ManifestAddonDetails {
    /// Database ID (local_addon.id or git_addon.id)
    pub id: i64,
    /// Git repo ID (only set for git addons)
    pub repo_id: Option<i64>,
    /// Git repo URL (only set for git addons)
    pub repo_url: Option<String>,
    /// Directory name in AddOns folder
    pub directory: String,
    /// Addon display name (from manifest Title)
    pub title: String,
    /// Author (from manifest Author)
    pub author: Option<String>,
    /// Version string (from manifest Version)
    pub version: Option<String>,
    /// Add-on version integer (from manifest AddOnVersion)
    pub add_on_version: Option<i64>,
    /// API version (from manifest APIVersion)
    pub api_version: Option<u32>,
    /// Dependencies from manifest DependsOn
    pub depends_on: Vec<String>,
    /// Optional dependencies from manifest OptionalDependsOn
    pub optional_depends_on: Vec<String>,
    /// Description (from manifest Description)
    pub description: Option<String>,
    /// Whether an update is available (only for git addons)
    pub is_upgradable: bool,
    /// Category ("Addons" or "Libraries")
    pub category: String,
    /// ESOUI addon ID if the local addon's directory matches an entry in addon_dir.
    /// When present, the user can promote this local addon to managed ESOUI.
    /// Only set for local addons, never for git addons.
    pub esoui_match_id: Option<i64>,
}

impl From<&LocalAddonResult> for ManifestAddonDetails {
    fn from(result: &LocalAddonResult) -> Self {
        Self {
            id: result.id,
            repo_id: None,
            repo_url: None,
            directory: result.directory.clone(),
            title: result.title.clone(),
            author: result.author.clone(),
            version: result.version.clone(),
            add_on_version: result.add_on_version,
            api_version: result.api_version,
            depends_on: result.depends_on.clone(),
            optional_depends_on: result.optional_depends_on.clone(),
            description: result.description.clone(),
            is_upgradable: false,
            category: result
                .category
                .clone()
                .unwrap_or_else(|| "Addons".to_string()),
            esoui_match_id: result.esoui_match_id,
        }
    }
}

impl From<&GitAddonResult> for ManifestAddonDetails {
    fn from(result: &GitAddonResult) -> Self {
        Self {
            id: result.id,
            repo_id: Some(result.repo_id),
            repo_url: Some(result.repo_url.clone()),
            directory: result.directory.clone(),
            title: result.title.clone(),
            author: result.author.clone(),
            version: result.version.clone(),
            add_on_version: result.add_on_version,
            api_version: result.api_version,
            depends_on: result.depends_on.clone(),
            optional_depends_on: result.optional_depends_on.clone(),
            description: result.description.clone(),
            is_upgradable: result.is_upgradable,
            category: result
                .category
                .clone()
                .unwrap_or_else(|| "Addons".to_string()),
            esoui_match_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErrorRecord {
    pub timestamp: DateTime<Utc>,
    pub context: String,
    pub message: String,
}

pub type AddonMap = HashMap<i64, String>;

#[derive(FromQueryResult, Clone)]
pub struct AddonDepOption {
    pub missing_dir: String,
    pub required_by: String,
    pub option_id: Option<i64>,
    pub option_name: Option<String>,
}

#[derive(FromQueryResult, Clone, Default, Debug)]
pub struct AddonRef {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug)]
pub enum Resolution {
    Installed(AddonRef),
    SatisfiedBy(AddonRef),
    Ignored,
    Unresolved { suggestions: Vec<AddonRef> },
}

#[derive(Clone, Debug)]
pub struct DepStatus {
    pub dep_dir: String,
    pub resolution: Resolution,
}

#[derive(Clone, Default, Debug)]
pub struct AddonDependencyView {
    pub forward: Vec<DepStatus>,
    pub dependents: Vec<AddonRef>,
    pub installed_addons: Vec<AddonRef>,
}

#[derive(Default, Clone)]
pub struct MissingDepView {
    pub missing_dir: String,
    pub required_by: String,
    pub options: HashMap<i64, String>,
    pub ignore: bool,
    pub satisfied_by: Option<i64>,
}
impl MissingDepView {
    pub fn new(required_by: String) -> Self {
        Self {
            required_by,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchDbAddon {
    pub id: i64,
    pub category_id: String,
    pub version: String,
    pub name: String,
    pub installed: bool,
}
impl From<&DbAddon::Model> for SearchDbAddon {
    fn from(a: &DbAddon::Model) -> Self {
        Self {
            id: a.id,
            category_id: a.category_id.to_string(),
            version: a.version.to_string(),
            name: a.name.to_string(),
            installed: false,
        }
    }
}

#[derive(FromQueryResult, Default, Deserialize, Clone)]
pub struct AddonDetails {
    pub id: i64,
    pub category_id: String,
    pub version: String,
    pub name: String,
    pub installed: bool,
}

#[derive(FromQueryResult, Clone, Default, Debug)]
pub struct AddonShowDetails {
    pub id: i64,
    pub name: String,
    pub author_name: String,
    pub category: String,
    pub version: String,
    pub date: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub download_total: Option<String>,
    pub download_monthly: Option<String>,
    pub favorite_total: Option<String>,
    pub file_info_url: String,
    pub download: Option<String>,
    pub file_name: Option<String>,
    pub md5: Option<String>,
    pub description: Option<String>,
    pub change_log: Option<String>,
    pub game_compat_version: Option<String>,
    pub game_compat_name: Option<String>,
    pub category_icon: Option<String>,
    // pub dirs: Vec<String>,
}
impl AddonShowDetails {
    pub fn is_upgradable(&self) -> bool {
        if !self.installed {
            return false;
        }
        let default = String::new();
        let inst_vers = self.installed_version.as_ref().unwrap_or(&default);
        *inst_vers != self.version
    }
}

#[derive(Default)]
pub struct UpdateResult {
    pub addons_updated: Vec<AddonDetails>,
}
impl Clone for UpdateResult {
    fn clone(&self) -> Self {
        Self {
            addons_updated: self.addons_updated.to_vec(),
        }
    }
}

#[derive(Default, Clone)]
pub struct ParentCategory {
    pub id: i64,
    pub title: String,
    pub child_categories: Vec<Category>,
}

#[derive(FromQueryResult, Default, Clone)]
pub struct CategoryResult {
    pub id: i64,
    pub title: String,
    pub file_count: Option<i32>,
}

#[derive(FromQueryResult, Default, Clone)]
pub struct AddonImageResult {
    pub index: i64,
    pub thumbnail: String,
    pub image: String,
}

// ---------------------------------------------------------------------------
// Installed addon query results (joined with installation state)
// ---------------------------------------------------------------------------

#[derive(FromQueryResult, Debug, Clone, Default)]
pub struct LocalInstalledAddon {
    pub id: i64,
    pub directory: String,
    pub title: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub add_on_version: Option<i64>,
    pub description: Option<String>,
    pub installed_version: Option<String>,
    pub installed_add_on_version: Option<i64>,
}

#[derive(FromQueryResult, Debug, Clone, Default)]
pub struct GitInstalledAddon {
    pub id: i64,
    pub repo_id: i64,
    pub repo_url: String,
    pub directory: String,
    pub title: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub add_on_version: Option<i64>,
    pub description: Option<String>,
    pub remote_version: Option<String>,
    pub remote_add_on_version: Option<i64>,
    pub installed_version: Option<String>,
    pub installed_add_on_version: Option<i64>,
}
