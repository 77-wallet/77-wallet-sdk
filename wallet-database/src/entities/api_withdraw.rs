use crate::{
    Error,
    entities::{
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
        asset_token_key::AssetTokenKey,
    },
};
use serde::Deserializer;
use std::fmt::Display;

/// Signed bytes are deliberately excluded from Debug/log output.
pub struct SignedSelfTransfer<'a> {
    pub trade_no: &'a str,
    pub uid: &'a str,
    pub name: &'a str,
    pub from_addr: &'a str,
    pub to_addr: &'a str,
    pub chain_code: &'a str,
    pub token_addr: AssetTokenKey,
    pub symbol: &'a str,
    pub value: &'a str,
    pub nonce: i64,
    pub tx_hash: &'a str,
    pub raw_tx: &'a str,
    pub transaction_fee: &'a str,
    pub notes: Option<&'a str>,
}

/// Receipt already observed for the exact signed ordinary EVM transaction.
/// A missing/uncertain RPC result must never be converted into this fact.
pub struct SelfTransferReceipt<'a> {
    pub trade_no: &'a str,
    pub chain_code: &'a str,
    pub tx_hash: &'a str,
    pub success: bool,
    pub transaction_time: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub block_height: &'a str,
    pub transaction_fee: &'a str,
    pub resource_consume: &'a str,
}

#[derive(Debug)]
pub struct WithdrawCreatedFact {
    pub uid: Option<String>,
    pub name: String,
    pub from_addr: String,
    pub to_addr: String,
    pub symbol: String,
    pub value: String,
    pub validate: String,
    pub chain_code: String,
    pub token_addr: AssetTokenKey,
    pub trade_no: String,
    pub trade_type: i64,
    pub status: ApiWithdrawStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingWithdrawResourceBudget {
    pub energy: u64,
    pub bandwidth: u64,
}

/// NOTE:
/// ErrCode MUST NOT implement Deserialize directly.
/// All deserialization must go through this function
/// to preserve fact semantics (0/null/invalid => None).
fn deserialize_opt_err_code<'de, D>(deserializer: D) -> Result<Option<ErrCode>, D::Error>
where
    D: Deserializer<'de>,
{
    // 先尝试解析为 Option<u32>
    let opt_u32: Option<u32> = serde::Deserialize::deserialize(deserializer)?;

    // 处理解析结果
    let code = match opt_u32 {
        // 如果是 None 或 Some(0)，返回 None
        None | Some(0) => None,
        // 如果是其他值，根据值返回对应的 ErrCode
        Some(6001) => Some(ErrCode::BalanceInsufficient),
        Some(6002) => Some(ErrCode::FeeInsufficient),
        Some(6003) => Some(ErrCode::AddressFormatIncorrect),
        Some(6004) => Some(ErrCode::NodeError),
        Some(6005) => Some(ErrCode::NetworkException),
        Some(6006) => Some(ErrCode::TransactionOnChainException),
        Some(6008) => Some(ErrCode::SDKInternalError),
        Some(6099) => Some(ErrCode::UnknownError),
        // 其他无效值也返回 None
        _ => None,
    };

    Ok(code)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApiWithdrawEntity {
    #[serde(skip_serializing)]
    pub id: i64,
    pub name: String,
    pub uid: String,
    pub from_addr: String,
    pub to_addr: String,
    pub value: String,
    pub validate: String,
    pub chain_code: String,
    pub token_addr: AssetTokenKey,
    pub symbol: String,
    pub trade_no: String,
    pub out_order_id: Option<String>,
    pub client_id: Option<String>,
    pub create_time: Option<String>,
    pub trade_type: ApiTradeType,
    pub init_status: ApiWithdrawStatus,
    pub status: ApiWithdrawStatus,
    pub nonce: i64,
    pub tx_hash: Option<String>,
    #[serde(skip_serializing)]
    pub raw_tx: Option<String>,
    #[serde(skip_serializing)]
    pub resource_consume: String,
    pub transaction_fee: String,
    pub estimated_transaction_fee: Option<String>,
    pub estimated_resource_consume: Option<String>,
    pub fee_estimated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    pub transaction_time: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    pub block_height: Option<String>,
    pub notes: Option<String>,
    pub post_tx_count: u32,
    pub post_confirm_tx_count: u32,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_err_code"
    )]
    pub err_code: Option<ErrCode>,
    pub err_msg: Option<String>,

