// #[derive(Debug, PartialEq, Eq, Hash)]
// pub struct ExchangeRateId {
//     pub currency: String,
//     pub chain_code: String,
//     pub symbol: String,
// }

// impl ExchangeRateId {
//     pub fn new(currency: &str, chain_code: &str, symbol: &str) -> Self {
//         Self {
//             currency: currency.to_string(),
//             chain_code: chain_code.to_string(),
//             symbol: symbol.to_string(),
//         }
//     }
// }

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow, wallet_macro::macros::Resource)]
#[resource(
    // schema_name = "wallet",
    query_req = "crate::entities::exchange_rate::QueryReq",
    sqlite_table_name = "exchange_rate",
    // primary_key = "target_currency: String",
    // constraint = "exchange_rate_symbol_currency_chain_code_idx"
)]
pub struct ExchangeRateEntity {
    pub name: String,
    pub rate: f64,
    #[resource(detail = "QueryReq")]
    pub target_currency: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

pub struct QueryReq {
    pub target_currency: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ExchangeRate {
    pub name: String,
    pub price: f64,
}
