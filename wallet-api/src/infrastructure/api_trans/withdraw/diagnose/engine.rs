use std::fmt;

use wallet_database::entities::api_withdraw::ApiWithdrawEntity;

use super::fact_snapshot::{dump_fact_snapshot, fact_mask};
use crate::infrastructure::api_trans::withdraw::shadow::{
    ADVANCEMENT_ORDER, AdvancementPoint, evaluate_point,
};

#[derive(Debug, Clone)]
pub struct DiagnoseResult {
    pub stage: AdvancementPoint,
    pub reasons: Vec<String>,
    pub facts_snapshot: String,
    pub facts_mask: (u64, u8),
    pub stuck_score: u8, // 0-4
    pub stage_index: u8,
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

pub fn diagnose_withdraw(withdraw: &ApiWithdrawEntity) -> DiagnoseResult {
    for (index, point) in ADVANCEMENT_ORDER.iter().enumerate() {
        let eval = evaluate_point(*point, withdraw);
        if eval.can_advance {
            if *point == AdvancementPoint::NeedTxExecReceiptUpload
                && is_tx_exec_receipt_success_missing_hash_blocked(withdraw)
            {
                return DiagnoseResult {
                    stage: *point,
                    reasons: vec![
                        "TxExecReceipt blocked: success payload missing tx_hash (waiting tx_hash backfill)"
                            .to_string(),
                    ],
                    facts_snapshot: dump_fact_snapshot(withdraw),
                    facts_mask: fact_mask(withdraw),
                    stuck_score: calculate_severity(*point, withdraw),
                    stage_index: index as u8,
                    next_expected_fact: Some("tx_hash"),
                };
            }

            return DiagnoseResult {
                stage: *point,
                reasons: eval.reasons.into_iter().map(|r| r.message).collect(),
                facts_snapshot: dump_fact_snapshot(withdraw),
                facts_mask: fact_mask(withdraw),
                stuck_score: calculate_severity(*point, withdraw),
                stage_index: index as u8,
                next_expected_fact: Some(point.next_expected_fact()),
            };
        }
    }

    // Special cases when no advancement point is available.
    //
    // 1) Audit rejected is a terminal business decision. It should not be treated as "stuck".
    if withdraw.audit_rejected_at.is_some() {
        return DiagnoseResult {
            stage: AdvancementPoint::FullyBlocked,
            reasons: vec!["Audit rejected".to_string()],
            facts_snapshot: dump_fact_snapshot(withdraw),
            facts_mask: fact_mask(withdraw),
            stuck_score: 0,
            stage_index: ADVANCEMENT_ORDER.len() as u8,
            next_expected_fact: None,
        };
    }

    // 2) Tx ACK sent but audit not passed yet: surface audit gate as the next required fact.
    if withdraw.tx_ack_sent_at.is_some()
        && withdraw.audit_passed_at.is_none()
        && withdraw.audit_rejected_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none()
    {
        let can_build_index =
            ADVANCEMENT_ORDER.iter().position(|p| *p == AdvancementPoint::CanBuild).unwrap_or(0);
        return DiagnoseResult {
            stage: AdvancementPoint::CanBuild,
            reasons: vec!["Audit not passed yet".to_string()],
            facts_snapshot: dump_fact_snapshot(withdraw),
            facts_mask: fact_mask(withdraw),
            stuck_score: calculate_severity(AdvancementPoint::CanBuild, withdraw),
            stage_index: can_build_index as u8,
            next_expected_fact: Some("audit_passed_at"),
        };
    }

    // 3) 保持 TxRes 强顺序屏障：链上成功与回执上传都完成，但尚未收到 AWM_ORDER_TRANS_RES。
    if withdraw.transaction_time.is_some()
        && withdraw.tx_exec_receipt_uploaded_at.is_some()
        && withdraw.tx_res_received_at.is_none()
        && withdraw.tx_res_ack_sent_at.is_none()
        && withdraw.finished_at.is_none()
        && withdraw.err_code.is_none()
    {
        let tx_res_ack_index = ADVANCEMENT_ORDER
            .iter()
            .position(|p| *p == AdvancementPoint::NeedTxResAck)
            .unwrap_or(0);
        return DiagnoseResult {
            stage: AdvancementPoint::NeedTxResAck,
            reasons: vec![
                "Waiting AWM_ORDER_TRANS_RES (tx result push) before sending TxRes ACK".to_string(),
            ],
            facts_snapshot: dump_fact_snapshot(withdraw),
            facts_mask: fact_mask(withdraw),
            stuck_score: calculate_severity(AdvancementPoint::NeedTxResAck, withdraw),
            stage_index: tx_res_ack_index as u8,
            next_expected_fact: Some("tx_res_received_at"),
        };
    }

    DiagnoseResult {
        stage: AdvancementPoint::FullyBlocked,
        reasons: vec!["No advancement possible".to_string()],
        facts_snapshot: dump_fact_snapshot(withdraw),
        facts_mask: fact_mask(withdraw),
        stuck_score: calculate_severity(AdvancementPoint::FullyBlocked, withdraw),
        stage_index: ADVANCEMENT_ORDER.len() as u8,
        next_expected_fact: None,
    }
}

fn is_tx_exec_receipt_success_missing_hash_blocked(withdraw: &ApiWithdrawEntity) -> bool {
    let tx_hash_missing =
        withdraw.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
    let has_success_execution_evidence =
        withdraw.chain_success_at.is_some() || withdraw.transaction_time.is_some();

    withdraw.tx_exec_receipt_uploaded_at.is_none()
        && has_success_execution_evidence
        && tx_hash_missing
}

fn calculate_severity(stage: AdvancementPoint, withdraw: &ApiWithdrawEntity) -> u8 {
    let base = stage.base_severity();
    let wait = calculate_wait_weight(stage, withdraw);
    std::cmp::min(base + wait, 4)
}

fn calculate_wait_weight(stage: AdvancementPoint, withdraw: &ApiWithdrawEntity) -> u8 {
    let now = chrono::Utc::now();
    let wait_minutes = (now - withdraw.created_at).num_minutes();
    let threshold = stage.wait_threshold_minutes();

    if wait_minutes < threshold {
        0
    } else {
        let excess_minutes = wait_minutes - threshold;
        (excess_minutes / threshold).min(2) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallet_database::entities::{
        api_trade_type::ApiTradeType,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
        asset_token_key::AssetTokenKey,
    };

    fn base_withdraw(trade_no: &str) -> ApiWithdrawEntity {
        ApiWithdrawEntity {
            id: 0,
            name: "t".to_string(),
            uid: "u".to_string(),
            from_addr: "a".to_string(),
            to_addr: "b".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "c".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "s".to_string(),
            trade_no: trade_no.to_string(),
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: None,
            raw_tx: None,
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            estimated_transaction_fee: None,
            estimated_resource_consume: None,
            fee_estimated_at: None,
            transaction_time: None,
            block_height: None,
            notes: None,
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: None,
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            tx_ack_sent_at: None,
            audit_passed_at: None,
            audit_rejected_at: None,
            audit_reason: None,
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            chain_success_at: None,
            chain_failed_at: None,
            failure_stage: None,
            created_at: chrono::Utc::now() - chrono::Duration::minutes(30),
            updated_at: Some(chrono::Utc::now()),
            out_order_id: None,
            client_id: None,
            create_time: None,
        }
    }

    #[test]
    fn diagnose_stage_need_tx_ack_when_missing_ack() {
        let w = base_withdraw("W1");
        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stage, AdvancementPoint::NeedTxAck);
    }

    #[test]
    fn fact_mask_changes_when_ack_written() {
        let mut w = base_withdraw("W2");
        let (m1, v1) = fact_mask(&w);
        w.tx_ack_sent_at = Some(chrono::Utc::now());
        let (m2, v2) = fact_mask(&w);
        assert_eq!(v1, v2);
        assert_ne!(m1, m2);
    }

    #[test]
    fn diagnose_withdraw_next_fact_is_audit_passed_when_ack_sent_but_audit_missing() {
        let mut w = base_withdraw("W3");
        w.tx_ack_sent_at = Some(chrono::Utc::now());

        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stage, AdvancementPoint::CanBuild);
        assert_eq!(diag.next_expected_fact, Some("audit_passed_at"));
    }

