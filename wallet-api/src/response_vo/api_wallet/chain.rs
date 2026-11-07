use crate::response_vo::standard_wallet::account::BalanceInfo;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiChainAssets {
    pub chain_code: String,
    pub name: String,
    pub symbol: String,
    pub address: String,
    pub token_address: String,
    // pub address_catogary: BtcAddressCategoryOpt,
    pub balance: BalanceInfo,
    /// 0/普通资产 1/多签资产 2/待部署多签账户的普通资产
    pub is_multisig: i8,
    // pub is_multichain: bool,
    pub asset_quantity_ratio: f64,
}
