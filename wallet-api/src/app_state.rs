use once_cell::sync::Lazy;
use tokio::sync::RwLock;

use crate::request::init::{BlockBrowserUrl, UrlParams};

pub static APP_STATE: Lazy<RwLock<AppState>> = Lazy::new(|| {
    // Arc::new()
    RwLock::new(AppState {
        currency: "USD".to_string(),
        language: "CHINESE_SIMPLIFIED".to_string(),
        url: UrlParams::default(),
    })
});

pub struct AppState {
    currency: String,
    language: String,
    url: UrlParams,
}

impl AppState {
    pub fn currency(&self) -> &str {
        &self.currency
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn url(&self) -> &UrlParams {
        &self.url
    }

    pub fn set_fiat_from_str(&mut self, fiat: &str) {
        self.currency = fiat.to_string();
    }

    pub fn set_block_browser_url(&mut self, block_browser_url_list: Vec<BlockBrowserUrl>) {
        self.url.block_browser_url_list = block_browser_url_list;
    }

    pub fn set_mqtt_url(&mut self, mqtt: Option<String>) {
        self.url.mqtt = mqtt
    }

    pub fn set_backend_url(&mut self, backend: Option<String>) {
        self.url.backend = backend
    }

    pub fn set_official_website(&mut self, official_website: Option<String>) {
        self.url.official_website = official_website
    }

    pub fn set_app_download_qr_code_url(&mut self, app_download_qr_code_url: Option<String>) {
        self.url.app_download_qr_code_url = app_download_qr_code_url
    }

    // pub fn set_app_download_url(&mut self, app_download_url: Option<String>) {
    //     self.url.app_download_url = app_download_url
    // }

    pub fn get_official_website(&self) -> Option<String> {
        self.url.official_website.clone()
    }

    pub fn set_language(&mut self, language: &str) {
        self.language = language.to_string();
    }
}
