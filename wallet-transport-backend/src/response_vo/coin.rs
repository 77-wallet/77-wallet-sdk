use rust_decimal::Decimal;

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
    #[serde(default, deserialize_with = "wallet_utils::serde_func::deserialize_default_false")]
    pub enable: bool,
    pub price: Option<f64>,
    // 币是否支持兑换
    #[serde(default, deserialize_with = "wallet_utils::serde_func::deserialize_default_false")]
    pub swappable: bool,
    #[serde(default, deserialize_with = "wallet_utils::serde_func::deserialize_default_false")]
    pub default_token: bool,
    #[serde(default, deserialize_with = "wallet_utils::serde_func::deserialize_default_false")]
    pub popular_token: bool,
    pub create_time: String,
    pub update_time: String,
}
impl CoinInfo {
    pub fn get_status(&self) -> Option<i32> {
        if self.enable { Some(1) } else { Some(0) }
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
    pub market_value: Option<f64>,
    pub master: bool,
    pub name: Option<String>,
    pub price: Option<f64>,
    pub unit: u8,
}

// #[derive(Debug, serde::Deserialize, serde::Serialize)]
// #[serde(rename_all = "camelCase")]
// pub struct TokenPriceInfo {
//     #[serde(
//         rename = "code",
//         deserialize_with = "wallet_utils::serde_func::uppercase_opt"
//     )]
//     pub symbol: Option<String>,
//     pub price: Option<rust_decimal::Decimal>,
//     pub chain_code: Option<String>,
//     pub price_percentage: Option<rust_decimal::Decimal>,
// }

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenPriceInfos {
    pub list: Vec<TokenPriceChangeBody>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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
    // 币的小数位
    pub decimals: u8,
}
impl TokenCurrency {
    pub fn new(
        chain_code: &str,
        code: &str,
        name: &str,
        price: Option<f64>,
        currency_price: Option<f64>,
        rate: f64,
        decimals: u8,
    ) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            price,
            currency_price,
            rate,
            decimals,
        }
    }
    pub fn get_price(&self, symbol: &str) -> f64 {
        if symbol.eq_ignore_ascii_case("usdt") {
            self.price.unwrap_or(0.0)
        } else {
            self.currency_price.unwrap_or(0.0)
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

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceChangeBody {
    pub id: Option<String>,
    // 链码
    pub chain_code: String,
    // 代币编码
    #[serde(rename = "code")]
    pub symbol: String,
    // 默认代币
    pub default_token: Option<bool>,
    // 启用状态
    pub enable: bool,
    // 市值
    pub market_value: f64,
    // 主币
    pub master: bool,
    // 代币名称
    pub name: Option<String>,
    // 单价(usdt)
    #[serde(default)]
    pub price: f64,
    // 波动
    pub price_percentage: Option<f64>,
    // 可以状态
    pub status: bool,
    // 代币合约地址
    #[serde(rename = "contractAddress")]
    pub token_address: Option<String>,
    // 24小时交易量
    pub day_change_amount: Option<f64>,
    // 精度
    pub unit: u8,
    // 代币别名
    pub aname: Option<String>,
    // 能否支持兑换
    pub swappable: Option<bool>,
    // 创建时间
    pub create_time: String,
    // 更新时间
    pub update_time: String,
}

impl TokenPriceChangeBody {
    pub fn get_status(&self) -> Option<i32> {
        if self.enable { Some(1) } else { Some(0) }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenPopularByPages {
    pub list: Vec<TokenPriceChangeBody>,
    #[serde(rename = "pageIndex")]
    pub page_index: i64,
    #[serde(rename = "totalPage")]
    pub total_page: i64,
    #[serde(rename = "pageSize")]
    pub page_size: i64,
    #[serde(rename = "totalCount")]
    pub total_count: i64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPrice {
    pub token_address: String,
    pub code: String,
    pub name: String,
    pub unit: u8,
    pub price: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinMarketValue {
    pub market_value: Decimal,
    pub max_supply_quantity: Option<Decimal>,
    pub circulating_supply: Option<Decimal>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinSwappable {
    pub symbol: String,
    pub token_address: String,
    pub chain_code: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CoinSwappableList {
    pub list: Vec<CoinSwappable>,
}
