use std::sync::Arc;

use alloy::primitives::U256;
use wallet_api::{
    domain::api_wallet::{RawTx, Tx},
    error::service::ServiceError,
    request::api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
    testkit::adapter_factory::{
        clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
    },
};
use wallet_chain_interact::{
    Error, QueryTransactionResult, tron::protocol::account::AccountResourceDetail,
    types::ChainPrivateKey,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;

#[derive(Clone)]
struct TronWithdrawResourceGateAdapter {
    available_energy: i64,
    available_bandwidth: i64,
}

impl TronWithdrawResourceGateAdapter {
    fn partial_energy() -> Self {
        Self { available_energy: 10_000, available_bandwidth: 0 }
    }
}

#[async_trait::async_trait]
impl Tx for TronWithdrawResourceGateAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<AccountResourceDetail, ServiceError> {
        Ok(AccountResourceDetail {
            free_net_used: 0,
            free_net_limit: self.available_bandwidth,
            net_used: 0,
            net_limit: 0,
            total_net_limit: 0,
            total_net_weight: 0,
            energy_used: 0,
            energy_limit: self.available_energy,
            total_energy_limit: 7_360,
            total_energy_weight: 100,
            tron_power_used: 0,
            tron_power_limit: 0,
        })
    }

    async fn balance_token_key(&self, _addr: &str, _token: AssetTokenKey) -> Result<U256, Error> {
        Ok(U256::ZERO)
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, Error> {
        Ok(0)
    }

    async fn query_tx_res(&self, _hash: &str) -> Result<Option<QueryTransactionResult>, Error> {
        Ok(None)
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, Error> {
        Ok("USDT".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, Error> {
        Ok("Tether USD".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, Error> {
        Ok(6)
    }

    async fn black_address(&self, _token: &str, _owner: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        _params: &ApiTransferReq,
        _private_key: ChainPrivateKey,
    ) -> Result<wallet_api::domain::chain::TransferResp, ServiceError> {
        unimplemented!()
    }

    async fn estimate_fee(
        &self,
        _req: ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, ServiceError> {
        Ok(r#"{"estimateFee":{"amount":"1.8027","currency":"TRX","unitPrice":0.0,"fiatValue":0.0},"energy":14650,"userEnergy":10000,"energyPrice":0.0,"bandwidth":345,"userBandwidth":0,"bandwidthPrice":0.0,"extraFee":0.0}"#.to_string())
    }

    async fn build_transfer_raw(
        &self,
        _params: &ApiTransferReq,
        _private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), ServiceError> {
        unimplemented!()
    }

    async fn broadcast_transfer(
        &self,
        _raw: RawTx,
    ) -> Result<wallet_api::domain::chain::TransferResp, ServiceError> {
        unimplemented!()
    }
}

pub(crate) struct AdapterGuard;

impl Drop for AdapterGuard {
    fn drop(&mut self) {
        clear_test_transaction_adapter_override("tron");
    }
}

pub(crate) fn install_tron_withdraw_resource_gate_adapter() -> AdapterGuard {
    let adapter: Arc<dyn Tx + Send + Sync> =
        Arc::new(TronWithdrawResourceGateAdapter::partial_energy());
    set_test_transaction_adapter_override("tron", adapter);
    AdapterGuard
}
