#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UrlParams {
    pub mqtt: Option<String>,
    pub backend: Option<String>,
    pub official_website: Option<String>,
    pub block_browser_url_list: Vec<BlockBrowserUrl>,
    pub app_download_qr_code_url: Option<String>,
    // pub app_download_url: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlockBrowserUrl {
    chain_code: String,
    pub address_url: Option<String>,
    pub hash_url: Option<String>,
    pub token_url: Option<String>,
}

impl BlockBrowserUrl {
    pub(crate) fn new(
        chain_code: String,
        address_url: Option<String>,
        hash_url: Option<String>,
        token_url: Option<String>,
    ) -> Self {
        Self { chain_code, address_url, hash_url, token_url }
    }
}
