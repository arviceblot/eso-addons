use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct FileDetails {
    #[serde(rename = "UID")]
    pub id: String,
    #[serde(rename = "UIFileName")]
    pub file_name: String,
    #[serde(rename = "UIDownload")]
    pub download_url: String,
    #[serde(rename = "UIVersion")]
    pub version: String,
    #[serde(rename = "UIDate")]
    pub date: u64,
}

#[derive(Deserialize)]
pub struct FileListItem {
    #[serde(rename = "UID")]
    pub id: String,
    #[serde(rename = "UIVersion")]
    pub version: String,
    #[serde(rename = "UIDate")]
    pub date: u64,
    #[serde(rename = "UIName")]
    pub name: String,
    #[serde(rename = "UICATID")]
    pub category: String,
    #[serde(rename = "UIDir")]
    pub directories: Vec<String>,
}

#[derive(Deserialize)]
pub struct EsoGameConfig {
    #[serde(rename = "FileList")]
    pub file_list: String,
    #[serde(rename = "FileDetails")]
    pub file_details: String,
    #[serde(rename = "ListFiles")]
    pub list_files: String,
}

#[derive(Deserialize)]
pub struct EsoApiFeeds {
    #[serde(rename = "APIFeeds")]
    pub api_feeds: EsoGameConfig,
}

#[derive(Deserialize)]
pub struct GameConfig {
    #[serde(rename = "GameID")]
    pub game_id: String,
    #[serde(rename = "GameConfig")]
    pub game_config: String,
}

#[derive(Deserialize)]
pub struct GlobalConfig {
    #[serde(rename = "GAMES")]
    pub games: Vec<GameConfig>,
}
