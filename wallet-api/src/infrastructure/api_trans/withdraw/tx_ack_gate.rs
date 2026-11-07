use dashmap::DashSet;
use once_cell::sync::Lazy;

static TX_ACK_IN_FLIGHT: Lazy<DashSet<String>> = Lazy::new(DashSet::new);

pub(crate) struct TxAckInFlightGuard {
    trade_no: String,
}

impl Drop for TxAckInFlightGuard {
    fn drop(&mut self) {
        TX_ACK_IN_FLIGHT.remove(&self.trade_no);
    }
}

pub(crate) fn try_acquire_tx_ack_gate(trade_no: &str) -> Option<TxAckInFlightGuard> {
    let trade_no = trade_no.to_string();
    if TX_ACK_IN_FLIGHT.insert(trade_no.clone()) {
        Some(TxAckInFlightGuard { trade_no })
    } else {
        None
    }
}
