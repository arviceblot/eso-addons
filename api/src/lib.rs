pub mod models;

use std::error::Error;

use models::*;

const GLOBAL_CONFIG: &str = "globalconfig.json";
const GAME_ID: &str = "ESO";

pub struct ApiClient {
    endpoint_url: String,
    pub client: reqwest::Client,
    game_config_url: String,
    pub file_list_url: String,
    pub file_details_url: String,
    pub list_files_url: String,
}

impl ApiClient {
    pub fn new(endpoint_url: &str) -> ApiClient {
        let client = reqwest::Client::builder()
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
        }
    }

    pub async fn update_endpoints(&mut self) -> Result<(), Box<dyn Error>> {
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

    pub async fn get_file_list(&mut self) -> Result<Vec<FileListItem>, Box<dyn Error>> {
        // Download and parse addon list
        let res = self
            .req_url::<Vec<FileListItem>>(&self.file_list_url)
            .await?;
        Ok(res)
    }

    pub async fn get_file_details(&self, id: u16) -> Result<FileDetails, Box<dyn Error>> {
        let req_url = format!("{}{}.json", self.file_details_url, id);
        let res = self.req_url::<Vec<FileDetails>>(&req_url).await.unwrap();
        let res = res.first().cloned().unwrap();
        Ok(res)
    }

    async fn get_game_config(&mut self) -> Result<(), Box<dyn Error>> {
        let res = self.req_url::<EsoApiFeeds>(&self.game_config_url).await?;
        self.file_list_url = res.api_feeds.file_list;
        self.file_details_url = res.api_feeds.file_details;
        self.list_files_url = res.api_feeds.list_files;

        Ok(())
    }

    async fn req_url<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, Box<dyn Error>> {
        println!("Requesting: {}", url);
        let res = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|err| Box::new(err))?
            .json::<T>()
            .await
            .map_err(|err| Box::new(err))?;
        Ok(res)
    }
}
