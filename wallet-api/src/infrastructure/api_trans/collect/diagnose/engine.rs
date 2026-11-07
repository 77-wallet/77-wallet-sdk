use std::fmt;
use wallet_database::entities::api_collect::ApiCollectEntity;

use super::fact_snapshot::{dump_fact_snapshot, fact_mask};
use crate::infrastructure::api_trans::collect::shadow::{
    predicate::evaluate_stage,
    stage::{COLLECT_ADVANCEMENT_ORDER, CollectStage},
};

#[derive(Debug, Clone)]
pub struct DiagnoseResult {
    pub stage: CollectStage,
    pub reasons: Vec<String>,
    pub facts_snapshot: String,
    pub facts_mask: (u64, u8),
    pub stuck_score: u8, // 0-4，0=可推进，4=完全阻塞
    pub stage_index: u8,
    pub wait_times: Vec<String>,
    pub next_expected_fact: Option<&'static str>,
}

impl fmt::Display for DiagnoseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stage={:?}, reasons={:?}, facts={}",
            self.stage, self.reasons, self.facts_snapshot
        )
    }
}

/// 诊断卡单原因
/// 使用统一的阶段评估，确保与推进顺序完全一致
pub fn diagnose_collect(collect: &ApiCollectEntity) -> DiagnoseResult {
    let wait_times = Vec::new();

    // 按推进顺序评估每个阶段，返回第一个可推进的阶段
    for (index, stage) in COLLECT_ADVANCEMENT_ORDER.iter().enumerate() {
        let eval = evaluate_stage(*stage, collect);

        if eval.can_advance {
            // Scanner 会冻结“Success + 空 tx_hash”的执行回执上传重试，诊断需要明确提示
            // 这是等待事实补齐（tx_hash backfill），不是普通 retry 中。
            if *stage == CollectStage::NeedTxExecReceiptUpload
                && is_tx_exec_receipt_success_missing_hash_blocked(collect)
            {
                return DiagnoseResult {
                    stage: *stage,
                    reasons: vec![
                        "TxExecReceipt blocked: success payload missing tx_hash (waiting tx_hash backfill)"
                            .to_string(),
                    ],
                    facts_snapshot: dump_fact_snapshot(collect),
                    facts_mask: fact_mask(collect),
                    stuck_score: calculate_severity(*stage, collect),
                    stage_index: index as u8,
                    wait_times: wait_times,
                    next_expected_fact: Some("tx_hash"),
                };
            }

            let reasons = if *stage == CollectStage::NeedServiceFeeUpload
                && collect.service_fee_order_received_at.is_some()
            {
                vec!["Backend fee order received; waiting for service fee upload".to_string()]
            } else {
                eval.reasons.into_iter().map(|r| r.message).collect()
            };

            return DiagnoseResult {
                stage: *stage,
                reasons,
                facts_snapshot: dump_fact_snapshot(collect),
                facts_mask: fact_mask(collect),
                stuck_score: calculate_severity(*stage, collect),
                stage_index: index as u8,
                wait_times: wait_times,
                next_expected_fact: Some(get_next_expected_fact(*stage)),
            };
        }
    }

    // 无可推进点：系统完全阻塞（通常是等待外部事实发生）
    let mut reasons = Vec::new();
    let mut next_expected_fact: Option<&'static str> = None;

    // 特例：需要服务费，但还没有上传服务费记录
    // 语义：等待 collect 侧把 UploadServiceFee 这一步补齐
    if collect.need_service_fee == Some(true) && collect.service_fee_uploaded_at.is_none() {
        if collect.service_fee_order_received_at.is_some() {
            reasons.push("Backend fee order received; waiting for service fee upload".to_string());
        } else {
            reasons.push("Waiting for service fee upload".to_string());
        }
        next_expected_fact = Some("service_fee_uploaded_at");
    }

    // 特例：广播事实和执行回执事实都已存在，但链上确认事实仍缺失。
    // 这类快照在 EVM 上常见于 RPC 接受提交但链上不可见/未确认。
    if reasons.is_empty()
        && collect.last_broadcast_at.is_some()
        && collect.transaction_time.is_none()
        && collect.tx_exec_receipt_uploaded_at.is_some()
        && collect.result_ack_sent_at.is_none()
        && collect.finished_at.is_none()
        && collect.err_code.is_none()
    {
        if collect.tx_res_received_at.is_none() {
            reasons.push("Waiting AWM_ORDER_TRANS_RES (tx result push)".to_string());
            next_expected_fact = Some("tx_res_received_at");
        } else {
            reasons.push(
                "Broadcast recorded but on-chain fact missing after TX_RES; confirm/recover path blocked or ineffective"
                    .to_string(),
            );
            next_expected_fact = Some("transaction_time");
        }
    }

    if reasons.is_empty() {
        reasons.push("No advancement possible".to_string());
    }

    DiagnoseResult {
        stage: CollectStage::FullyBlocked,
        reasons,
        facts_snapshot: dump_fact_snapshot(collect),
        facts_mask: fact_mask(collect),
        stuck_score: calculate_severity(CollectStage::FullyBlocked, collect),
        stage_index: COLLECT_ADVANCEMENT_ORDER.len() as u8,
        wait_times,
        next_expected_fact,
    }
}

