use crate::entities::asset_token_key::AssetTokenKey;
use serde::Deserializer;
use std::fmt::Display;

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

// 错误码枚举
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
/// ApiFeeEntity 是一个【事实驱动实体】
///
/// 设计原则：
/// - 本表不再是状态机
/// - 所有字段表示"已经发生的事实"
/// - 不允许通过更新字段来表达"即将发生"或"期望状态"
///
/// 时间字段语义：
/// - transaction_time / finished_at：
///   仅表示链上事实已完成
/// - *_uploaded_at / *_sent_at / *_confirmed_at：
///   表示副作用完成时间
///
/// 严禁：
/// - 在副作用逻辑中修改 finished_at
/// - 使用 status 推导未来行为
///
/// 所有行为必须通过【事实谓词扫描】触发。
pub struct ApiFeeEntity {
    // ===== Identity / Business =====
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
    pub trade_type: u8,
    pub status: ApiFeeStatus, // UI/人类可读状态，不参与执行逻辑
    pub nonce: i64,
    pub tx_hash: Option<String>,
    #[serde(skip_serializing)]
    pub raw_tx: Option<String>,
    #[serde(skip_serializing)]
    pub resource_consume: String,
    pub transaction_fee: String,
    pub transaction_time: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    pub block_height: Option<String>,
    pub notes: Option<String>,
    pub post_tx_count: u32,
    pub post_confirm_tx_count: u32,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_err_code"
    )]
    /// err_code 表示"是否发生过终止型错误事实"
    /// - None: 没有发生终止型错误
    /// - Some(ErrCode): 发生过不可逆执行失败
    pub err_code: Option<ErrCode>,
    pub err_msg: Option<String>,

    // ===== Tx ACK（交易 ACK 事实）=====
    /// Tx ACK：确认已发送交易 ACK（推进事实）
    pub tx_ack_sent_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,

    // ===== Build / Broadcast Execution Facts =====
    pub building_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // BuildTx 执行占位
    pub last_broadcast_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 最近一次 Broadcast 执行占位
    /// EVM uncertain tracking (RPC accepted/hash known but tx not visible on same RPC node yet)
    pub broadcast_uncertain_since_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// Number of uncertain observations (broadcast/recover) for this tx hash lifecycle
    pub broadcast_uncertain_retry_count: u32,
    /// Last time we checked/recorded uncertain status (used for backoff throttling)
    pub broadcast_uncertain_last_checked_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// Timeout-reconcile marker (run at most once per lifecycle)
    pub broadcast_uncertain_reconciled_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// Automatic rebuild/rebroadcast retries attempted after uncertain timeout
    pub broadcast_uncertain_rebroadcast_count: u32,

    // ===== Tx Exec Receipt Upload（交易执行回执上传事实）=====
    /// Tx Exec Receipt Upload：确认已上传交易执行回执（推进事实）
    pub tx_exec_receipt_uploaded_at:
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,

    // ===== Result ACK（结果确认事实）=====
    /// Tx Res ACK：确认已将链上结果可靠告知后端（推进事实）
    pub tx_res_ack_sent_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    /// SER TxRes push received timestamp (AWM_ORDER_TRANS_RES)
    /// - Hard gate: TX_RES ack MUST NOT be sent before this fact exists.
    pub tx_res_received_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,

    // ===== Terminal Fact =====
    pub finished_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, // 链上终态事实

    // ===== Meta =====
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
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
)]
#[repr(u8)]
/// 手续费交易状态（历史兼容字段）
///
/// ⚠️ 架构定位：
/// - 本枚举为历史遗留设计
/// - 与当前事实驱动模型并非一一对应
///
/// 当前用途：
/// - UI 展示
/// - 运维查看
/// - 统计分析
/// - 后台分页
///
/// ❌ 禁止：
/// - Scanner / Executor 通过 status 判断是否可执行
/// - 用于执行决策
/// - 用于判断是否可推进某一步
///
/// 未来演进：
/// - 可能被更粗粒度的阶段字段替代
/// - 或完全由前端基于事实字段自行映射
///
/// 注意：status 是事实的派生字段，不是事实本身
pub enum ApiFeeStatus {
    Init,                  // 0, 初始化
    SendingTx,             // 1, 发送交易成功，有hash
    SendingTxFailed,       // 2, 发送交易失败
    SendingTxReport,       // 3, 发送交易报告给服务器
    SendingTxFailedReport, // 4, 发送交易失败报告给服务器，结束
    Success,               // 5，收到成功确认
    Failure,               // 6，收到失败确认
    ConfirmSuccessReport,  // 7, 结束
    ConfirmFailureReport,  // 8, 结束
}

impl Display for ApiFeeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl ApiFeeStatus {
    /// 判断状态是否为终态
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ApiFeeStatus::SendingTxFailedReport
                | ApiFeeStatus::ConfirmSuccessReport
                | ApiFeeStatus::ConfirmFailureReport
        )
    }
}

impl ApiFeeEntity {
    /// 判断是否需要发送交易 ACK
    ///
    /// 前置事实：
    /// - fee 已落库（id 存在）
    /// - 交易 ACK 尚未发送
    ///
    /// 注意：
    /// - attempted 仅用于执行保护，不作为调度条件
    /// - 此函数是事实谓词，用于 Scanner 和 Worker 的双重保险
    pub fn need_tx_ack(&self) -> bool {
        self.tx_ack_sent_at.is_none()
    }

    /// 根据当前事实字段，派生一个"最接近的叙事状态"
    ///
    /// ⚠️ 重要说明：
    /// - ApiFeeStatus 是【历史遗留枚举】
    /// - 与当前事实字段【不是一一对应关系】
    /// - 本方法仅用于：
    ///   - UI 展示
    ///   - 后台分页 / 运维查看
    ///   - 统计
    ///
    /// ❌ 严禁：
    /// - 用于执行决策
    /// - 用于判断是否可推进某一步
    ///
    /// 设计取舍：
    /// - 这是一个"叙事映射（Narrative Mapping）"
    /// - 不是状态机
    /// - 不保证语义完全精确，只保证"人类可理解"
    ///
    /// 纯派生函数（Pure Function）
    ///
    /// 特性保证：
    /// - 不读取数据库
    /// - 不修改任何字段
    /// - 不依赖时间先后
    /// - 不依赖当前 status 值
    ///
    /// 对同一个实体：
    /// - 多次调用结果必然一致
    pub fn recompute_status(&self) -> ApiFeeStatus {
        use ApiFeeStatus::*;

        // 终态优先（不可逆）
        if self.tx_res_ack_sent_at.is_some() {
            return ConfirmSuccessReport;
        }

        if self.tx_exec_receipt_uploaded_at.is_some() {
            return SendingTxReport;
        }

        if self.transaction_time.is_some() {
            return SendingTx;
        }

        // if !self.raw_tx.is_empty() {
        //     return SufficientBalance;
        // }

        if self.tx_ack_sent_at.is_some() {
            return Init;
        }

        Init
    }

    /// 是否已进入【事实终态】
    ///
    /// 事实终态定义：
    /// - 链上结果已确认（finished_at）
    /// - 或 已可靠告知后端（tx_res_ack_sent_at）
    ///
    /// 一旦进入事实终态：
    /// - 不允许再产生任何推进性副作用
    /// - 只能允许"幂等重试型"行为（如补 ACK）
    pub fn is_fact_terminal(&self) -> bool {
        self.finished_at.is_some() || self.tx_res_ack_sent_at.is_some()
    }
}

#[derive(Debug)]
pub struct FeeCreatedFact {
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
    pub status: ApiFeeStatus,
}
