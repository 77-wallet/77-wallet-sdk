use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedAddressItem {
    /// 下标
    pub index: i32,
    /// 地址绑定状态： 0 未绑定 / 1 已绑定
    pub bind_status: u8,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsListRes(pub Vec<AssetsItem>);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsItem {
    pub index: i32,
    pub address_list: Vec<AddressItem>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressItem {
    pub address: String,
    #[serde(default)]
    pub token_infos: Vec<TokenInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    #[serde(rename = "tokenCode")]
    pub symbol: String,
    pub token_address: String,
    pub amount: f64,
    #[serde(rename = "assets")]
    pub usdt_amount: f64,
}