fn is_tx_exec_receipt_success_missing_hash_blocked(collect: &ApiCollectEntity) -> bool {
    let tx_hash_missing =
        collect.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
    let has_success_execution_evidence =
        collect.err_code.is_none() && collect.transaction_time.is_some();

    collect.tx_exec_receipt_uploaded_at.is_none()
        && has_success_execution_evidence
        && tx_hash_missing
}

/// 计算严重程度（基于阶段和具体情况）
fn calculate_severity(stage: CollectStage, collect: &ApiCollectEntity) -> u8 {
    let base_severity = stage.base_severity();

    // 计算等待时间权重
    let wait_weight = calculate_wait_weight(stage, collect);

    // 总严重程度，最大为 4
    std::cmp::min(base_severity + wait_weight, 4)
}

/// 计算等待时间权重
fn calculate_wait_weight(stage: CollectStage, collect: &ApiCollectEntity) -> u8 {
    // 基于创建时间计算等待分钟数
    let now = chrono::Utc::now();
    let wait_minutes = (now - collect.created_at).num_minutes();

    // 获取阶段特定的等待时间阈值
    let threshold = stage.wait_threshold_minutes();

    // 等待时间超过阈值后开始计算权重，每超过阈值时间增加 1 权重，最大 2
    if wait_minutes < threshold {
        0
    } else {
        let excess_minutes = wait_minutes - threshold;
        (excess_minutes / threshold).min(2) as u8
    }
}

/// 获取下一期望事实
fn get_next_expected_fact(stage: CollectStage) -> &'static str {
    match stage {
        CollectStage::NeedOrderAck => "order_ack_sent_at",
        CollectStage::NeedResourceGate => "resource_gate_released_at",
        CollectStage::CanBuild => "raw_tx",
        CollectStage::NeedTxFeeResAck => "tx_fee_res_ack_sent_at",
        CollectStage::CanBroadcast => "last_broadcast_at",
        CollectStage::NeedRecover => "transaction_time",
        CollectStage::NeedTxExecReceiptUpload => "tx_exec_receipt_uploaded_at",
        CollectStage::NeedResultAck => "result_ack_sent_at",
        CollectStage::NeedServiceFeeUpload => "service_fee_uploaded_at",
        CollectStage::FullyBlocked => "finished_at",
    }
}

