#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdvancementPoint {
    NeedTxAck,
    CanBuild,
    CanBroadcast,
    NeedRecover,
    NeedTxExecReceiptUpload,
    NeedTxResAck,
    FullyBlocked,
}

/// 推进点顺序常量
/// - 顺序与 scan_round 完全一致
/// - try_advance 必须使用此常量，确保行为一致性
pub const ADVANCEMENT_ORDER: &[AdvancementPoint] = &[
    AdvancementPoint::NeedTxAck,
    AdvancementPoint::CanBuild,
    AdvancementPoint::CanBroadcast,
    AdvancementPoint::NeedRecover,
    AdvancementPoint::NeedTxExecReceiptUpload,
    AdvancementPoint::NeedTxResAck,
];

pub trait StageQueryBuilder {
    fn sql_filter(point: AdvancementPoint) -> String;
    fn rust_predicate(
        point: AdvancementPoint,
    ) -> fn(&wallet_database::entities::api_fee::ApiFeeEntity) -> bool;
}

pub struct DefaultStageQueryBuilder;

impl StageQueryBuilder for DefaultStageQueryBuilder {
    fn sql_filter(point: AdvancementPoint) -> String {
        match point {
            AdvancementPoint::NeedTxAck => "tx_ack_sent_at IS NULL".to_string(),
            AdvancementPoint::CanBuild => {
                "tx_ack_sent_at IS NOT NULL AND raw_tx IS NULL AND transaction_time IS NULL AND finished_at IS NULL".to_string()
            }
            AdvancementPoint::CanBroadcast => {
                "raw_tx IS NOT NULL AND last_broadcast_at IS NULL AND transaction_time IS NULL AND finished_at IS NULL AND (chain_code NOT IN ('bnb','eth','polygon','avalanche','fantom','arbitrum','base','scroll','sol','tron') OR broadcast_uncertain_since_at IS NULL)".to_string()
            }
            AdvancementPoint::NeedRecover => {
                "tx_hash IS NOT NULL AND transaction_time IS NULL AND tx_exec_receipt_uploaded_at IS NULL AND finished_at IS NULL AND err_code IS NULL".to_string()
            }
            AdvancementPoint::NeedTxExecReceiptUpload => {
                "tx_exec_receipt_uploaded_at IS NULL AND finished_at IS NULL AND (err_code IS NOT NULL OR transaction_time IS NOT NULL)".to_string()
            }
            AdvancementPoint::NeedTxResAck => {
                "transaction_time IS NOT NULL AND tx_res_ack_sent_at IS NULL AND finished_at IS NULL".to_string()
            }
            AdvancementPoint::FullyBlocked => "".to_string(),
        }
    }

    fn rust_predicate(
        point: AdvancementPoint,
    ) -> fn(&wallet_database::entities::api_fee::ApiFeeEntity) -> bool {
        match point {
            AdvancementPoint::NeedTxAck => |fee| fee.tx_ack_sent_at.is_none(),
            AdvancementPoint::CanBuild => |fee| {
                fee.tx_ack_sent_at.is_some()
                    && fee.raw_tx.is_none()
                    && fee.transaction_time.is_none()
                    && fee.finished_at.is_none()
            },
            AdvancementPoint::CanBroadcast => |fee| {
                fee.raw_tx.is_some()
                    && fee.last_broadcast_at.is_none()
                    && fee.transaction_time.is_none()
                    && fee.finished_at.is_none()
                    && fee.err_code.is_none()
                    && (!crate::domain::api_wallet::trans::shared_mechanism::tracks_broadcast_uncertain_state(
                        &fee.chain_code,
                    )
                        || fee.broadcast_uncertain_since_at.is_none())
            },
            AdvancementPoint::NeedRecover => |fee| {
                fee.tx_hash.is_some()
                    && fee.transaction_time.is_none()
                    && fee.tx_exec_receipt_uploaded_at.is_none()
                    && fee.finished_at.is_none()
                    && fee.err_code.is_none()
            },
            AdvancementPoint::NeedTxExecReceiptUpload => |fee| {
                fee.tx_exec_receipt_uploaded_at.is_none()
                    && fee.finished_at.is_none()
                    && (fee.err_code.is_some() || fee.transaction_time.is_some())
            },
            AdvancementPoint::NeedTxResAck => |fee| {
                fee.transaction_time.is_some()
                    && fee.tx_res_ack_sent_at.is_none()
                    && fee.finished_at.is_none()
            },
            AdvancementPoint::FullyBlocked => |_| false,
        }
    }
}