    // ===== TRON Resource Gate Facts =====
    /// 最近一次资源门禁检查时间（仅作为调度事实，不代表门禁已通过）
    #[serde(skip_serializing)]
    pub resource_check_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// 门禁放行时间（有值表示当前订单已不再阻塞于资源门禁）
    #[serde(skip_serializing)]
    pub resource_gate_released_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// 最近一次门禁结论（内部数字枚举）
    #[serde(skip_serializing)]
    pub resource_gate_result: Option<ApiResourceGateResult>,
    /// 最近一次门禁阻塞原因（内部数字枚举，仅 blocked 时有意义）
    #[serde(skip_serializing)]
    pub resource_block_reason: Option<ApiResourceBlockReason>,
    /// 资源依赖任务号（平台代打能量等），用于关联资源子流程事实
    #[serde(skip_serializing)]
    pub resource_dependency_trade_no: Option<String>,
    /// 资源依赖类型（内部数字枚举），用于区分依赖来源
    #[serde(skip_serializing)]
    pub resource_dependency_type: Option<ApiResourceDependencyType>,

    // ===== Tx ACK（交易 ACK 事实）=====
    #[serde(skip_serializing)]
    pub tx_ack_sent_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 确认已接收并持久化该交易

    // ===== Build / Broadcast Execution Facts =====
    #[serde(skip_serializing)]
    pub building_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // BuildTx 执行占位
    #[serde(skip_serializing)]
    pub last_broadcast_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 最近一次 Broadcast 执行占位
    #[serde(skip_serializing)]
    pub broadcast_uncertain_since_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // EVM 广播/恢复不确定态开始时间
    #[serde(skip_serializing)]
    pub broadcast_uncertain_retry_count: u32, // EVM 广播/恢复不确定态重试计数
    #[serde(skip_serializing)]
    pub broadcast_uncertain_last_checked_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // EVM 广播/恢复不确定态最近检查时间
    #[serde(skip_serializing)]
    pub broadcast_uncertain_reconciled_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // EVM 不确定态超时后 nonce reconcile 执行时间
    #[serde(skip_serializing)]
    pub broadcast_uncertain_rebroadcast_count: u32, // EVM 不确定态超时后的自动重播次数

    // ===== Tx Result ACK（结果确认事实）=====
    #[serde(skip_serializing)]
    pub tx_res_ack_sent_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 确认已将交易结果可靠告知后端
    /// SER TxRes push received timestamp (AWM_ORDER_TRANS_RES)
    /// - Hard gate: TX_RES ack MUST NOT be sent before this fact exists.
    #[serde(skip_serializing)]
    pub tx_res_received_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,

    // ===== Tx Exec Receipt Upload（交易执行回执上传事实）=====
    #[serde(skip_serializing)]
    pub tx_exec_receipt_uploaded_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 已上传交易执行回执

    // ===== Terminal Fact =====
    #[serde(skip_serializing)]
    pub finished_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 链上终态事实

    // ===== Audit 事实 =====
    pub audit_passed_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 审核通过事实
    pub audit_rejected_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 审核拒绝事实
    #[serde(skip_serializing)]
    pub audit_reason: Option<String>,                         // 审核拒绝原因

    // ===== Chain Result 事实 =====
    #[serde(skip_serializing)]
    pub chain_success_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 链上成功事实
    #[serde(skip_serializing)]
    pub chain_failed_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 链上失败事实

    // ===== Failure Stage 事实 =====
    #[serde(skip_serializing)]
    pub failure_stage: Option<WithdrawFailureStage>, // 失败阶段