/// 可能的卡单记录诊断
pub fn is_potentially_stuck(collect: &ApiCollectEntity) -> bool {
    // 已完成或有错误的订单不视为卡单
    if collect.finished_at.is_some() || collect.err_code.is_some() {
        return false;
    }

    // 有推进点的订单不视为卡单
    let diag = diagnose_collect(collect);
    diag.stuck_score >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_collect::ApiCollectStatus, asset_token_key::AssetTokenKey,
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
            chain_code: "tron".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "USDT".to_string(),
            trade_no: "C_DIAG_TEST".to_string(),
            trade_type: 2,
            risk_addr: 0,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
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
    fn diagnose_tx_exec_receipt_missing_hash_blocked_reason() {
        let mut c = base_collect();
        c.transaction_time = Some(Utc::now());
        c.tx_hash = Some(String::new());
        c.err_code = None;
        c.tx_exec_receipt_uploaded_at = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::NeedTxExecReceiptUpload);
        assert!(
            diag.reasons
                .iter()
                .any(|r| r.contains("TxExecReceipt blocked: success payload missing tx_hash"))
        );
        assert_eq!(diag.next_expected_fact, Some("tx_hash"));
    }

    #[test]
    fn diagnose_broadcast_visible_pending_routes_to_recover() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;
        c.err_code = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::NeedRecover);
    }

    #[test]
    fn diagnose_tx_exec_receipt_fail_path_not_blocked_by_missing_hash_reason() {
        let mut c = base_collect();
        c.tx_hash = Some(String::new());
        c.err_code = Some(wallet_database::entities::api_collect::ErrCode::UnknownError);
        c.tx_exec_receipt_uploaded_at = None;

        let diag = diagnose_collect(&c);
        assert!(
            !diag.reasons.iter().any(|r| r.contains("waiting tx_hash backfill")),
            "fail path should not be frozen by missing tx_hash"
        );
    }

    #[test]
    fn diagnose_waiting_tx_res_reason_for_broadcasted_receipt_uploaded() {
        let mut c = base_collect();
        c.last_broadcast_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());
        c.transaction_time = None;
        c.tx_res_received_at = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::FullyBlocked);
        assert!(diag.reasons.iter().any(|r| r.contains("AWM_ORDER_TRANS_RES")));
        assert_eq!(diag.next_expected_fact, Some("tx_res_received_at"));
    }

    #[test]
    fn diagnose_service_fee_upload_without_backend_order_fact() {
        let mut c = base_collect();
        c.resource_gate_released_at = Some(Utc::now());
        c.raw_tx = None;
        c.tx_hash = None;
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = None;
        c.service_fee_uploaded_at = None;
        c.tx_fee_res_ack_sent_at = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::NeedServiceFeeUpload);
        assert_eq!(diag.next_expected_fact, Some("service_fee_uploaded_at"));
    }

    #[test]
    fn diagnose_service_fee_order_received_routes_to_upload_stage() {
        let mut c = base_collect();
        c.resource_gate_released_at = Some(Utc::now());
        c.raw_tx = None;
        c.tx_hash = None;
        c.need_service_fee = Some(true);
        c.service_fee_order_received_at = Some(Utc::now());
        c.service_fee_uploaded_at = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::NeedServiceFeeUpload);
        assert_eq!(diag.next_expected_fact, Some("service_fee_uploaded_at"));
        assert!(
            diag.reasons.iter().any(|r| r.contains("Backend fee order received")),
            "diagnosis should preserve backend-order context when present"
        );
    }

    #[test]
    fn diagnose_fresh_order_routes_to_need_order_ack() {
        let mut c = base_collect();
        c.order_ack_sent_at = None;
        c.raw_tx = None;
        c.tx_hash = None;
        c.tx_exec_receipt_uploaded_at = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::NeedOrderAck);
        assert_eq!(diag.next_expected_fact, Some("order_ack_sent_at"));
    }

    #[test]
    fn diagnose_fully_finished_record_is_fully_blocked() {
        let mut c = base_collect();
        c.finished_at = Some(Utc::now());
        c.result_ack_sent_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());
        c.transaction_time = Some(Utc::now());
        c.last_broadcast_at = Some(Utc::now());

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::FullyBlocked);
    }

    #[test]
    fn diagnose_returns_can_broadcast_stage_when_ready() {
        // raw_tx present, no broadcast yet, no service-fee cycle
        let mut c = base_collect();
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;

        let diag = diagnose_collect(&c);
        assert_eq!(diag.stage, CollectStage::CanBroadcast);
        assert_eq!(diag.next_expected_fact, Some("last_broadcast_at"));
    }

    #[test]
    fn is_potentially_stuck_false_for_finished_record() {
        let mut c = base_collect();
        c.finished_at = Some(Utc::now());
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());

        assert!(!is_potentially_stuck(&c));
    }

    #[test]
    fn is_potentially_stuck_false_for_error_record() {
        let mut c = base_collect();
        c.err_code = Some(wallet_database::entities::api_collect::ErrCode::UnknownError);
        c.tx_exec_receipt_uploaded_at = Some(Utc::now());

        assert!(!is_potentially_stuck(&c));
    }

    #[test]
    fn diagnose_facts_mask_version_is_2() {
        let c = base_collect();
        let diag = diagnose_collect(&c);
        assert_eq!(diag.facts_mask.1, 2);
    }

    #[test]
    fn diagnose_stage_index_aligns_with_advancement_order() {
        use crate::infrastructure::api_trans::collect::shadow::stage::COLLECT_ADVANCEMENT_ORDER;

        let mut c = base_collect();
        c.tx_exec_receipt_uploaded_at = None;
        c.transaction_time = None;

        let diag = diagnose_collect(&c);
        let expected_index =
            COLLECT_ADVANCEMENT_ORDER.iter().position(|s| *s == diag.stage).map(|i| i as u8);
        assert_eq!(Some(diag.stage_index), expected_index);
    }
}
