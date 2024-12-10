#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinInfo {
    pub id: String,
    #[serde(
        rename = "code",
        deserialize_with = "wallet_utils::serde_func::deserialize_uppercase_opt"
    )]
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub chain_code: Option<String>,
    #[serde(rename = "contractAddress")]
    pub token_address: Option<String>,
    pub protocol: Option<String>,
    #[serde(rename = "unit")]
    pub decimals: Option<u8>,
}
impl CoinInfo {
    pub fn token_address(&self) -> Option<String> {
        match &self.token_address {
            Some(token_address) => {
                if token_address.is_empty() {
                    None
                } else {
                    Some(token_address.clone())
                }
            }
            None => None,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinInfos {
    pub total_page: i32,
    pub page_size: i32,
    pub total_count: i32,
    pub list: Vec<CoinInfo>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryByContractAddressRes {
    pub id: String,
    pub chain_code: String,
    pub code: String,
    pub contract_address: Option<String>,
    pub market_value: Option<rust_decimal::Decimal>,
    pub master: bool,
    pub name: Option<String>,
    pub price: Option<rust_decimal::Decimal>,
    pub unit: u8,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenPriceInfos {
    pub list: Vec<wallet_types::valueobject::TokenPriceChangeBody>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCurrency {
    pub chain_code: String,
    pub code: String,
    pub name: String,
    // 币的usdt 单价
    pub price: Option<f64>,
    // 法币d的单价
    pub currency_price: Option<f64>,
    // 汇率
    pub rate: f64,
}
impl TokenCurrency {
    pub fn get_price(&self, symbol: &str) -> Option<f64> {
        if symbol.eq_ignore_ascii_case("usdt") {
            self.price
        } else {
            self.currency_price
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHistoryPrices {
    pub list: Vec<TokenHistoryPrice>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHistoryPrice {
    pub date: String,
    pub price: wallet_types::Decimal,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRate {
    pub rate: f64,
    pub name: String,
    pub target_currency: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenRates {
    pub list: Vec<TokenRate>,
}
