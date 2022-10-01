use serde::Deserialize;

#[derive(Deserialize)]
pub struct FileDetails {
    #[serde(rename = "UID")]
    id: u16,
    #[serde(rename = "UIFileName")]
    file_name: String,
    #[serde(rename = "UIDownload")]
    download_url: String,
}

#[derive(Deserialize)]
pub struct FileListItem {
    #[serde(rename = "UID")]
    id: u16,
    #[serde(rename = "UIVersion")]
    version: String,
    #[serde(rename = "UIDate")]
    date: String,
    #[serde(rename = "UIName")]
    name: String,
    // UIDir (List)
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
