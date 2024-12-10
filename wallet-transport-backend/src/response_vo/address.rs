#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDetailsRes {
    /// 地址
    pub address: String,
    /// 是否多签
    pub multi_sig: bool,
    /// 多签受理地址
    pub accept_list: Option<Vec<String>>,
    /// 多签订单信息
    pub order_list: Option<Vec<serde_json::Value>>,
    /// 多签确认地址
    pub confirm_list: Option<Vec<String>>,
    /// 多签因子
    pub multi_sig_elements: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDetailsList {
    pub list: Vec<AddressDetailsRes>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressUid {
    pub address: String,
    pub uid: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressUidList {
    pub list: Vec<AddressUid>,
}