    #[test]
    fn diagnose_withdraw_not_stuck_when_audit_rejected() {
        let mut w = base_withdraw("W4");
        w.tx_ack_sent_at = Some(chrono::Utc::now());
        w.audit_rejected_at = Some(chrono::Utc::now());

        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stuck_score, 0);
        assert!(diag.reasons.iter().any(|r| r.contains("Audit rejected")));
    }

    #[test]
    fn diagnose_withdraw_tx_exec_receipt_missing_hash_blocked_reason() {
        let mut w = base_withdraw("W5");
        w.tx_ack_sent_at = Some(chrono::Utc::now());
        w.audit_passed_at = Some(chrono::Utc::now());
        w.raw_tx = Some("{}".to_string());
        w.last_broadcast_at = Some(chrono::Utc::now());
        w.transaction_time = Some(chrono::Utc::now());
        w.tx_hash = Some(String::new());

        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stage, AdvancementPoint::NeedTxExecReceiptUpload);
        assert!(
            diag.reasons
                .iter()
                .any(|r| r.contains("TxExecReceipt blocked: success payload missing tx_hash"))
        );
        assert_eq!(diag.next_expected_fact, Some("tx_hash"));
    }

    #[test]
    fn diagnose_withdraw_broadcast_visible_pending_routes_to_recover() {
        let mut w = base_withdraw("W6");
        w.tx_ack_sent_at = Some(chrono::Utc::now());
        w.audit_passed_at = Some(chrono::Utc::now());
        w.raw_tx = Some("{}".to_string());
        w.last_broadcast_at = Some(chrono::Utc::now());
        w.tx_hash = Some("0xhash".to_string());

        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stage, AdvancementPoint::NeedRecover);
        assert_eq!(diag.next_expected_fact, Some("transaction_time"));
    }

    #[test]
    fn diagnose_withdraw_waiting_tx_res_received_should_be_explicit() {
        let mut w = base_withdraw("W7");
        w.tx_ack_sent_at = Some(chrono::Utc::now());
        w.audit_passed_at = Some(chrono::Utc::now());
        w.raw_tx = Some("{}".to_string());
        w.tx_hash = Some("0xhash".to_string());
        w.last_broadcast_at = Some(chrono::Utc::now());
        w.transaction_time = Some(chrono::Utc::now());
        w.tx_exec_receipt_uploaded_at = Some(chrono::Utc::now());
        w.tx_res_received_at = None;
        w.tx_res_ack_sent_at = None;

        let diag = diagnose_withdraw(&w);
        assert_eq!(diag.stage, AdvancementPoint::NeedTxResAck);
        assert_eq!(diag.next_expected_fact, Some("tx_res_received_at"));
        assert!(diag.reasons.iter().any(|r| r.contains("AWM_ORDER_TRANS_RES")));
    }
}
