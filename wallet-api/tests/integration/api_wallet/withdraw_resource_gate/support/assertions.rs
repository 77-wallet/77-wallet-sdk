use std::time::{Duration, Instant};

use crate::harness::{
    decrypt_captured_api_backend_body,
    worker::{CapturedHttpRequest, MockBackendRecorder},
};

const TRANS_EVENT_ACK: &str =
    wallet_transport_backend::consts::endpoint::api_wallet::TRANS_EVENT_ACK;

pub(crate) async fn assert_event_ack_payload_exists(
    recorder: &MockBackendRecorder,
    resource_trade_no: &str,
    ack_type: &str,
    event_type: &str,
) {
    let matched =
        wait_for_event_ack_payload(recorder, resource_trade_no, ack_type, event_type).await;

    if matched {
        return;
    }

    let captured_requests = recorder.snapshot();
    let decoded_event_acks = decode_event_ack_payloads(&captured_requests);

    panic!(
        "expected event ack payload trade_no={resource_trade_no}, ack_type={ack_type}, type={event_type}; decoded event ack payloads: {decoded_event_acks:?}; captured requests: {captured_requests:?}"
    );
}

async fn wait_for_event_ack_payload(
    recorder: &MockBackendRecorder,
    resource_trade_no: &str,
    ack_type: &str,
    event_type: &str,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = recorder.snapshot().iter().any(|req| {
            req.path.contains(TRANS_EVENT_ACK) && {
                let payload = decrypt_captured_api_backend_body(&req.body);
                payload["tradeNo"].as_str() == Some(resource_trade_no)
                    && payload["ackType"].as_str() == Some(ack_type)
                    && payload["type"].as_str() == Some(event_type)
            }
        });
        if found || Instant::now() >= deadline {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn decode_event_ack_payloads(captured_requests: &[CapturedHttpRequest]) -> Vec<serde_json::Value> {
    captured_requests
        .iter()
        .filter(|req| req.path.contains(TRANS_EVENT_ACK))
        .map(|req| decrypt_captured_api_backend_body(&req.body))
        .collect()
}
