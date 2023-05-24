use entity::addon as DbAddon;
use entity::category::Model as Category;
use sea_orm::FromQueryResult;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

pub type AddonMap = HashMap<i32, String>;

#[derive(FromQueryResult, Clone)]
pub struct AddonDepOption {
    pub missing_dir: String,
    pub required_by: String,
    pub option_id: Option<i32>,
    pub option_name: Option<String>,
}

#[derive(Default)]
pub struct MissingDepView {
    pub required_by: String,
    pub options: HashMap<i32, String>,
    pub ignore: bool,
    pub satisfied_by: Option<i32>,
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
    pub id: i32,
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
    pub id: i32,
    pub category_id: String,
    pub version: String,
    pub name: String,
    pub installed: bool,
}

#[derive(FromQueryResult, Clone, Default)]
pub struct AddonShowDetails {
    pub id: i32,
    pub name: String,
    pub author_name: String,
    pub category: String,
    pub version: String,
    pub date: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub download_total: Option<String>,
    pub favorite_total: Option<String>,
    pub file_info_url: String,
    pub download: Option<String>,
    pub file_name: Option<String>,
    pub md5: Option<String>,
    pub description: Option<String>,
    pub change_log: Option<String>,
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
    pub missing_deps: Vec<AddonDepOption>,
    pub missing_details: Vec<i32>,
}
impl Clone for UpdateResult {
    fn clone(&self) -> Self {
        Self {
            addons_updated: self.addons_updated.to_vec(),
            missing_deps: self.missing_deps.to_vec(),
            missing_details: self.missing_details.to_vec(),
        }
    }
}

#[derive(Default, Clone)]
pub struct ParentCategory {
    pub id: i32,
    pub title: String,
    pub child_categories: Vec<Category>,
}
