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
pub(crate) struct TronResourceGateAdapter {
    available_energy: i64,
    available_bandwidth: i64,
    native_balance: U256,
    funding_addr: Option<String>,
    funding_balance: U256,
}

impl TronResourceGateAdapter {
    pub(crate) fn near_ready() -> Self {
        Self {
            available_energy: 14_649,
            available_bandwidth: 0,
            native_balance: U256::from(10_000_000u64),
            funding_addr: None,
            funding_balance: U256::ZERO,
        }
    }

    pub(crate) fn insufficient_funding(funding_addr: &str) -> Self {
        Self {
            available_energy: 14_649,
            available_bandwidth: 0,
            native_balance: U256::ZERO,
            funding_addr: Some(funding_addr.to_string()),
            funding_balance: U256::from(1_000_000u64),
        }
    }
}

#[async_trait::async_trait]
impl Tx for TronResourceGateAdapter {
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

    async fn balance_token_key(&self, addr: &str, token: AssetTokenKey) -> Result<U256, Error> {
        if token.is_contract() {
            return Ok(U256::from(10_000_000u64));
        }
        if self.funding_addr.as_deref() == Some(addr) {
            return Ok(self.funding_balance);
        }
        Ok(self.native_balance)
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
        Ok(r#"{"estimateFee":{"amount":"1.8027","currency":"TRX","unitPrice":0.0,"fiatValue":0.0},"energy":14650,"userEnergy":14649,"energyPrice":0.0,"bandwidth":345,"userBandwidth":0,"bandwidthPrice":0.001,"extraFee":0.0}"#.to_string())
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

pub(crate) fn install_tron_resource_gate_adapter() -> AdapterGuard {
    let adapter: Arc<dyn Tx + Send + Sync> = Arc::new(TronResourceGateAdapter::near_ready());
    set_test_transaction_adapter_override("tron", adapter);
    AdapterGuard
}

pub(crate) fn install_tron_insufficient_fee_funding_adapter(funding_addr: &str) -> AdapterGuard {
    let adapter: Arc<dyn Tx + Send + Sync> =
        Arc::new(TronResourceGateAdapter::insufficient_funding(funding_addr));
    set_test_transaction_adapter_override("tron", adapter);
    AdapterGuard
}
