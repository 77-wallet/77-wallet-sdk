use wallet_database::entities::api_collect::ApiCollectEntity;

/// 事实掩码字段定义
pub const FACT_MASK_SCHEMA: &[(&str, u8)] = &[
    ("order_ack_sent_at", 0),
    ("raw_tx", 1),
    ("need_service_fee", 2),
    ("ever_needed_service_fee", 3),
    ("tx_fee_res_ack_sent_at", 4),
    ("last_broadcast_at", 5),
    ("transaction_time", 6),
    ("tx_exec_receipt_uploaded_at", 7),
    ("result_ack_sent_at", 8),
    ("service_fee_order_received_at", 9),
    ("service_fee_uploaded_at", 10),
    ("finished_at", 11),
    ("err_code", 12),
];

/// 生成事实快照压缩日志
/// 这是生产排查神器
pub fn dump_fact_snapshot(c: &ApiCollectEntity) -> String {
    format!(
        "ack={:?} raw={} fee={:?} ever_fee={} fee_ack={:?} \
         broadcast={:?} tx_time={:?} receipt={:?} result_ack={:?} \
         service_fee_order={:?} service_fee_up={:?} finished={:?} err={:?}",
        c.order_ack_sent_at.is_some(),
        c.raw_tx.is_some(),
        c.need_service_fee,
        c.ever_needed_service_fee,
        c.tx_fee_res_ack_sent_at.is_some(),
        c.last_broadcast_at.is_some(),
        c.transaction_time.is_some(),
        c.tx_exec_receipt_uploaded_at.is_some(),
        c.result_ack_sent_at.is_some(),
        c.service_fee_order_received_at.is_some(),
        c.service_fee_uploaded_at.is_some(),
        c.finished_at.is_some(),
        c.err_code.is_some(),
    )
}

/// 生成事实掩码（用于机器处理）
pub fn fact_mask(c: &ApiCollectEntity) -> (u64, u8) {
    const MASK_VERSION: u8 = 2;

    let mut mask = 0u64;

    // 每一位代表一个事实的存在性
    if c.order_ack_sent_at.is_some() {
        mask |= 1 << 0;
    }
    if c.raw_tx.is_some() {
        mask |= 1 << 1;
    }
    if c.need_service_fee == Some(true) {
        mask |= 1 << 2;
    }
    if c.ever_needed_service_fee {
        mask |= 1 << 3;
    }
    if c.tx_fee_res_ack_sent_at.is_some() {
        mask |= 1 << 4;
    }
    if c.last_broadcast_at.is_some() {
        mask |= 1 << 5;
    }
    if c.transaction_time.is_some() {
        mask |= 1 << 6;
    }
    if c.tx_exec_receipt_uploaded_at.is_some() {
        mask |= 1 << 7;
    }
    if c.result_ack_sent_at.is_some() {
        mask |= 1 << 8;
    }
    if c.service_fee_order_received_at.is_some() {
        mask |= 1 << 9;
    }
    if c.service_fee_uploaded_at.is_some() {
        mask |= 1 << 10;
    }
    if c.finished_at.is_some() {
        mask |= 1 << 11;
    }
    if c.err_code.is_some() {
        mask |= 1 << 12;
    }

    (mask, MASK_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_collect::{ApiCollectEntity, ApiCollectStatus, ErrCode},
        asset_token_key::AssetTokenKey,
    };

    fn bare_collect() -> ApiCollectEntity {
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
            symbol: "s".to_string(),
            trade_no: "C_MASK_TEST".to_string(),
            trade_type: 2,
            risk_addr: 0,
            status: ApiCollectStatus::Init,
            nonce: 0,
            tx_hash: None,
            transaction_fee: "0".to_string(),
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
            order_ack_sent_at: None,
            raw_tx: None,
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
            updated_at: None,
        }
    }

    #[test]
    fn fact_mask_empty_is_zero() {
        let c = bare_collect();
        let (mask, version) = fact_mask(&c);
        assert_eq!(mask, 0);
        assert_eq!(version, 2);
    }

    #[test]
    fn fact_mask_order_ack_sets_bit0() {
        let mut c = bare_collect();
        c.order_ack_sent_at = Some(Utc::now());
        let (mask, _) = fact_mask(&c);
        assert_ne!(mask & (1 << 0), 0);
    }

    #[test]
    fn fact_mask_raw_tx_sets_bit1() {
        let mut c = bare_collect();
        c.raw_tx = Some("{}".to_string());
        let (mask, _) = fact_mask(&c);
        assert_ne!(mask & (1 << 1), 0);
    }

    #[test]
    fn fact_mask_need_service_fee_only_sets_when_true() {
        let mut c = bare_collect();
        c.need_service_fee = Some(false);
        let (mask_false, _) = fact_mask(&c);
        assert_eq!(mask_false & (1 << 2), 0, "false should not set bit 2");

        c.need_service_fee = Some(true);
        let (mask_true, _) = fact_mask(&c);
        assert_ne!(mask_true & (1 << 2), 0, "true must set bit 2");
    }

    #[test]
    fn fact_mask_ever_needed_service_fee_sets_bit3() {
        let mut c = bare_collect();
        c.ever_needed_service_fee = true;
        let (mask, _) = fact_mask(&c);
        assert_ne!(mask & (1 << 3), 0);
    }

    #[test]
    fn fact_mask_finished_sets_bit11() {
        let mut c = bare_collect();
        c.finished_at = Some(Utc::now());
        let (mask, _) = fact_mask(&c);
        assert_ne!(mask & (1 << 11), 0);
    }

    #[test]
    fn fact_mask_err_code_sets_bit12() {
        let mut c = bare_collect();
        c.err_code = Some(ErrCode::UnknownError);
        let (mask, _) = fact_mask(&c);
        assert_ne!(mask & (1 << 12), 0);
    }

    #[test]
    fn fact_mask_schema_bit_positions_match_implementation() {
        // Schema declares 13 fields with indices 0..=12; verify none exceed u64 capacity.
        for (name, bit) in FACT_MASK_SCHEMA {
            assert!(*bit < 64, "field '{name}' bit position {bit} overflows u64");
        }
        // All declared bit positions must be unique.
        let mut seen = std::collections::HashSet::new();
        for (name, bit) in FACT_MASK_SCHEMA {
            assert!(seen.insert(bit), "duplicate bit position {bit} for field '{name}'");
        }
    }

    #[test]
    fn dump_fact_snapshot_reflects_set_fields() {
        let mut c = bare_collect();
        c.order_ack_sent_at = Some(Utc::now());
        c.raw_tx = Some("{}".to_string());

        let snap = dump_fact_snapshot(&c);
        assert!(snap.contains("ack=true"), "ack should be true: {snap}");
        assert!(snap.contains("raw=true"), "raw should be true: {snap}");
        assert!(snap.contains("finished=false"), "finished should be false: {snap}");
        assert!(snap.contains("err=false"), "err should be false: {snap}");
    }

    #[test]
    fn dump_fact_snapshot_reflects_empty_entity() {
        let c = bare_collect();
        let snap = dump_fact_snapshot(&c);
        assert!(snap.contains("ack=false"));
        assert!(snap.contains("raw=false"));
        assert!(snap.contains("err=false"));
    }
}
