use std::fmt;

use wallet_database::entities::api_fee::ApiFeeEntity;

use super::fact_snapshot::{dump_fact_snapshot, fact_mask};
use crate::infrastructure::api_trans::collect_fee::shadow::{
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

pub fn diagnose_fee(fee: &ApiFeeEntity) -> DiagnoseResult {
    for (index, point) in ADVANCEMENT_ORDER.iter().enumerate() {
        let eval = evaluate_point(*point, fee);
        if eval.can_advance {
            if *point == AdvancementPoint::NeedTxExecReceiptUpload
                && is_tx_exec_receipt_success_missing_hash_blocked(fee)
            {
                return DiagnoseResult {
                    stage: *point,
                    reasons: vec![
                        "TxExecReceipt blocked: success payload missing tx_hash (waiting tx_hash backfill)"
                            .to_string(),
                    ],
                    facts_snapshot: dump_fact_snapshot(fee),
                    facts_mask: fact_mask(fee),
                    stuck_score: calculate_severity(*point, fee),
                    stage_index: index as u8,
                    next_expected_fact: Some("tx_hash"),
                };
            }

            return DiagnoseResult {
                stage: *point,
                reasons: eval.reasons.into_iter().map(|r| r.message).collect(),
                facts_snapshot: dump_fact_snapshot(fee),
                facts_mask: fact_mask(fee),
                stuck_score: calculate_severity(*point, fee),
                stage_index: index as u8,
                next_expected_fact: Some(point.next_expected_fact()),
            };
        }
    }

    DiagnoseResult {
        stage: AdvancementPoint::FullyBlocked,
        reasons: vec!["No advancement possible".to_string()],
        facts_snapshot: dump_fact_snapshot(fee),
        facts_mask: fact_mask(fee),
        stuck_score: calculate_severity(AdvancementPoint::FullyBlocked, fee),
        stage_index: ADVANCEMENT_ORDER.len() as u8,
        next_expected_fact: None,
    }
}

fn is_tx_exec_receipt_success_missing_hash_blocked(fee: &ApiFeeEntity) -> bool {
    let tx_hash_missing = fee.tx_hash.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
    let has_success_execution_evidence = fee.err_code.is_none() && fee.transaction_time.is_some();

    fee.tx_exec_receipt_uploaded_at.is_none() && has_success_execution_evidence && tx_hash_missing
}

fn calculate_severity(stage: AdvancementPoint, fee: &ApiFeeEntity) -> u8 {
    let base = stage.base_severity();
    let wait = calculate_wait_weight(stage, fee);
    std::cmp::min(base + wait, 4)
}

fn calculate_wait_weight(stage: AdvancementPoint, fee: &ApiFeeEntity) -> u8 {
    let now = chrono::Utc::now();
    let wait_minutes = (now - fee.created_at).num_minutes();
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
        api_fee::{ApiFeeEntity, ApiFeeStatus},
        asset_token_key::AssetTokenKey,
    };

    fn base_fee(trade_no: &str) -> ApiFeeEntity {
        ApiFeeEntity {
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
            trade_type: 0,
            status: ApiFeeStatus::Init,
            nonce: 0,
            tx_hash: None,
            raw_tx: None,
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
            tx_ack_sent_at: None,
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: None,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            finished_at: None,
            created_at: chrono::Utc::now() - chrono::Duration::minutes(30),
            updated_at: Some(chrono::Utc::now()),
        }
    }

    #[test]
    fn diagnose_stage_need_tx_ack_when_missing_ack() {
        let fee = base_fee("F1");
        let diag = diagnose_fee(&fee);
        assert_eq!(diag.stage, AdvancementPoint::NeedTxAck);
    }

    #[test]
    fn fact_mask_changes_when_ack_written() {
        let mut fee = base_fee("F2");
        let (m1, v1) = fact_mask(&fee);
        fee.tx_ack_sent_at = Some(chrono::Utc::now());
        let (m2, v2) = fact_mask(&fee);
        assert_eq!(v1, v2);
        assert_ne!(m1, m2);
    }

    #[test]
    fn diagnose_tx_exec_receipt_missing_hash_blocked_reason() {
        let mut fee = base_fee("F3");
        fee.tx_ack_sent_at = Some(chrono::Utc::now());
        fee.raw_tx = Some("{}".to_string());
        fee.transaction_time = Some(chrono::Utc::now());
        fee.tx_hash = Some(String::new());

        let diag = diagnose_fee(&fee);
        assert_eq!(diag.stage, AdvancementPoint::NeedTxExecReceiptUpload);
        assert!(
            diag.reasons
                .iter()
                .any(|r| r.contains("TxExecReceipt blocked: success payload missing tx_hash"))
        );
        assert_eq!(diag.next_expected_fact, Some("tx_hash"));
    }

    #[test]
    fn diagnose_broadcast_visible_pending_routes_to_recover() {
        let mut fee = base_fee("F5");
        fee.tx_ack_sent_at = Some(chrono::Utc::now());
        fee.raw_tx = Some("{}".to_string());
        fee.last_broadcast_at = Some(chrono::Utc::now());
        fee.tx_hash = Some("0xabc".to_string());
        fee.tx_exec_receipt_uploaded_at = None;

        let diag = diagnose_fee(&fee);
        assert_eq!(diag.stage, AdvancementPoint::NeedRecover);
    }

    #[test]
    fn diagnose_tx_exec_receipt_fail_path_not_blocked_by_missing_hash_reason() {
        let mut fee = base_fee("F4");
        fee.tx_ack_sent_at = Some(chrono::Utc::now());
        fee.raw_tx = Some("{}".to_string());
        fee.last_broadcast_at = Some(chrono::Utc::now());
        fee.tx_hash = Some(String::new());
        fee.err_code = Some(wallet_database::entities::api_fee::ErrCode::UnknownError);

        let diag = diagnose_fee(&fee);
        assert!(
            !diag.reasons.iter().any(|r| r.contains("waiting tx_hash backfill")),
            "fail path should not be frozen by missing tx_hash"
        );
    }
}
