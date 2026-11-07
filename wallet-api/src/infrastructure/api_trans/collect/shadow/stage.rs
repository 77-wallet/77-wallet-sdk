use chrono::Utc;
use wallet_database::entities::api_collect::{ApiCollectEntity, ApiCollectStatus};

use crate::domain::api_wallet::trans::shared_mechanism;

const COMPLETED_FEE_BALANCE_RETRY_COOLDOWN_SECS: i64 = 30;

fn completed_fee_balance_retry_is_cooling_down(collect: &ApiCollectEntity) -> bool {
    collect.status == ApiCollectStatus::InsufficientBalance
        && collect.service_fee_uploaded_at.is_some()
        && collect.tx_fee_res_ack_sent_at.is_some()
        && collect.updated_at.is_some_and(|updated_at| {
            Utc::now().signed_duration_since(updated_at).num_seconds()
                < COMPLETED_FEE_BALANCE_RETRY_COOLDOWN_SECS
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectStage {
    NeedOrderAck,
    NeedResourceGate,
    CanBuild,
    NeedTxFeeResAck,
    CanBroadcast,
    NeedRecover,
    NeedTxExecReceiptUpload,
    NeedResultAck,
    NeedServiceFeeUpload,
    FullyBlocked,
}

/// 推进点顺序常量
/// - 顺序与 scan_round 完全一致
/// - try_advance 必须使用此常量，确保行为一致性
/// - 诊断逻辑也必须使用此顺序
pub const COLLECT_ADVANCEMENT_ORDER: &[CollectStage] = &[
    CollectStage::NeedOrderAck,
    CollectStage::NeedResourceGate,
    CollectStage::CanBuild,
    CollectStage::NeedTxFeeResAck,
    CollectStage::CanBroadcast,
    CollectStage::NeedRecover,
    CollectStage::NeedTxExecReceiptUpload,
    CollectStage::NeedResultAck,
    CollectStage::NeedServiceFeeUpload,
];

impl CollectStage {
    /// 转换为静态字符串，用于 metrics label
    pub fn as_str(&self) -> &'static str {
        match self {
            CollectStage::NeedOrderAck => "need_order_ack",
            CollectStage::NeedResourceGate => "need_resource_gate",
            CollectStage::CanBuild => "can_build",
            CollectStage::NeedTxFeeResAck => "need_tx_fee_res_ack",
            CollectStage::CanBroadcast => "can_broadcast",
            CollectStage::NeedRecover => "need_recover",
            CollectStage::NeedTxExecReceiptUpload => "need_tx_exec_receipt_upload",
            CollectStage::NeedResultAck => "need_result_ack",
            CollectStage::NeedServiceFeeUpload => "need_service_fee_upload",
            CollectStage::FullyBlocked => "fully_blocked",
        }
    }

    /// 获取阶段基础严重程度
    pub fn base_severity(&self) -> u8 {
        match self {
            CollectStage::NeedOrderAck => 1,
            CollectStage::NeedResourceGate => 1,
            CollectStage::CanBuild => 0,
            CollectStage::NeedTxFeeResAck => 1,
            CollectStage::CanBroadcast => 0,
            CollectStage::NeedRecover => 2,
            CollectStage::NeedTxExecReceiptUpload => 1,
            CollectStage::NeedResultAck => 1,
            CollectStage::NeedServiceFeeUpload => 1,
            CollectStage::FullyBlocked => 4,
        }
    }

    /// 获取阶段等待时间阈值（分钟）
    pub fn wait_threshold_minutes(&self) -> i64 {
        match self {
            CollectStage::NeedOrderAck => 2,
            CollectStage::NeedResourceGate => 2,
            CollectStage::CanBuild => 5,
            CollectStage::NeedTxFeeResAck => 3,
            CollectStage::CanBroadcast => 5,
            CollectStage::NeedRecover => 10,
            CollectStage::NeedTxExecReceiptUpload => 15,
            CollectStage::NeedResultAck => 8,
            CollectStage::NeedServiceFeeUpload => 5,
            CollectStage::FullyBlocked => 1,
        }
    }
}

/// 阶段查询构建器
/// 统一 SQL predicate 和 Rust predicate 的来源
/// 确保扫描和推进使用相同的阶段定义
pub trait StageQueryBuilder {
    /// 获取阶段的 SQL 过滤条件
    fn sql_filter(stage: CollectStage) -> String;

    /// 获取阶段的 Rust predicate 函数
    fn rust_predicate(
        stage: CollectStage,
    ) -> fn(&wallet_database::entities::api_collect::ApiCollectEntity) -> bool;
}

/// 默认的阶段查询构建器实现
pub struct DefaultStageQueryBuilder;

impl StageQueryBuilder for DefaultStageQueryBuilder {
    /// 获取阶段的 SQL 过滤条件
    fn sql_filter(stage: CollectStage) -> String {
        match stage {
            CollectStage::NeedOrderAck => {
                "order_ack_sent_at IS NULL".to_string()
            }
            CollectStage::NeedResourceGate => {
                "order_ack_sent_at IS NOT NULL AND lower(chain_code) = 'tron' AND resource_gate_released_at IS NULL AND (resource_block_reason IS NULL OR (resource_block_reason = 2 AND resource_dependency_trade_no IS NULL)) AND raw_tx IS NULL AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL".to_string()
            }
            CollectStage::CanBuild => {
                // BuildTx 只看“当前周期是否真的进入过服务费上传”。
                // 如果只是重开后保留了旧 ack，而当前周期并没有上传过服务费，
                // 这里就不应该把它当成“必须先等手续费结果”的单子。
                // TRON 归集还必须先有资源闸门释放事实，才能进入 BuildTx。
                "order_ack_sent_at IS NOT NULL AND raw_tx IS NULL AND (need_service_fee IS NULL OR need_service_fee = false) AND (lower(chain_code) <> 'tron' OR resource_gate_released_at IS NOT NULL) AND (service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NOT NULL) AND (status <> 1 OR service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NULL OR julianday(updated_at) <= julianday('now', '-30 seconds')) AND transaction_time IS NULL AND finished_at IS NULL AND err_code IS NULL".to_string()
            }
            CollectStage::NeedTxFeeResAck => {
                // 只有当前周期已经真的进入服务费上传，才会进入这个等待态。
                // 否则就是“历史上曾经需要过手续费”，但当前并没有新的手续费结果要等。
                "(need_service_fee IS NULL OR need_service_fee = false) AND service_fee_uploaded_at IS NOT NULL AND ever_needed_service_fee = true AND tx_fee_res_ack_sent_at IS NULL AND last_broadcast_at IS NULL AND finished_at IS NULL AND transaction_time IS NULL".to_string()
            }
            CollectStage::CanBroadcast => {
                // 广播前同样只关心当前周期是否已经走到服务费上传这一步。
                // 没有进入过该阶段，就不该被旧的手续费 ACK 事实阻塞。
                "raw_tx IS NOT NULL AND last_broadcast_at IS NULL AND transaction_time IS NULL AND finished_at IS NULL AND (service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NOT NULL) AND (chain_code NOT IN ('bnb','eth','polygon','avalanche','fantom','arbitrum','base','scroll','sol','tron') OR broadcast_uncertain_since_at IS NULL)".to_string()
            }
            CollectStage::NeedRecover => {
                "tx_hash IS NOT NULL AND transaction_time IS NULL AND tx_exec_receipt_uploaded_at IS NULL AND finished_at IS NULL AND err_code IS NULL".to_string()
            }
            CollectStage::NeedTxExecReceiptUpload => {
                "tx_exec_receipt_uploaded_at IS NULL AND finished_at IS NULL AND (err_code IS NOT NULL OR transaction_time IS NOT NULL)".to_string()
            }
            CollectStage::NeedResultAck => {
                "transaction_time IS NOT NULL AND result_ack_sent_at IS NULL AND finished_at IS NULL".to_string()
            }
            CollectStage::NeedServiceFeeUpload => {
                "need_service_fee = true AND service_fee_uploaded_at IS NULL AND err_code IS NULL AND finished_at IS NULL".to_string()
            }
            CollectStage::FullyBlocked => {
                "".to_string()
            }
        }
    }

    /// 获取阶段的 Rust predicate 函数
    fn rust_predicate(
        stage: CollectStage,
    ) -> fn(&wallet_database::entities::api_collect::ApiCollectEntity) -> bool {
        match stage {
            CollectStage::NeedOrderAck => |collect| collect.order_ack_sent_at.is_none(),
            CollectStage::NeedResourceGate => |collect| {
                collect.order_ack_sent_at.is_some()
                    && shared_mechanism::is_tron_chain(&collect.chain_code)
                    && collect.resource_gate_released_at.is_none()
                    && collect.raw_tx.is_none()
                    && collect.transaction_time.is_none()
                    && collect.finished_at.is_none()
                    && collect.err_code.is_none()
            },
            CollectStage::CanBuild => |collect| {
                // 当前周期没走到服务费上传时，旧 ACK 不应影响构建。
                collect.order_ack_sent_at.is_some()
                    && collect.raw_tx.is_none()
                    && collect.need_service_fee != Some(true)
                    && (!shared_mechanism::is_tron_chain(&collect.chain_code)
                        || collect.resource_gate_released_at.is_some())
                    && (collect.service_fee_uploaded_at.is_none()
                        || collect.tx_fee_res_ack_sent_at.is_some())
                    && !completed_fee_balance_retry_is_cooling_down(collect)
                    && collect.transaction_time.is_none()
                    && collect.finished_at.is_none()
                    && collect.err_code.is_none()
            },
            CollectStage::NeedTxFeeResAck => |collect| {
                // 只有当前周期已经上传过服务费，才需要等待手续费结果 ACK。
                collect.need_service_fee != Some(true)
                    && collect.service_fee_uploaded_at.is_some()
                    && collect.ever_needed_service_fee == true
                    && collect.tx_fee_res_ack_sent_at.is_none()
                    && collect.last_broadcast_at.is_none()
                    && collect.finished_at.is_none()
                    && collect.transaction_time.is_none()
            },
            CollectStage::CanBroadcast => |collect| {
                let broadcast_uncertain_in_progress =
                    shared_mechanism::tracks_broadcast_uncertain_state(&collect.chain_code)
                        && collect.broadcast_uncertain_since_at.is_some();
                // 同 BuildTx 一样，广播只看当前周期是否真的进过服务费上传。
                collect.raw_tx.is_some()
                    && collect.last_broadcast_at.is_none()
                    && collect.transaction_time.is_none()
                    && collect.finished_at.is_none()
                    && (collect.service_fee_uploaded_at.is_none()
                        || collect.tx_fee_res_ack_sent_at.is_some())
                    && !broadcast_uncertain_in_progress
            },
            CollectStage::NeedRecover => |collect| {
                collect.tx_hash.is_some()
                    && collect.transaction_time.is_none()
                    && collect.tx_exec_receipt_uploaded_at.is_none()
                    && collect.finished_at.is_none()
                    && collect.err_code.is_none()
            },
            CollectStage::NeedTxExecReceiptUpload => |collect| {
                collect.tx_exec_receipt_uploaded_at.is_none()
                    && collect.finished_at.is_none()
                    && (collect.err_code.is_some() || collect.transaction_time.is_some())
            },
            CollectStage::NeedResultAck => |collect| {
                collect.transaction_time.is_some()
                    && collect.result_ack_sent_at.is_none()
                    && collect.finished_at.is_none()
            },
            CollectStage::NeedServiceFeeUpload => |collect| {
                collect.need_service_fee == Some(true)
                    && collect.service_fee_uploaded_at.is_none()
                    && collect.finished_at.is_none()
                    && collect.err_code.is_none()
            },
            CollectStage::FullyBlocked => |_| false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMPLETED_FEE_BALANCE_RETRY_COOLDOWN_SECS, CollectStage, DefaultStageQueryBuilder,
        StageQueryBuilder,
    };
    use chrono::Utc;
    use wallet_database::entities::{
        api_collect::{ApiCollectEntity, ApiCollectStatus, ErrCode},
        asset_token_key::AssetTokenKey,
    };

    fn base_collect() -> ApiCollectEntity {
        ApiCollectEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "sol".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "USDT".to_string(),
            trade_no: "C_STAGE_TEST".to_string(),
            trade_type: 2,
            risk_addr: 0,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some(String::new()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some(String::new()),
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            order_ack_sent_at: Some(Utc::now()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            tx_res_received_at: None,
            service_fee_order_received_at: None,
            service_fee_uploaded_at: None,
            need_service_fee: None,
            ever_needed_service_fee: false,
            tx_fee_res_ack_sent_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn need_tx_exec_receipt_upload_allows_transaction_time_without_last_broadcast() {
        let mut c = base_collect();
        c.transaction_time = Some(Utc::now());
        c.last_broadcast_at = None;
        c.tx_exec_receipt_uploaded_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedTxExecReceiptUpload);
        assert!(pred(&c));
    }

    #[test]
    fn need_recover_allows_broadcast_visible_pending_chain_result() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedRecover);
        assert!(pred(&c));
    }

    #[test]
    fn need_tx_exec_receipt_upload_rejects_broadcast_only_pending() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;
        c.err_code = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedTxExecReceiptUpload);
        assert!(!pred(&c));
    }

    #[test]
    fn sql_filter_aligns_with_broadcast_visible_recover_semantics() {
        let can_build_sql = DefaultStageQueryBuilder::sql_filter(CollectStage::CanBuild);
        assert!(can_build_sql.contains("service_fee_uploaded_at IS NULL"));
        assert!(can_build_sql.contains("need_service_fee IS NULL OR need_service_fee = false"));
        assert!(can_build_sql.contains("lower(chain_code) <> 'tron'"));
        assert!(can_build_sql.contains("resource_gate_released_at IS NOT NULL"));
        assert!(can_build_sql.contains("julianday('now', '-30 seconds')"));
        assert!(can_build_sql.contains("transaction_time IS NULL"));
        assert!(can_build_sql.contains("finished_at IS NULL"));
        assert!(
            can_build_sql
                .contains("service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NOT NULL")
        );

        let fee_ack_sql = DefaultStageQueryBuilder::sql_filter(CollectStage::NeedTxFeeResAck);
        assert!(fee_ack_sql.contains("service_fee_uploaded_at IS NOT NULL"));
        assert!(fee_ack_sql.contains("need_service_fee IS NULL OR need_service_fee = false"));

        let fee_upload_sql =
            DefaultStageQueryBuilder::sql_filter(CollectStage::NeedServiceFeeUpload);
        assert!(fee_upload_sql.contains("service_fee_uploaded_at IS NULL"));
        assert!(!fee_upload_sql.contains("service_fee_order_received_at"));

        let can_broadcast_sql = DefaultStageQueryBuilder::sql_filter(CollectStage::CanBroadcast);
        assert!(
            can_broadcast_sql
                .contains("service_fee_uploaded_at IS NULL OR tx_fee_res_ack_sent_at IS NOT NULL")
        );

        let recover_sql = DefaultStageQueryBuilder::sql_filter(CollectStage::NeedRecover);
        assert!(!recover_sql.contains("last_broadcast_at IS NULL"));
        assert!(recover_sql.contains("tx_hash IS NOT NULL"));
        assert!(recover_sql.contains("transaction_time IS NULL"));

        let receipt_sql =
            DefaultStageQueryBuilder::sql_filter(CollectStage::NeedTxExecReceiptUpload);
        assert!(!receipt_sql.contains("last_broadcast_at"));
        assert!(receipt_sql.contains("transaction_time IS NOT NULL"));
        assert!(receipt_sql.contains("err_code IS NOT NULL"));

        let resource_gate_sql =
            DefaultStageQueryBuilder::sql_filter(CollectStage::NeedResourceGate);
        assert!(resource_gate_sql.contains("lower(chain_code) = 'tron'"));
        assert!(resource_gate_sql.contains("resource_gate_released_at IS NULL"));
        assert!(resource_gate_sql.contains("raw_tx IS NULL"));
    }

    #[test]
    fn completed_fee_balance_shortage_cools_down_before_build_retry() {
        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        let mut collect = base_collect();
        collect.status = ApiCollectStatus::InsufficientBalance;
        collect.raw_tx = None;
        collect.need_service_fee = Some(false);
        collect.service_fee_uploaded_at = Some(Utc::now());
        collect.tx_fee_res_ack_sent_at = Some(Utc::now());
        collect.updated_at = Some(Utc::now());

        assert!(!pred(&collect));

        collect.updated_at = Some(
            Utc::now() - chrono::Duration::seconds(COMPLETED_FEE_BALANCE_RETRY_COOLDOWN_SECS + 1),
        );
        assert!(pred(&collect));
    }

    #[test]
    fn can_build_requires_fee_cycle_cleared() {
        let mut stale = base_collect();
        stale.raw_tx = None;
        stale.need_service_fee = Some(true);
        stale.service_fee_uploaded_at = Some(Utc::now());

        let mut ready = base_collect();
        ready.raw_tx = None;
        ready.need_service_fee = Some(false);
        ready.service_fee_uploaded_at = Some(Utc::now());
        ready.tx_fee_res_ack_sent_at = Some(Utc::now());

        let mut blocked = base_collect();
        blocked.raw_tx = None;
        blocked.need_service_fee = Some(true);
        blocked.service_fee_order_received_at = None;
        blocked.service_fee_uploaded_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(!pred(&stale));
        assert!(pred(&ready));
        assert!(!pred(&blocked));
    }

    #[test]
    fn can_build_requires_resource_gate_for_tron_only() {
        let mut blocked = base_collect();
        blocked.chain_code = "tron".to_string();
        blocked.raw_tx = None;
        blocked.need_service_fee = Some(false);
        blocked.resource_gate_released_at = None;

        let mut released = blocked.clone();
        released.resource_gate_released_at = Some(Utc::now());

        let mut non_tron = blocked.clone();
        non_tron.chain_code = "sol".to_string();

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(!pred(&blocked));
        assert!(pred(&released));
        assert!(pred(&non_tron));
    }

    #[test]
    fn need_resource_gate_selects_unreleased_tron_only() {
        let mut blocked = base_collect();
        blocked.chain_code = "tron".to_string();
        blocked.raw_tx = None;
        blocked.resource_gate_released_at = None;

        let mut released = blocked.clone();
        released.resource_gate_released_at = Some(Utc::now());

        let mut non_tron = blocked.clone();
        non_tron.chain_code = "sol".to_string();

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedResourceGate);
        assert!(pred(&blocked));
        assert!(!pred(&released));
        assert!(!pred(&non_tron));
    }

    #[test]
    fn can_broadcast_blocks_sol_uncertain_state() {
        let mut blocked = base_collect();
        blocked.chain_code = "sol".to_string();
        blocked.broadcast_uncertain_since_at = Some(Utc::now());

        let mut ready = base_collect();
        ready.chain_code = "sol".to_string();
        ready.broadcast_uncertain_since_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBroadcast);
        assert!(!pred(&blocked));
        assert!(pred(&ready));
    }

    #[test]
    fn can_broadcast_blocks_added_evm_uncertain_state() {
        let mut blocked = base_collect();
        blocked.chain_code = "polygon".to_string();
        blocked.broadcast_uncertain_since_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBroadcast);
        assert!(!pred(&blocked));
    }

    #[test]
    fn can_broadcast_sql_blocks_sol_uncertain_state() {
        let sql = DefaultStageQueryBuilder::sql_filter(CollectStage::CanBroadcast);

        for chain_code in ["polygon", "avalanche", "fantom", "arbitrum", "base", "scroll"] {
            assert!(sql.contains(&format!("'{chain_code}'")));
        }
        assert!(sql.contains("broadcast_uncertain_since_at IS NULL"));
    }

    #[test]
    fn can_build_rejects_committed_or_finished_orders() {
        let mut committed = base_collect();
        committed.raw_tx = None;
        committed.need_service_fee = Some(false);
        committed.transaction_time = Some(Utc::now());

        let mut finished = base_collect();
        finished.raw_tx = None;
        finished.need_service_fee = Some(false);
        finished.finished_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(!pred(&committed));
        assert!(!pred(&finished));
    }

    #[test]
    fn can_build_rejects_failed_orders() {
        let mut failed = base_collect();
        failed.raw_tx = None;
        failed.need_service_fee = Some(false);
        failed.err_code = Some(ErrCode::UnknownError);

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(!pred(&failed));
    }

    #[test]
    fn can_build_requires_fee_res_ack_after_completed_fee_cycle() {
        let mut blocked = base_collect();
        blocked.raw_tx = None;
        blocked.need_service_fee = Some(false);
        blocked.ever_needed_service_fee = true;
        blocked.service_fee_uploaded_at = Some(Utc::now());
        blocked.tx_fee_res_ack_sent_at = None;

        let mut ready = blocked.clone();
        ready.tx_fee_res_ack_sent_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(!pred(&blocked));
        assert!(pred(&ready));
    }

    #[test]
    fn can_build_does_not_require_fee_res_ack_when_service_fee_was_not_uploaded() {
        let mut ready = base_collect();
        ready.raw_tx = None;
        ready.need_service_fee = Some(false);
        ready.ever_needed_service_fee = true;
        ready.service_fee_uploaded_at = None;
        ready.tx_fee_res_ack_sent_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBuild);
        assert!(pred(&ready));
    }

    #[test]
    fn need_tx_fee_res_ack_requires_uploaded_service_fee() {
        let mut uploaded = base_collect();
        uploaded.raw_tx = None;
        uploaded.need_service_fee = Some(false);
        uploaded.service_fee_uploaded_at = Some(Utc::now());
        uploaded.tx_fee_res_ack_sent_at = None;
        uploaded.ever_needed_service_fee = true;

        let mut not_uploaded = uploaded.clone();
        not_uploaded.service_fee_uploaded_at = None;

        let mut already_acked = uploaded.clone();
        already_acked.tx_fee_res_ack_sent_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedTxFeeResAck);
        assert!(pred(&uploaded));
        assert!(!pred(&not_uploaded));
        assert!(!pred(&already_acked));
    }

    #[test]
    fn can_broadcast_does_not_require_fee_res_ack_without_service_fee_upload() {
        let mut ready = base_collect();
        ready.raw_tx = Some("{}".to_string());
        ready.need_service_fee = Some(false);
        ready.ever_needed_service_fee = true;
        ready.service_fee_uploaded_at = None;
        ready.tx_fee_res_ack_sent_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::CanBroadcast);
        assert!(pred(&ready));
    }

    #[test]
    fn need_service_fee_upload_only_requires_need_service_fee() {
        let mut c = base_collect();
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = None;
        c.service_fee_uploaded_at = None;
        c.err_code = None;
        c.finished_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedServiceFeeUpload);
        assert!(pred(&c));
    }

    #[test]
    fn need_service_fee_upload_blocks_when_already_uploaded() {
        let mut c = base_collect();
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = None;
        c.service_fee_uploaded_at = Some(Utc::now());
        c.err_code = None;
        c.finished_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedServiceFeeUpload);
        assert!(!pred(&c));
    }

    #[test]
    fn need_tx_exec_receipt_upload_allows_err_code_without_last_broadcast() {
        let mut c = base_collect();
        c.err_code = Some(ErrCode::UnknownError);
        c.last_broadcast_at = None;
        c.transaction_time = None;
        c.tx_exec_receipt_uploaded_at = None;

        let pred = DefaultStageQueryBuilder::rust_predicate(CollectStage::NeedTxExecReceiptUpload);
        assert!(pred(&c));
    }
}
