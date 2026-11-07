use wallet_database::entities::api_fee::ApiFeeEntity;

/// 事实掩码字段定义
pub const FACT_MASK_SCHEMA: &[(&str, u8)] = &[
    ("tx_ack_sent_at", 0),
    ("raw_tx", 1),
    ("last_broadcast_at", 2),
    ("tx_hash", 3),
    ("transaction_time", 4),
    ("tx_exec_receipt_uploaded_at", 5),
    ("tx_res_ack_sent_at", 6),
    ("finished_at", 7),
    ("err_code", 8),
];

/// 生成事实快照压缩日志
pub fn dump_fact_snapshot(f: &ApiFeeEntity) -> String {
    format!(
        "ack={} raw={} broadcast={} hash={} tx_time={} receipt={} result_ack={} finished={} err={}",
        f.tx_ack_sent_at.is_some(),
        f.raw_tx.is_some(),
        f.last_broadcast_at.is_some(),
        f.tx_hash.is_some(),
        f.transaction_time.is_some(),
        f.tx_exec_receipt_uploaded_at.is_some(),
        f.tx_res_ack_sent_at.is_some(),
        f.finished_at.is_some(),
        f.err_code.is_some(),
    )
}

/// 生成事实掩码（用于机器处理）
pub fn fact_mask(f: &ApiFeeEntity) -> (u64, u8) {
    const MASK_VERSION: u8 = 1;
    let mut mask = 0u64;

    if f.tx_ack_sent_at.is_some() {
        mask |= 1 << 0;
    }
    if f.raw_tx.is_some() {
        mask |= 1 << 1;
    }
    if f.last_broadcast_at.is_some() {
        mask |= 1 << 2;
    }
    if f.tx_hash.is_some() {
        mask |= 1 << 3;
    }
    if f.transaction_time.is_some() {
        mask |= 1 << 4;
    }
    if f.tx_exec_receipt_uploaded_at.is_some() {
        mask |= 1 << 5;
    }
    if f.tx_res_ack_sent_at.is_some() {
        mask |= 1 << 6;
    }
    if f.finished_at.is_some() {
        mask |= 1 << 7;
    }
    if f.err_code.is_some() {
        mask |= 1 << 8;
    }

    (mask, MASK_VERSION)
}
