use wallet_database::entities::api_withdraw::ApiWithdrawEntity;

const MASK_VERSION: u8 = 1;

// Bit positions
const BIT_ACK: u64 = 1 << 0;
const BIT_AUDIT: u64 = 1 << 1;
const BIT_RAW: u64 = 1 << 2;
const BIT_BROADCAST: u64 = 1 << 3;
const BIT_TX_HASH: u64 = 1 << 4;
const BIT_TX_TIME: u64 = 1 << 5;
const BIT_RECEIPT: u64 = 1 << 6;
const BIT_RES_ACK: u64 = 1 << 7;
const BIT_FINISHED: u64 = 1 << 8;
const BIT_ERR: u64 = 1 << 9;

pub fn fact_mask(withdraw: &ApiWithdrawEntity) -> (u64, u8) {
    let mut mask = 0u64;

    if withdraw.tx_ack_sent_at.is_some() {
        mask |= BIT_ACK;
    }
    if withdraw.audit_passed_at.is_some() {
        mask |= BIT_AUDIT;
    }
    if withdraw.raw_tx.is_some() {
        mask |= BIT_RAW;
    }
    if withdraw.last_broadcast_at.is_some() {
        mask |= BIT_BROADCAST;
    }
    if withdraw.tx_hash.is_some() {
        mask |= BIT_TX_HASH;
    }
    if withdraw.transaction_time.is_some() {
        mask |= BIT_TX_TIME;
    }
    if withdraw.tx_exec_receipt_uploaded_at.is_some() {
        mask |= BIT_RECEIPT;
    }
    if withdraw.tx_res_ack_sent_at.is_some() {
        mask |= BIT_RES_ACK;
    }
    if withdraw.finished_at.is_some() {
        mask |= BIT_FINISHED;
    }
    if withdraw.err_code.is_some() {
        mask |= BIT_ERR;
    }

    (mask, MASK_VERSION)
}

pub fn dump_fact_snapshot(withdraw: &ApiWithdrawEntity) -> String {
    format!(
        "ack={} audit={} raw={} broadcast={} tx_hash={} tx_time={} receipt={} res_ack={} finished={} err={}",
        withdraw.tx_ack_sent_at.is_some(),
        withdraw.audit_passed_at.is_some(),
        withdraw.raw_tx.is_some(),
        withdraw.last_broadcast_at.is_some(),
        withdraw.tx_hash.is_some(),
        withdraw.transaction_time.is_some(),
        withdraw.tx_exec_receipt_uploaded_at.is_some(),
        withdraw.tx_res_ack_sent_at.is_some(),
        withdraw.finished_at.is_some(),
        withdraw.err_code.is_some(),
    )
}
