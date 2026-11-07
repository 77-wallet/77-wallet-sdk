use chrono::{DateTime, Utc};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    sqlx::Type,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
)]
#[repr(i64)]
pub enum ApiTronAccountActivationStatus {
    Pending = 1,
    Broadcasted = 2,
    Success = 3,
    Fail = 4,
}

impl ApiTronAccountActivationStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    sqlx::Type,
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
)]
#[repr(i64)]
pub enum ApiTronAccountActivationRecoverStatus {
    RecoverWaiting = 1,
    RetryBuild = 2,
    RetryRecover = 3,
}

impl ApiTronAccountActivationRecoverStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiTronAccountActivationEntity {
    pub id: i64,
    pub uid: String,
    pub origin_trade_no: String,
    pub origin_trade_type: i64,
    pub activation_trade_no: String,
    pub chain_code: String,
    pub payer_address: String,
    pub target_address: String,
    pub amount: String,
    pub status: ApiTronAccountActivationStatus,
    pub building_at: Option<DateTime<Utc>>,
    pub raw_tx: Option<String>,
    pub tx_hash: Option<String>,
    pub tx_status: Option<String>,
    pub recover_status: Option<ApiTronAccountActivationRecoverStatus>,
    pub last_broadcast_at: Option<DateTime<Utc>>,
    pub chain_success_at: Option<DateTime<Utc>>,
    pub chain_failed_at: Option<DateTime<Utc>>,
    pub account_verified_at: Option<DateTime<Utc>>,
    pub err_code: Option<String>,
    pub err_msg: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub retry_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewApiTronAccountActivation {
    pub uid: String,
    pub origin_trade_no: String,
    pub origin_trade_type: i64,
    pub activation_trade_no: String,
    pub chain_code: String,
    pub payer_address: String,
    pub target_address: String,
    pub amount: String,
}

impl NewApiTronAccountActivation {
    pub fn collect(
        uid: impl Into<String>,
        origin_trade_no: impl Into<String>,
        activation_trade_no: impl Into<String>,
        payer_address: impl Into<String>,
        target_address: impl Into<String>,
    ) -> Self {
        Self {
            uid: uid.into(),
            origin_trade_no: origin_trade_no.into(),
            origin_trade_type: crate::entities::api_trade_type::ApiTradeType::Collect as i64,
            activation_trade_no: activation_trade_no.into(),
            chain_code: "tron".to_string(),
            payer_address: payer_address.into(),
            target_address: target_address.into(),
            amount: "0.00001".to_string(),
        }
    }
}
