use entity::addon as DbAddon;
use entity::category::Model as Category;
use sea_orm::FromQueryResult;
use serde_derive::{Deserialize, Serialize};

#[derive(FromQueryResult)]
pub struct AddonDepOption {
    pub id: i32,
    pub name: String,
    pub dir: String,
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

#[derive(FromQueryResult)]
pub struct AddonDetails {
    pub id: i32,
    pub category_id: String,
    pub version: String,
    pub name: String,
    pub installed: bool,
}

#[derive(FromQueryResult, Clone)]
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
    pub ttc_updated: bool,
}

#[derive(Default)]
pub struct ParentCategory {
    pub id: i32,
    pub title: String,
    pub child_categories: Vec<Category>,
}
