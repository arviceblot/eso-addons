use reqwest::Response;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use snafu::ResultExt;
extern crate chrono;

use chrono::prelude::*;
use tracing::info;

use crate::config::Config;
use crate::error::{self, Result};

const GLOBAL_CONFIG: &str = "globalconfig.json";
const GAME_ID: &str = "ESO";

#[derive(Debug, Clone, Default)]
pub struct ApiClient {
    endpoint_url: String,
    pub client: reqwest::Client,
    game_config_url: String,
    pub file_list_url: String,
    pub file_details_url: String,
    pub list_files_url: String,
    pub category_list_url: String,
}

impl ApiClient {
    pub fn new(endpoint_url: &str) -> ApiClient {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .gzip(true)
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_3) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/48.0.2564.116 Safari/537.36")
            .build().unwrap();
        ApiClient {
            endpoint_url: endpoint_url.to_string(),
            client,
            game_config_url: "".to_string(),
            file_list_url: "".to_string(),
            file_details_url: "".to_string(),
            list_files_url: "".to_string(),
            category_list_url: "".to_string(),
        }
    }

    pub async fn update_endpoints(&mut self) -> Result<()> {
        let req_url = format!("{}/{}", self.endpoint_url, GLOBAL_CONFIG);
        let res = self.req_url::<GlobalConfig>(&req_url).await?;
        for game in res.games {
            if game.game_id == GAME_ID {
                self.game_config_url = game.game_config;
                break;
            }
        }
        // update game endpoints
        self.get_game_config().await?;
        Ok(())
    }

    pub fn update_endpoints_from_config(&mut self, config: &Config) {
        self.file_list_url = config.file_list.to_owned();
        self.file_details_url = config.file_details.to_owned();
        self.list_files_url = config.list_files.to_owned();
        self.category_list_url = config.category_list.to_owned();
    }

    pub async fn get_file_list(&mut self) -> Result<Vec<FileListItem>> {
        // Download and parse addon list
        let res = self
            .req_url::<Vec<FileListItem>>(&self.file_list_url)
            .await?;
        Ok(res)
    }

    pub async fn get_file_details(&self, id: i32) -> Result<FileDetails> {
        let req_url = format!("{}{}.json", self.file_details_url, id);
        let res = self.req_url::<Vec<FileDetails>>(&req_url).await.unwrap();
        let res = res.first().cloned().unwrap();
        Ok(res)
    }

    pub async fn download_file(&self, url: &str) -> Result<Response> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context(error::ApiGetUrlSnafu { url })?;
        Ok(response)
    }

    pub async fn get_categories(&self) -> Result<Vec<Category>> {
        let res = self
            .req_url::<Vec<Category>>(&self.category_list_url)
            .await
            .unwrap();
        Ok(res)
    }

    pub async fn get_hm_data(&self, data: String) -> Result<Response> {
        let url = "http://harvestmap.binaryvector.net:8081";
        let res = self
            .client
            .post(url)
            .body(data)
            .send()
            .await
            .context(error::ApiGetUrlSnafu { url })
            .unwrap();
        Ok(res)
    }

    async fn get_game_config(&mut self) -> Result<()> {
        let res = self.req_url::<EsoApiFeeds>(&self.game_config_url).await?;
        self.file_list_url = res.api_feeds.file_list;
        self.file_details_url = res.api_feeds.file_details;
        self.list_files_url = res.api_feeds.list_files;
        self.category_list_url = res.api_feeds.category_list;

        Ok(())
    }

    async fn req_url<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        info!("Requesting: {url}");
        let res = self
            .client
            .get(url)
            .send()
            .await
            .context(error::ApiGetUrlSnafu { url })?
            .json::<T>()
            .await
            .context(error::ApiParseResponseSnafu { url })?;
        Ok(res)
    }
}

#[derive(Deserialize)]
pub struct Category {
    #[serde(rename = "UICATID")]
    pub id: String,
    #[serde(rename = "UICATTitle")]
    pub title: String,
    #[serde(rename = "UICATICON")]
    pub icon: String,
    #[serde(rename = "UICATFileCount")]
    pub file_count: String,
    #[serde(rename = "UICATParentIDs")]
    pub parent_ids: Vec<String>,
}

#[derive(Deserialize, Clone)]
pub struct FileDetails {
    #[serde(rename = "UID")]
    pub id: String,
    #[serde(rename = "UICATID")]
    pub category: String,
    #[serde(rename = "UIVersion")]
    pub version: String,
    #[serde(rename = "UIDate", deserialize_with = "convert_date")]
    pub date: DateTime<Utc>,
    #[serde(rename = "UIMD5")]
    pub md5: String,
    #[serde(rename = "UIFileName")]
    pub file_name: String,
    #[serde(rename = "UIDownload")]
    pub download_url: String,
    #[serde(rename = "UIPending")]
    pub pending: String,
    #[serde(rename = "UIName")]
    pub name: String,
    #[serde(rename = "UIAuthorName")]
    pub author_name: String,
    #[serde(rename = "UIDescription")]
    pub description: String,
    #[serde(rename = "UIChangeLog")]
    pub change_log: String,
    #[serde(rename = "UIHitCount")]
    pub hit_count: String,
    #[serde(rename = "UIHitCountMonthly")]
    pub hit_count_monthly: String,
    #[serde(rename = "UIFavoriteTotal")]
    pub favorite_total: String,
}

#[derive(Deserialize)]
pub struct FileListItem {
    #[serde(rename = "UID")]
    pub id: String,
    #[serde(rename = "UICATID")]
    pub category: String,
    #[serde(rename = "UIVersion")]
    pub version: String,
    #[serde(rename = "UIDate", deserialize_with = "convert_date")]
    pub date: DateTime<Utc>,
    #[serde(rename = "UIName")]
    pub name: String,
    #[serde(rename = "UIAuthorName")]
    pub author_name: String,
    #[serde(rename = "UIFileInfoURL")]
    pub file_info_url: String,
    #[serde(rename = "UIDownloadTotal")]
    pub download_total: String,
    #[serde(rename = "UIDownloadMonthly")]
    pub download_monthly: String,
    #[serde(rename = "UIFavoriteTotal")]
    pub favorite_total: String,
    #[serde(rename = "UIDir")]
    pub directories: Vec<String>,
}

fn convert_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: u64 = Deserialize::deserialize(deserializer)
        .map_err(D::Error::custom)
        .unwrap();
    let timestamp = s / 1000;
    let naive = NaiveDateTime::from_timestamp_opt(timestamp.try_into().unwrap(), 0).unwrap();
    let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);
    Ok(datetime)
}

#[derive(Deserialize)]
pub struct EsoGameConfig {
    #[serde(rename = "FileList")]
    pub file_list: String,
    #[serde(rename = "FileDetails")]
    pub file_details: String,
    #[serde(rename = "ListFiles")]
    pub list_files: String,
    #[serde(rename = "CategoryList")]
    pub category_list: String,
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
