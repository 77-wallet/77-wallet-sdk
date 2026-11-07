use crate::harness::next_unique_id;

const TRON_RECOVER_TX_HASH: &str =
    "6f2f3e7f5dbe46e7b8ff8d3c9b62df9b2b7b6f3e3c9d4a1d2f5d8e9f0a1b2c3d4";
const TRON_BACKFILL_TX_HASH: &str =
    "6f2f3e7f5dbe46e7b8ff8d3c9b62df9b2b7b6f3e3c9d4a1d2f5d8e9f0a1b2c3d5";

pub(crate) struct CollectRecoveryFixture {
    pub(crate) trade_no: String,
    pub(crate) tx_hash: String,
}

impl CollectRecoveryFixture {
    pub(crate) fn blockhash_rebuild() -> Self {
        Self {
            trade_no: format!("T_collect_blockhash_rebuild_refresh_{}", next_unique_id()),
            tx_hash: "old-hash".to_string(),
        }
    }

    pub(crate) fn expired_tron_raw_probe() -> Self {
        Self {
            trade_no: format!("C_collect_recover_expired_raw_probe_{}", next_unique_id()),
            tx_hash: TRON_RECOVER_TX_HASH.to_string(),
        }
    }

    pub(crate) fn tron_backfill() -> Self {
        Self {
            trade_no: format!("C_collect_recover_backfill_{}", next_unique_id()),
            tx_hash: TRON_BACKFILL_TX_HASH.to_string(),
        }
    }

    pub(crate) fn broadcast_visible_pending() -> Self {
        Self {
            trade_no: format!("T_collect_recover_{}", next_unique_id()),
            tx_hash: "0xrecover".to_string(),
        }
    }
}
