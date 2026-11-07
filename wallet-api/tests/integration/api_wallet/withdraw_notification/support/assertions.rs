use wallet_api::error::service::ServiceError;

pub(crate) fn then_frontend_notification_failed(result: Result<(), ServiceError>) {
    assert!(result.is_err(), "frontend notify failure should bubble up");
}

pub(crate) fn then_tx_ack_sent(result: Result<(), ServiceError>) {
    result.expect("send withdraw tx ack");
}

pub(crate) fn then_worker_left_flow_retryable(result: Result<(), ServiceError>) {
    result.expect("backend ack failure should leave the worker retryable");
}
