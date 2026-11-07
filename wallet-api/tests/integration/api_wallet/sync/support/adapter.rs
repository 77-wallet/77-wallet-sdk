use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use alloy::primitives::U256;
use wallet_api::{
    domain::api_wallet::Tx,
    error::service::ServiceError,
    testkit::adapter_factory::{
        clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
    },
};
use wallet_chain_interact::{Error as ChainError, QueryTransactionResult};
use wallet_database::entities::asset_token_key::AssetTokenKey;

use super::fixtures::CHAIN_CODE;

pub(crate) struct InstalledBalanceAdapter {
    calls: Arc<AtomicUsize>,
    _guard: AdapterGuard,
}

impl InstalledBalanceAdapter {
    pub(crate) fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

pub(crate) fn install_balance_adapter(balance: U256, fail: bool) -> InstalledBalanceAdapter {
    let adapter = MockBalanceAdapter::new(balance, fail);
    let calls = adapter.calls.clone();
    let guard = install_adapter(CHAIN_CODE, adapter);

    InstalledBalanceAdapter { calls, _guard: guard }
}

#[derive(Clone)]
struct MockBalanceAdapter {
    balance: U256,
    fail: bool,
    calls: Arc<AtomicUsize>,
}

impl MockBalanceAdapter {
    fn new(balance: U256, fail: bool) -> Self {
        Self { balance, fail, calls: Arc::new(AtomicUsize::new(0)) }
    }
}

#[async_trait::async_trait]
impl Tx for MockBalanceAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<wallet_chain_interact::tron::protocol::account::AccountResourceDetail, ServiceError>
    {
        unimplemented!()
    }

    async fn balance_token_key(
        &self,
        _addr: &str,
        _token: AssetTokenKey,
    ) -> Result<U256, ChainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(ChainError::TransportError(wallet_transport::errors::TransportError::EmptyResult))
        } else {
            Ok(self.balance)
        }
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, ChainError> {
        Ok(0)
    }

    async fn query_tx_res(
        &self,
        _hash: &str,
    ) -> Result<Option<QueryTransactionResult>, ChainError> {
        Ok(None)
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, ChainError> {
        Ok("BNB".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, ChainError> {
        Ok("BNB Smart Chain".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, ChainError> {
        Ok(18)
    }

    async fn black_address(&self, _token: &str, _owner: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<wallet_api::domain::chain::TransferResp, ServiceError> {
        unimplemented!()
    }

    async fn estimate_fee(
        &self,
        _req: wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, ServiceError> {
        Ok("0".to_string())
    }

    async fn build_transfer_raw(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<(String, wallet_api::domain::api_wallet::RawTx, String), ServiceError> {
        unimplemented!()
    }

    async fn broadcast_transfer(
        &self,
        _raw: wallet_api::domain::api_wallet::RawTx,
    ) -> Result<wallet_api::domain::chain::TransferResp, ServiceError> {
        unimplemented!()
    }
}

struct AdapterGuard {
    chain_code: String,
}

impl Drop for AdapterGuard {
    fn drop(&mut self) {
        clear_test_transaction_adapter_override(&self.chain_code);
    }
}

fn install_adapter(chain_code: &str, adapter: MockBalanceAdapter) -> AdapterGuard {
    let adapter: Arc<dyn Tx + Send + Sync> = Arc::new(adapter);
    set_test_transaction_adapter_override(chain_code, adapter);
    AdapterGuard { chain_code: chain_code.to_string() }
}
