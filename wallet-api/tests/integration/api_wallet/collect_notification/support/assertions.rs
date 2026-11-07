use wallet_api::error::service::ServiceError;

pub(crate) fn then_frontend_notification_failed(result: Result<(), ServiceError>) {
    assert!(result.is_err(), "frontend notify failure should bubble up");
}
