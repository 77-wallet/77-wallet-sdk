use chrono::{DateTime, Utc};

use crate::entities::{api_resource_type::ApiResourceType, api_trade_type::ApiTradeType};

/// 订单资源代理/回收事实。
///
/// 本表只描述服务于归集/提币继续执行的“打能量/回收能量”任务，
/// 也就是 `tradeType=5/6/7/8` 这类绑定原始订单的资源流程。
/// 平台资源质押/解质押（`tradeType=4`）不属于本表，必须走
/// `api_resource_operation` 的独立任务流。
///
/// 共享边界：
/// - 这是 collect / withdraw 共用的一条资源副链
/// - 主流程归属由 `origin_trade_type` 区分
/// - 资源来源由 `source` 区分：
///   - `Platform`：平台代理/回收
///   - `Local`：本地出款地址 fallback 代理
/// - scanner 读取这张表时，必须同时明确“主流程归属”和“资源来源”
///   两层边界，避免不同主链或不同来源互相抢任务。
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
pub enum ApiResourceDelegationSource {
    Platform = 1,
    Local = 2,
}

impl ApiResourceDelegationSource {
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
pub enum ApiResourceDelegationOperationType {
    Delegate = 1,
    Undelegate = 2,
}

impl ApiResourceDelegationOperationType {
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
pub enum ApiResourceDelegationStatus {
    Pending = 1,
    Success = 2,
    Fail = 3,
}

impl ApiResourceDelegationStatus {
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
pub enum ApiResourceDelegationMode {
    WithdrawAddress = 1,
    AuthorizedAddress = 2,
}

impl ApiResourceDelegationMode {
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
pub enum ApiResourceDelegationResultStatus {
    Success = 1,
    Fail = 2,
}

impl ApiResourceDelegationResultStatus {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiResourceDelegationEntity {
    pub id: i64,
    pub uid: String,
    /// 资源任务来源：平台代理还是本地出款地址 fallback
    pub source: ApiResourceDelegationSource,
    pub operation_type: ApiResourceDelegationOperationType,
    pub origin_trade_no: Option<String>,
    /// 原始主流程归属：collect / withdraw
    pub origin_trade_type: Option<i64>,
    pub resource_trade_no: String,
    pub chain_code: String,
    pub owner_address: String,
    pub receiver_address: String,
    /// 资源代理/回收模式：1=平台出款地址自己签；2=授权地址 owner + 本地被授权地址签。
    pub delegation_mode: ApiResourceDelegationMode,
    /// 后端下发的 TRON active permission id，只有授权地址代理/回收时需要。
    pub permission_id: Option<String>,
    pub resource_type: ApiResourceType,
    /// 链上 `delegateResource` 需要的 TRX 数量，来自平台代理任务的
    /// `nativeValue`；本地占位任务没有后端估算值时保持为 `0`。
    pub native_amount: String,
    /// 资源数量，来自平台代理任务的 `rscValue`；用于事实记录和排障展示，
    /// 不能直接作为链上 delegate 的 TRX 数量。
    pub amount: String,
    pub status: ApiResourceDelegationStatus,
    pub task_ack_sent_at: Option<DateTime<Utc>>,
    pub task_ack_send_count: i64,
    pub building_at: Option<DateTime<Utc>>,
    pub raw_tx: Option<String>,
    pub last_broadcast_at: Option<DateTime<Utc>>,
    pub tx_hash: Option<String>,
    pub chain_success_at: Option<DateTime<Utc>>,
    pub chain_failed_at: Option<DateTime<Utc>>,
    pub transaction_time: Option<DateTime<Utc>>,
    pub broadcast_uncertain_since_at: Option<DateTime<Utc>>,
    pub broadcast_uncertain_retry_count: i64,
    pub broadcast_uncertain_last_checked_at: Option<DateTime<Utc>>,
    pub broadcast_uncertain_reconciled_at: Option<DateTime<Utc>>,
    pub broadcast_uncertain_rebroadcast_count: i64,
    pub tx_exec_receipt_uploaded_at: Option<DateTime<Utc>>,
    pub result_status: Option<ApiResourceDelegationResultStatus>,
    /// Backend MQTT final result received timestamp.
    ///
    /// Hard gate: platform resource TX_RES ACK must not be sent before this
    /// fact exists. SDK chain recovery facts must not populate it.
    pub tx_res_received_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result_ack_sent_at: Option<DateTime<Utc>>,
    pub result_ack_send_count: i64,
    pub result_payload: Option<String>,
    pub fail_type: Option<i64>,
    pub err_code: Option<String>,
    pub err_msg: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewApiResourceDelegation {
    pub uid: String,
    pub source: ApiResourceDelegationSource,
    pub operation_type: ApiResourceDelegationOperationType,
    pub origin_trade_no: Option<String>,
    pub origin_trade_type: Option<i64>,
    pub resource_trade_no: String,
    pub chain_code: String,
    pub owner_address: String,
    pub receiver_address: String,
    pub delegation_mode: ApiResourceDelegationMode,
    pub permission_id: Option<String>,
    pub resource_type: ApiResourceType,
    pub native_amount: String,
    pub amount: String,
}

impl NewApiResourceDelegation {
    pub fn platform_delegate(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        origin_trade_no: impl Into<String>,
        origin_trade_type: i64,
        owner_address: impl Into<String>,
        receiver_address: impl Into<String>,
        amount: impl Into<String>,
    ) -> Self {
        Self {
            uid: uid.into(),
            source: ApiResourceDelegationSource::Platform,
            operation_type: ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: Some(origin_trade_no.into()),
            origin_trade_type: Some(origin_trade_type),
            resource_trade_no: resource_trade_no.into(),
            chain_code: "tron".to_string(),
            owner_address: owner_address.into(),
            receiver_address: receiver_address.into(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: "0".to_string(),
            amount: amount.into(),
        }
    }

    pub fn platform_delegate_task(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        origin_trade_type: ApiTradeType,
        operation_type: ApiResourceDelegationOperationType,
        chain_code: impl Into<String>,
        owner_address: impl Into<String>,
        receiver_address: impl Into<String>,
        resource_type: ApiResourceType,
        native_amount: impl Into<String>,
        amount: impl Into<String>,
    ) -> Self {
        Self {
            uid: uid.into(),
            source: ApiResourceDelegationSource::Platform,
            operation_type,
            origin_trade_no: None,
            origin_trade_type: Some(origin_trade_type as i64),
            resource_trade_no: resource_trade_no.into(),
            chain_code: chain_code.into(),
            owner_address: owner_address.into(),
            receiver_address: receiver_address.into(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type,
            native_amount: native_amount.into(),
            amount: amount.into(),
        }
    }

    pub fn local_delegate(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        origin_trade_no: impl Into<String>,
        origin_trade_type: i64,
        owner_address: impl Into<String>,
        receiver_address: impl Into<String>,
        native_amount: impl Into<String>,
        amount: impl Into<String>,
    ) -> Self {
        Self {
            uid: uid.into(),
            source: ApiResourceDelegationSource::Local,
            operation_type: ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: Some(origin_trade_no.into()),
            origin_trade_type: Some(origin_trade_type),
            resource_trade_no: resource_trade_no.into(),
            chain_code: "tron".to_string(),
            owner_address: owner_address.into(),
            receiver_address: receiver_address.into(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: native_amount.into(),
            amount: amount.into(),
        }
    }

    pub fn local_undelegate(
        uid: impl Into<String>,
        resource_trade_no: impl Into<String>,
        origin_trade_no: impl Into<String>,
        origin_trade_type: i64,
        owner_address: impl Into<String>,
        receiver_address: impl Into<String>,
        native_amount: impl Into<String>,
        amount: impl Into<String>,
    ) -> Self {
        Self {
            uid: uid.into(),
            source: ApiResourceDelegationSource::Local,
            operation_type: ApiResourceDelegationOperationType::Undelegate,
            origin_trade_no: Some(origin_trade_no.into()),
            origin_trade_type: Some(origin_trade_type),
            resource_trade_no: resource_trade_no.into(),
            chain_code: "tron".to_string(),
            owner_address: owner_address.into(),
            receiver_address: receiver_address.into(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: native_amount.into(),
            amount: amount.into(),
        }
    }

    pub fn with_delegation_auth(
        mut self,
        delegation_mode: ApiResourceDelegationMode,
        permission_id: Option<String>,
    ) -> Self {
        self.delegation_mode = delegation_mode;
        self.permission_id = permission_id;
        self
    }
}
