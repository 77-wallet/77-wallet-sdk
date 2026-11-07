use crate::harness::{decrypt_captured_api_backend_body, worker::CapturedHttpRequest};

pub(crate) fn count_withdraw_tx_ack_requests(
    requests: &[CapturedHttpRequest],
    trade_no: &str,
) -> usize {
    requests
        .iter()
        .filter(|req| {
            req.path
                .contains(wallet_transport_backend::consts::endpoint::api_wallet::TRANS_EVENT_ACK)
        })
        .filter(|req| {
            let payload = decrypt_captured_api_backend_body(&req.body);
            payload["tradeNo"].as_str() == Some(trade_no)
                && payload["ackType"].as_str() == Some("TX")
                && payload["type"].as_str() == Some("WD")
        })
        .count()
}
