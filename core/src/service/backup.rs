use sea_orm::FromQueryResult;
use serde::{Deserialize, Serialize};

#[derive(FromQueryResult, Clone, Default, Debug, Serialize, Deserialize)]
/// Backup type of InstalledAddon without version numbers (so all can be updated on backup restore)
pub struct BackupInstalledAddon {
    pub addon_id: i64,
    pub date: String,
}

#[derive(FromQueryResult, Clone, Default, Debug, Serialize, Deserialize)]
pub struct BackupManualDependency {
    pub addon_dir: String,
    pub satisfied_by: Option<i64>,
    pub ignore: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct BackupData {
    pub installed_addons: Vec<BackupInstalledAddon>,
    pub manual_dependencies: Vec<BackupManualDependency>,
}
