use chrono::{DateTime, Utc};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCoinInfo {
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
    #[serde(deserialize_with = "de_timestamp_millis")]
    pub create_time: DateTime<Utc>,
    #[serde(deserialize_with = "de_timestamp_millis_opt")]
    pub update_time: Option<DateTime<Utc>>,
}
impl ApiCoinInfo {
    pub fn get_status(&self) -> Option<i32> {
        if self.enable { Some(1) } else { Some(0) }
    }
}
use serde::{Deserialize, Deserializer};

/// 通用时间戳解析器：支持秒级/毫秒 + Option
pub fn de_timestamp_millis_opt<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    use chrono::{TimeZone, Utc};
    // 支持 null -> None
    let opt = Option::<i64>::deserialize(deserializer)?;

    let ts = match opt {
        Some(v) => v,
        None => return Ok(None),
    };

    // 自动判断秒/毫秒
    let (secs, nsecs) = if ts > 1_000_000_000_000 {
        // 毫秒
        let secs = ts / 1000;
        let nsecs = ((ts % 1000) * 1_000_000) as u32;
        (secs, nsecs)
    } else {
        // 秒
        (ts, 0)
    };

    let dt = Utc
        .timestamp_opt(secs, nsecs)
        .single()
        .ok_or_else(|| serde::de::Error::custom("invalid timestamp"))?;

    Ok(Some(dt))
}

pub fn de_timestamp_millis<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    de_timestamp_millis_opt(deserializer)
        .and_then(|opt| opt.ok_or_else(|| serde::de::Error::custom("timestamp is null")))
}