    // ===== Meta =====
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(
    sqlx::Type,
    Debug,
    Clone,
    Copy,
    serde_repr::Deserialize_repr,
    serde_repr::Serialize_repr,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
#[repr(i8)]
pub enum ApiWithdrawStatus {
    InitOrder = -1,            // -1
    Init = 0,                  // 0
    AuditPass = 1,             // 1
    AuditReject = 2,           // 2
    SendingTx = 3,             // 3
    SendingTxFailed = 4,       // 4
    SendingTxReport = 5,       // 5, 发送交易报告给服务器
    SendingTxFailedReport = 6, // 6,发送交易失败报告给服务器，结束
    Success = 7,               // 7，收到成功确认
    Failure = 8,               // 8，收到失败确认
    ConfirmSuccessReport = 9,  // 9, 结束
    ConfirmFailureReport = 10, // 10, 结束
}

impl Display for ApiWithdrawStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl TryFrom<u8> for ApiWithdrawStatus {
    type Error = crate::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ApiWithdrawStatus::Init),
            1 => Ok(ApiWithdrawStatus::AuditPass),
            2 => Ok(ApiWithdrawStatus::AuditReject),
            3 => Ok(ApiWithdrawStatus::SendingTx),
            4 => Ok(ApiWithdrawStatus::SendingTxFailed),
            5 => Ok(ApiWithdrawStatus::SendingTxReport),
            6 => Ok(ApiWithdrawStatus::SendingTxFailedReport),
            7 => Ok(ApiWithdrawStatus::Success),
            8 => Ok(ApiWithdrawStatus::Failure),
            9 => Ok(ApiWithdrawStatus::ConfirmSuccessReport),
            10 => Ok(ApiWithdrawStatus::ConfirmFailureReport),
            _ => Err(Error::InvalidValue(value)),
        }
    }
}

// ERR_6001(6001,"余额不足"),
//     ERR_6002(6002,"手续费不足"),
//     ERR_6003(6003,"地址格式不正确"),
//     ERR_6004(6004,"节点错误"),
//     ERR_6005(6005,"网络异常"),
//     ERR_6006(6006,"交易上链异常，人工确认"),
//     ERR_6008(6007,"SDK内部错误"),
//     ERR_6099(6099,"未知错误"),
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, sqlx::Type, serde::Serialize)]
#[repr(u32)]
pub enum ErrCode {
    BalanceInsufficient = 6001,
    FeeInsufficient = 6002,
    AddressFormatIncorrect = 6003,
    NodeError = 6004,
    NetworkException = 6005,
    TransactionOnChainException = 6006,
    SDKInternalError = 6008,
    UnknownError = 6099,
}

impl std::fmt::Display for ErrCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let err_str = format!("ERR_{}", *self as u32);
        write!(f, "{}", err_str)
    }
}

impl<'de> serde::Deserialize<'de> for ErrCode {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Err(serde::de::Error::custom(
            "ErrCode must not be deserialized directly; use deserialize_opt_err_code",
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OptErrCode(pub Option<ErrCode>);

// 实现 TryFrom<u32>，将 0 转换为 None
impl TryFrom<u32> for OptErrCode {
    type Error = crate::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            6001 => Ok(OptErrCode(Some(ErrCode::BalanceInsufficient))),
            6002 => Ok(OptErrCode(Some(ErrCode::FeeInsufficient))),
            6003 => Ok(OptErrCode(Some(ErrCode::AddressFormatIncorrect))),
            6004 => Ok(OptErrCode(Some(ErrCode::NodeError))),
            6005 => Ok(OptErrCode(Some(ErrCode::NetworkException))),
            6006 => Ok(OptErrCode(Some(ErrCode::TransactionOnChainException))),
            6008 => Ok(OptErrCode(Some(ErrCode::SDKInternalError))),
            6099 => Ok(OptErrCode(Some(ErrCode::UnknownError))),
            _ => Ok(OptErrCode(None)),
        }
    }
}

#[derive(
    sqlx::Type,
    Debug,
    Clone,
    Copy,
    serde_repr::Deserialize_repr,
    serde_repr::Serialize_repr,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
#[repr(u8)]
pub enum WithdrawFailureStage {
    Unknown = 0,
    Build = 1,
    Broadcast = 2,
    Chain = 3,
    TxResultAck = 4,
}

/// Canonical supported EVM routes come from wallet-types, not a DB whitelist.
pub(crate) fn signed_evm_chain_codes() -> &'static [String] {
    static CODES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CODES.get_or_init(|| {
        wallet_types::chain::chain::ChainCodes::try_from(
            wallet_types::constant::chain_type::ETH_TYPE,
        )
        .expect("wallet-types must define the EVM chain type")
        .0
        .into_iter()
        .map(|chain| chain.to_string())
        .collect()
    })
}

pub(crate) fn is_signed_evm_chain(code: &str) -> bool {
    signed_evm_chain_codes().iter().any(|supported| supported == code)
}

pub(crate) fn signed_evm_chain_codes_json() -> &'static str {
    static JSON: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(signed_evm_chain_codes())
            .expect("chain strings are JSON serializable")
    })
}
