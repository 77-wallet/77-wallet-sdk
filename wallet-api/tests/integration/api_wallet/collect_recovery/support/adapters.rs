use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use alloy::primitives::U256;
use serde_json::json;
use wallet_api::{
    domain::api_wallet::{RawTx, Tx},
    testkit::adapter_factory::{
        clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
    },
};
use wallet_chain_interact::{
    BillResourceConsume, QueryTransactionResult, tron::operations::RawTransactionParams,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_types::chain::chain::ChainCode;

#[derive(Clone)]
struct CollectTronRecoverProbeAdapter {
    query_count: Arc<AtomicUsize>,
    query_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    tx_hash: String,
    transaction_fee: f64,
    resource_consume: String,
    transaction_time_ms: u128,
    block_height: u128,
    missing_receipt: bool,
    chain_nonce: Option<u64>,
    query_uncertain: bool,
    conflict_response: Option<bool>,
    conflict_count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Tx for CollectTronRecoverProbeAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<
        wallet_chain_interact::tron::protocol::account::AccountResourceDetail,
        wallet_api::error::service::ServiceError,
    > {
        unimplemented!("not used in collect recover probe")
    }

    async fn balance_token_key(
        &self,
        _addr: &str,
        _token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error> {
        Ok(U256::ZERO)
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, wallet_api::error::service::ServiceError> {
        self.chain_nonce.ok_or(wallet_api::error::service::ServiceError::Timeout)
    }

    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error> {
        Ok(self.block_height as u64)
    }

    async fn query_tx_res(
        &self,
        _hash: &str,
    ) -> Result<Option<QueryTransactionResult>, wallet_chain_interact::Error> {
        self.query_count.fetch_add(1, Ordering::Relaxed);
        if let Some(hook) = &self.query_hook {
            hook();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if self.query_uncertain {
            return Err(wallet_chain_interact::Error::Other(
                "simulated receipt query failure".into(),
            ));
        }
        if self.missing_receipt {
            return Ok(None);
        }
        Ok(Some(QueryTransactionResult::new(
            self.tx_hash.clone(),
            self.transaction_fee,
            self.resource_consume.clone(),
            self.transaction_time_ms,
            2,
            self.block_height,
        )))
    }

    async fn finalized_nonce_conflict(
        &self,
        chain_code: &str,
        address: &str,
        nonce: i32,
        _original_hash: &str,
    ) -> Result<
        Option<wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict>,
        wallet_api::error::service::ServiceError,
    > {
        self.conflict_count.fetch_add(1, Ordering::Relaxed);
        match self.conflict_response {
            None => Err(wallet_api::error::service::ServiceError::Timeout),
            Some(false) => Ok(None),
            Some(true) => Ok(Some(
                wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict {
                    chain_code: chain_code.to_owned(),
                    from_addr: address.trim().to_owned(),
                    nonce,
                    conflicting_tx_hash: format!("0x{}", "22".repeat(32)),
                    block_hash: format!("0x{}", "aa".repeat(32)),
                    block_number: 99,
                },
            )),
        }
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("TRX".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("Tron".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, wallet_chain_interact::Error> {
        Ok(6)
    }

    async fn black_address(
        &self,
        _token: &str,
        _owner: &str,
    ) -> Result<bool, wallet_api::error::service::ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<wallet_api::domain::chain::TransferResp, wallet_api::error::service::ServiceError>
    {
        unimplemented!("not used in collect recover probe")
    }

    async fn estimate_fee(
        &self,
        _req: wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, wallet_api::error::service::ServiceError> {
        Ok(json!({
            "estimateFee": {
                "amount": "0.1",
                "currency": "USD",
                "unitPrice": 0.0,
                "fiatValue": 0.0
            }
        })
        .to_string())
    }

    async fn build_transfer_raw(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<(String, RawTx, String), wallet_api::error::service::ServiceError> {
        unimplemented!("not used in collect recover probe")
    }

    async fn broadcast_transfer(
        &self,
        _raw: RawTx,
    ) -> Result<wallet_api::domain::chain::TransferResp, wallet_api::error::service::ServiceError>
    {
        unimplemented!("not used in collect recover probe")
    }
}

pub(crate) struct TronRecoverProbeGuard {
    chain_code: String,
}

impl Drop for TronRecoverProbeGuard {
    fn drop(&mut self) {
        clear_test_transaction_adapter_override(&self.chain_code);
    }
}

pub(crate) fn install_collect_tron_recover_probe_adapter(
    query_count: Arc<AtomicUsize>,
    query_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    tx_hash: &str,
    transaction_fee: f64,
    resource_consume: &str,
    transaction_time_ms: u128,
    block_height: u128,
) -> TronRecoverProbeGuard {
    let chain_code = ChainCode::Tron.to_string();
    let adapter = Arc::new(CollectTronRecoverProbeAdapter {
        query_count,
        query_hook,
        tx_hash: tx_hash.to_string(),
        transaction_fee,
        resource_consume: resource_consume.to_string(),
        transaction_time_ms,
        block_height,
        missing_receipt: false,
        chain_nonce: Some(0),
        query_uncertain: false,
        conflict_response: Some(false),
        conflict_count: Arc::new(AtomicUsize::new(0)),
    });
    let tx_adapter: Arc<dyn Tx + Send + Sync> = adapter;
    set_test_transaction_adapter_override(&chain_code, tx_adapter);
    TronRecoverProbeGuard { chain_code }
}

pub(crate) fn expired_tron_raw_tx_json(expiration_ms: i64) -> String {
    let raw = RawTransactionParams {
        tx_id: "expired-tron-tx".to_string(),
        raw_data: json!({
            "expiration": expiration_ms,
            "timestamp": expiration_ms.saturating_sub(1_000),
        })
        .to_string(),
        raw_data_hex: "0a00".to_string(),
        signature: vec![],
    };
    let bill = BillResourceConsume::new_tron(0, 0);
    serde_json::to_string(&RawTx::Tron(raw, bill, "0".to_string()))
        .expect("serialize expired tron raw tx")
}

// Configurable chain boundary shared by finalized conflict recovery scenarios.
pub(crate) fn install_evm_recover_probe(
    tx_hash: &str,
    missing_receipt: bool,
    conflict_response: Option<bool>,
    query_count: Arc<AtomicUsize>,
    conflict_count: Arc<AtomicUsize>,
) -> TronRecoverProbeGuard {
    install_evm_recover_probe_with_nonce(
        tx_hash,
        missing_receipt,
        conflict_response,
        query_count,
        conflict_count,
        Some(2),
        false,
    )
}

pub(crate) fn install_evm_recover_probe_with_nonce(
    tx_hash: &str,
    missing_receipt: bool,
    conflict_response: Option<bool>,
    query_count: Arc<AtomicUsize>,
    conflict_count: Arc<AtomicUsize>,
    chain_nonce: Option<u64>,
    query_uncertain: bool,
) -> TronRecoverProbeGuard {
    let chain_code = "eth".to_owned();
    let adapter = Arc::new(CollectTronRecoverProbeAdapter {
        query_count,
        query_hook: None,
        tx_hash: tx_hash.to_owned(),
        transaction_fee: 0.0001,
        resource_consume: "{}".to_owned(),
        transaction_time_ms: 1_700_000_000_000,
        block_height: 99,
        missing_receipt,
        chain_nonce,
        query_uncertain,
        conflict_response,
        conflict_count,
    });
    set_test_transaction_adapter_override(&chain_code, adapter);
    TronRecoverProbeGuard { chain_code }
}
