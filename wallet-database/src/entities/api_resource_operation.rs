use chrono::{DateTime, Utc};

use crate::entities::api_resource_type::ApiResourceType;

/// 平台资源质押/解质押任务事实。
///
/// 本表只描述 `tradeType=4` 的独立平台资源管理任务：
/// stake / unstake。它不服务于某个具体归集/提币订单，也不得承载
/// 打能量/回收能量（`tradeType=5/6/7/8`）流程。
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
pub enum ApiResourceOperationTaskSource {
    Backend = 1,
    Client = 2,
}

impl ApiResourceOperationTaskSource {
    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Backend => 1,
            Self::Client => 2,
        }
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
pub enum ApiResourceOperationType {
    Stake = 1,
    Unstake = 2,
    Vote = 3,
    WithdrawReward = 4,
    WithdrawUnfreeze = 5,
    Delegate = 6,
    Undelegate = 7,
    BatchDelegate = 8,
    BatchUndelegate = 9,
}

impl ApiResourceOperationType {
    pub fn as_i64(&self) -> i64 {
        match self {
            Self::Stake => 1,
            Self::Unstake => 2,
            Self::Vote => 3,
            Self::WithdrawReward => 4,
            Self::WithdrawUnfreeze => 5,
            Self::Delegate => 6,
            Self::Undelegate => 7,
            Self::BatchDelegate => 8,
            Self::BatchUndelegate => 9,
        }
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
pub enum ApiResourceOperationStatus {
    Pending = 1,
}

impl ApiResourceOperationStatus {
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
pub enum ApiResourceOperationRecoverStatus {
    RecoverWaiting = 1,
    RetryBuild = 2,
    RetryRecover = 3,
}

impl ApiResourceOperationRecoverStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiResourceOperationEntity {
    pub id: i64,
    pub uid: String,
    pub task_source: ApiResourceOperationTaskSource,
    pub operation_type: ApiResourceOperationType,
    pub resource_trade_no: String,
    pub chain_code: String,
    pub owner_address: String,
    pub receiver_address: Option<String>,
    pub resource_type: ApiResourceType,
    pub amount: String,
    pub status: ApiResourceOperationStatus,
    pub task_ack_sent_at: Option<DateTime<Utc>>,
    pub building_at: Option<DateTime<Utc>>,
    pub raw_tx: Option<String>,
    pub tx_hash: Option<String>,
    pub transaction_fee: Option<String>,
    pub last_broadcast_at: Option<DateTime<Utc>>,
    pub transaction_time: Option<DateTime<Utc>>,
    pub tx_status: Option<String>,
    pub tx_exec_receipt_uploaded_at: Option<DateTime<Utc>>,
    pub result_status: Option<String>,
    pub result_received_at: Option<DateTime<Utc>>,
    pub result_ack_sent_at: Option<DateTime<Utc>>,
    pub result_payload: Option<String>,
    pub fail_type: Option<i64>,
    pub err_code: Option<String>,
    pub err_msg: Option<String>,
    pub recover_status: Option<ApiResourceOperationRecoverStatus>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub retry_count: i64,
    pub broadcast_uncertain_since_at: Option<DateTime<Utc>>,
    pub broadcast_uncertain_retry_count: i64,
    pub broadcast_uncertain_last_checked_at: Option<DateTime<Utc>>,
    pub broadcast_uncertain_reconciled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewApiResourceOperation {
    pub uid: String,
    pub task_source: ApiResourceOperationTaskSource,
    pub operation_type: ApiResourceOperationType,
    pub resource_trade_no: String,
    pub chain_code: String,
    pub owner_address: String,
    pub receiver_address: Option<String>,
    pub resource_type: ApiResourceType,
    pub amount: String,
}

impl NewApiResourceOperation {
    pub fn backend(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        owner_address: impl Into<String>,
        resource_type: ApiResourceType,
        amount: impl Into<String>,
        operation_type: ApiResourceOperationType,
    ) -> Self {
        Self {
            uid: uid.into(),
            task_source: ApiResourceOperationTaskSource::Backend,
            operation_type,
            resource_trade_no: resource_trade_no.into(),
            chain_code: "tron".to_string(),
            owner_address: owner_address.into(),
            receiver_address: None,
            resource_type,
            amount: amount.into(),
        }
    }

    pub fn backend_stake(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        owner_address: impl Into<String>,
        amount: impl Into<String>,
    ) -> Self {
        Self::backend(
            uid,
            resource_trade_no,
            owner_address,
            ApiResourceType::Energy,
            amount,
            ApiResourceOperationType::Stake,
        )
    }

    pub fn client(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        owner_address: impl Into<String>,
        resource_type: ApiResourceType,
        amount: impl Into<String>,
        operation_type: ApiResourceOperationType,
    ) -> Self {
        Self {
            uid: uid.into(),
            task_source: ApiResourceOperationTaskSource::Client,
            operation_type,
            resource_trade_no: resource_trade_no.into(),
            chain_code: "tron".to_string(),
            owner_address: owner_address.into(),
            receiver_address: None,
            resource_type,
            amount: amount.into(),
        }
    }

    pub fn client_vote(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        owner_address: impl Into<String>,
        vote_count: impl Into<String>,
    ) -> Self {
        Self::client(
            uid,
            resource_trade_no,
            owner_address,
            ApiResourceType::Bandwidth,
            vote_count,
            ApiResourceOperationType::Vote,
        )
    }

    pub fn client_withdraw_reward(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        owner_address: impl Into<String>,
        reward_amount: impl Into<String>,
    ) -> Self {
        Self::client(
            uid,
            resource_trade_no,
            owner_address,
            ApiResourceType::Bandwidth,
            reward_amount,
            ApiResourceOperationType::WithdrawReward,
        )
    }
}
