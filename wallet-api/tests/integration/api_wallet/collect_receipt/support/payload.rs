use chrono::Utc;
use serde_json::Value;
use wallet_api::testkit::collect::build_collect_tx_exec_receipt_payload;
use wallet_database::entities::{
    api_collect::{ApiCollectEntity, ApiCollectStatus},
    asset_token_key::AssetTokenKey,
};

pub(crate) fn base_collect_for_receipt() -> ApiCollectEntity {
    ApiCollectEntity {
        id: 1,
        name: "collect".to_string(),
        uid: "uid".to_string(),
        from_addr: "from".to_string(),
        to_addr: "persisted-to".to_string(),
        value: "1.12".to_string(),
        validate: "digest".to_string(),
        chain_code: "sol".to_string(),
        token_addr: AssetTokenKey::Contract("token".to_string()),
        symbol: "USDC".to_string(),
        trade_no: "trade-no".to_string(),
        trade_type: 2,
        risk_addr: 1,
        status: ApiCollectStatus::SendingTx,
        nonce: 0,
        tx_hash: Some("hash".to_string()),
        transaction_fee: "0".to_string(),
        transaction_time: Some(Utc::now()),
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
        last_broadcast_at: Some(Utc::now()),
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

pub(crate) fn collect_receipt_payload_json(req: &ApiCollectEntity, trade_no: &str) -> Value {
    serde_json::to_value(build_collect_tx_exec_receipt_payload(req, trade_no))
        .expect("serialize receipt payload")
}

pub(crate) fn assert_collect_receipt_payload(
    payload_json: &Value,
    trade_no: &str,
    to_addr: &str,
    tx_hash: &str,
) {
    assert_eq!(payload_json["tradeNo"], trade_no);
    assert_eq!(payload_json["to"], to_addr);
    assert_eq!(payload_json["hash"], tx_hash);
    assert_eq!(payload_json["status"], "SUCCESS");
}

pub(crate) fn assert_collect_tx_exec_receipt_uploaded(rec: &ApiCollectEntity) {
    assert!(
        rec.tx_exec_receipt_uploaded_at.is_some(),
        "collect receipt upload should mark tx_exec_receipt_uploaded_at"
    );
}
