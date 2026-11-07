use alloy::primitives::U256;
use serde_json::Value;

use crate::harness::{GivenRole, ThenRole, WhenRole, WorkerTestEnv, ensure_worker_env};

use super::super::super::support::{
    ServiceFeeUploadScenario, given_eth_service_fee_upload_waiting,
    given_sol_service_fee_upload_waiting, install_collect_eth_test_adapter,
    install_collect_test_adapter_fee_shortage, then_service_fee_upload_payload,
    when_upload_collect_service_fee,
};

pub(crate) struct CollectServiceFeeUploadScenario {
    env: &'static WorkerTestEnv,
}

impl CollectServiceFeeUploadScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();
        Self { env }
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectServiceFeeUploadGiven {
    fn sol_fee_shortage_adapter(&self, recipient_missing: bool) -> impl Drop;

    fn eth_fee_adapter(&self, balance_wei: u128, fee_amount: f64) -> impl Drop;

    async fn sol_service_fee_upload_waiting(&self, trade_prefix: &str) -> ServiceFeeUploadScenario;

    async fn eth_service_fee_upload_waiting(&self) -> ServiceFeeUploadScenario;
}

#[async_trait::async_trait(?Send)]
impl CollectServiceFeeUploadGiven for GivenRole<'_, CollectServiceFeeUploadScenario> {
    fn sol_fee_shortage_adapter(&self, recipient_missing: bool) -> impl Drop {
        install_collect_test_adapter_fee_shortage(recipient_missing, 0)
    }

    fn eth_fee_adapter(&self, balance_wei: u128, fee_amount: f64) -> impl Drop {
        install_collect_eth_test_adapter(U256::from(balance_wei), fee_amount)
    }

    async fn sol_service_fee_upload_waiting(&self, trade_prefix: &str) -> ServiceFeeUploadScenario {
        given_sol_service_fee_upload_waiting(self.scenario().env, trade_prefix).await
    }

    async fn eth_service_fee_upload_waiting(&self) -> ServiceFeeUploadScenario {
        given_eth_service_fee_upload_waiting(self.scenario().env).await
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectServiceFeeUploadWhen {
    async fn service_fee_is_uploaded(&self, upload: &ServiceFeeUploadScenario, expect_msg: &str);
}

#[async_trait::async_trait(?Send)]
impl CollectServiceFeeUploadWhen for WhenRole<'_, CollectServiceFeeUploadScenario> {
    async fn service_fee_is_uploaded(&self, upload: &ServiceFeeUploadScenario, expect_msg: &str) {
        when_upload_collect_service_fee(upload, expect_msg).await;
    }
}

pub(crate) trait CollectServiceFeeUploadThen {
    fn payload_routes_reverse_transfer(&self, upload: &ServiceFeeUploadScenario);

    fn payload_trade_no_matches(&self, upload: &ServiceFeeUploadScenario);

    fn payload_amount_is(&self, upload: &ServiceFeeUploadScenario, expected: f64, reason: &str);
}

impl CollectServiceFeeUploadThen for ThenRole<'_, CollectServiceFeeUploadScenario> {
    fn payload_routes_reverse_transfer(&self, upload: &ServiceFeeUploadScenario) {
        let payload = payload(self, upload);
        assert_eq!(payload["tradeNo"].as_str(), Some(upload.trade_no.as_str()));
        assert_eq!(payload["from"].as_str(), Some(upload.to_addr.as_str()));
        assert_eq!(payload["to"].as_str(), Some(upload.from_addr.as_str()));
        assert_eq!(payload["tokenCode"].as_str(), Some(upload.token_code));
        assert_eq!(payload["contractAddress"].as_str(), Some(upload.contract_address));
    }

    fn payload_trade_no_matches(&self, upload: &ServiceFeeUploadScenario) {
        let payload = payload(self, upload);
        assert_eq!(payload["tradeNo"].as_str(), Some(upload.trade_no.as_str()));
    }

    fn payload_amount_is(&self, upload: &ServiceFeeUploadScenario, expected: f64, reason: &str) {
        let payload = payload(self, upload);
        let amount = payload["amount"].as_f64().unwrap_or_default();
        assert!(
            (amount - expected).abs() < 1e-12,
            "service fee upload must use {reason}, got {amount}"
        );
    }
}

fn payload(
    then: &ThenRole<'_, CollectServiceFeeUploadScenario>,
    upload: &ServiceFeeUploadScenario,
) -> Value {
    then_service_fee_upload_payload(then.scenario().env, &upload.trade_no)
}