impl AdvancementPoint {
    pub fn base_severity(&self) -> u8 {
        match self {
            AdvancementPoint::NeedTxAck => 1,
            AdvancementPoint::CanBuild => 0,
            AdvancementPoint::CanBroadcast => 0,
            AdvancementPoint::NeedRecover => 2,
            AdvancementPoint::NeedTxExecReceiptUpload => 1,
            AdvancementPoint::NeedTxResAck => 1,
            AdvancementPoint::FullyBlocked => 4,
        }
    }

    pub fn wait_threshold_minutes(&self) -> i64 {
        match self {
            AdvancementPoint::NeedTxAck => 2,
            AdvancementPoint::CanBuild => 5,
            AdvancementPoint::CanBroadcast => 5,
            AdvancementPoint::NeedRecover => 10,
            AdvancementPoint::NeedTxExecReceiptUpload => 15,
            AdvancementPoint::NeedTxResAck => 8,
            AdvancementPoint::FullyBlocked => 1,
        }
    }

    pub fn next_expected_fact(&self) -> &'static str {
        match self {
            AdvancementPoint::NeedTxAck => "tx_ack_sent_at",
            AdvancementPoint::CanBuild => "raw_tx",
            AdvancementPoint::CanBroadcast => "last_broadcast_at",
            AdvancementPoint::NeedRecover => "transaction_time",
            AdvancementPoint::NeedTxExecReceiptUpload => "tx_exec_receipt_uploaded_at",
            AdvancementPoint::NeedTxResAck => "tx_res_ack_sent_at",
            AdvancementPoint::FullyBlocked => "finished_at",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{api_fee::ApiFeeStatus, asset_token_key::AssetTokenKey};

    fn base_fee() -> wallet_database::entities::api_fee::ApiFeeEntity {
        wallet_database::entities::api_fee::ApiFeeEntity {
            id: 1,
            name: "n".to_string(),
            uid: "u".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "0".to_string(),
            validate: "v".to_string(),
            chain_code: "tron".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "s".to_string(),
            trade_no: "F_STAGE_TEST".to_string(),
            trade_type: 3,
            status: ApiFeeStatus::Init,
            nonce: 0,
            tx_hash: Some("h".to_string()),
            raw_tx: Some("{}".to_string()),
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            transaction_time: None,
            block_height: Some("0".to_string()),
            notes: Some("".to_string()),
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: Some("".to_string()),
            tx_ack_sent_at: Some(Utc::now()),
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: Some(Utc::now()),
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            finished_at: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
        }
    }

    #[test]
    fn can_build_sql_rejects_committed_or_finished_rows() {
        let sql = DefaultStageQueryBuilder::sql_filter(AdvancementPoint::CanBuild);
        assert!(sql.contains("transaction_time IS NULL"));
        assert!(sql.contains("finished_at IS NULL"));
    }

    #[test]
    fn can_build_rejects_committed_or_finished_rows() {
        let mut committed = base_fee();
        committed.raw_tx = None;
        committed.transaction_time = Some(Utc::now());

        let mut finished = base_fee();
        finished.raw_tx = None;
        finished.finished_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(AdvancementPoint::CanBuild);
        assert!(!pred(&committed));
        assert!(!pred(&finished));
    }

    #[test]
    fn can_broadcast_blocks_added_evm_uncertain_state() {
        let mut blocked = base_fee();
        blocked.chain_code = "polygon".to_string();
        blocked.broadcast_uncertain_since_at = Some(Utc::now());

        let pred = DefaultStageQueryBuilder::rust_predicate(AdvancementPoint::CanBroadcast);
        assert!(!pred(&blocked));
    }

    #[test]
    fn can_broadcast_sql_tracks_added_evm_chains() {
        let sql = DefaultStageQueryBuilder::sql_filter(AdvancementPoint::CanBroadcast);

        for chain_code in ["polygon", "avalanche", "fantom", "arbitrum", "base", "scroll"] {
            assert!(sql.contains(&format!("'{chain_code}'")));
        }
        assert!(sql.contains("broadcast_uncertain_since_at IS NULL"));
    }
}
