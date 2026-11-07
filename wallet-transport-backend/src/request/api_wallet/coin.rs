#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenQueryByPageReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default)]
    pub page_num: Option<i32>,
    #[serde(default)]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,
}

impl ApiTokenQueryByPageReq {
    pub fn new(
        create_time: Option<String>,
        update_time: Option<String>,
        page_num: i32,
        page_size: i32,
    ) -> Self {
        Self {
            order_column: None,
            order_type: None,
            chain_code: None,
            code: None,
            page_num: Some(page_num),
            page_size: Some(page_size),
            create_time,
            update_time,
        }
    }
}
