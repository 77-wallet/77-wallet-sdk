use std::sync::Arc;

use alloy::primitives::U256;
use serde_json::json;
use wallet_api::{
    domain::{
        api_wallet::{RawTx, Tx},
        chain::adapter::sol_tx::TOKEN_ACCOUNT_RENT,
    },
    error::business::{
        BusinessError,
        chain::{ChainError, InsufficientBalanceDetail},
    },
    testkit::adapter_factory::{
        clear_test_transaction_adapter_override, set_test_transaction_adapter_override,
    },
};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_types::chain::chain::ChainCode;

#[derive(Clone)]
struct CollectSolTestAdapter {
    recipient_missing: bool,
    force_fee_insufficient: bool,
    balance: u64,
    fee: f64,
}

#[async_trait::async_trait]
impl Tx for CollectSolTestAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<
        wallet_chain_interact::tron::protocol::account::AccountResourceDetail,
        wallet_api::error::service::ServiceError,
    > {
        unimplemented!("not used in collect fee checks")
    }

    async fn balance_token_key(
        &self,
        _addr: &str,
        _token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error> {
        Ok(U256::from(self.balance))
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, wallet_api::error::service::ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error> {
        Ok(0)
    }

    async fn query_tx_res(
        &self,
        _hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, wallet_chain_interact::Error>
    {
        Ok(None)
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("SOL".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("Solana".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, wallet_chain_interact::Error> {
        Ok(9)
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
        unimplemented!("not used in collect fee checks")
    }

    async fn estimate_fee(
        &self,
        req: wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, wallet_api::error::service::ServiceError> {
        if self.force_fee_insufficient {
            return Err(wallet_api::error::service::ServiceError::Business(BusinessError::Chain(
                ChainError::InsufficientFeeBalance,
            )));
        }

        if self.recipient_missing {
            return Err(wallet_api::error::service::ServiceError::Business(
                BusinessError::Chain(ChainError::insufficient_balance_with_detail(
                    InsufficientBalanceDetail::new()
                        .from_addr(req.from)
                        .to_addr(req.to)
                        .chain_code("sol".to_string())
                        .value(req.value)
                        .balance(self.balance.to_string())
                        .need("990880".to_string())
                        .reason(
                            "recipient account is not initialized and transfer amount is below rent-exempt minimum",
                        ),
                )),
            ));
        }

        Ok(json!({
            "estimateFee": {
                "amount": format!("{}", self.fee),
                "currency": "USD",
                "unitPrice": 0.0,
                "fiatValue": 0.0
            }
        })
        .to_string())
    }

    async fn estimate_fee_without_balance_check(
        &self,
        _req: wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, wallet_api::error::service::ServiceError> {
        Ok(json!({
            "estimateFee": {
                "amount": format!("{}", self.fee),
                "currency": "USD",
                "unitPrice": 0.0,
                "fiatValue": 0.0
            }
        })
        .to_string())
    }

    async fn recipient_ata_rent(
        &self,
        _req: &wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
    ) -> Result<u64, wallet_api::error::service::ServiceError> {
        Ok(if self.recipient_missing { TOKEN_ACCOUNT_RENT } else { 0 })
    }

    async fn build_transfer_raw(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<(String, RawTx, String), wallet_api::error::service::ServiceError> {
        unimplemented!("not used in collect fee checks")
    }

    async fn broadcast_transfer(
        &self,
        _raw: RawTx,
    ) -> Result<wallet_api::domain::chain::TransferResp, wallet_api::error::service::ServiceError>
    {
        unimplemented!("not used in collect fee checks")
    }
}

#[derive(Clone)]
struct CollectEthTestAdapter {
    balance_wei: U256,
    fee_amount: f64,
}

impl CollectEthTestAdapter {
    fn fee_json(&self) -> String {
        json!({
            "default": "propose",
            "data": [{
                "type": "propose",
                "estimateFee": {
                    "amount": format!("{}", self.fee_amount),
                    "currency": "USD",
                    "unitPrice": 0.0,
                    "fiatValue": 0.0
                },
                "maxFee": {
                    "amount": format!("{}", self.fee_amount * 1.2),
                    "currency": "USD",
                    "unitPrice": 0.0,
                    "fiatValue": 0.0
                },
                "feeSetting": {
                    "gasLimit": 23100,
                    "baseFee": "1000000000",
                    "priorityFee": "1000000000",
                    "maxFeePerGas": "2000000000"
                }
            }]
        })
        .to_string()
    }
}

#[async_trait::async_trait]
impl Tx for CollectEthTestAdapter {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<
        wallet_chain_interact::tron::protocol::account::AccountResourceDetail,
        wallet_api::error::service::ServiceError,
    > {
        unimplemented!("not used in collect fee checks")
    }

    async fn balance_token_key(
        &self,
        _addr: &str,
        _token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error> {
        Ok(self.balance_wei)
    }

    async fn nonce(&self, _addr: &str) -> Result<u64, wallet_api::error::service::ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error> {
        Ok(0)
    }

    async fn query_tx_res(
        &self,
        _hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, wallet_chain_interact::Error>
    {
        Ok(None)
    }

    async fn token_symbol(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("ETH".to_string())
    }

    async fn token_name(&self, _token: &str) -> Result<String, wallet_chain_interact::Error> {
        Ok("Ethereum".to_string())
    }

    async fn decimals(&self, _token: &str) -> Result<u8, wallet_chain_interact::Error> {
        Ok(18)
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
        unimplemented!("not used in collect fee checks")
    }

    async fn estimate_fee(
        &self,
        _req: wallet_api::request::api_wallet::trans::ApiBaseTransferReq,
        _main_symbol: &str,
    ) -> Result<String, wallet_api::error::service::ServiceError> {
        Ok(self.fee_json())
    }

    async fn build_transfer_raw(
        &self,
        _params: &wallet_api::request::api_wallet::trans::ApiTransferReq,
        _private_key: wallet_chain_interact::types::ChainPrivateKey,
    ) -> Result<(String, RawTx, String), wallet_api::error::service::ServiceError> {
        unimplemented!("not used in collect fee checks")
    }

    async fn broadcast_transfer(
        &self,
        _raw: RawTx,
    ) -> Result<wallet_api::domain::chain::TransferResp, wallet_api::error::service::ServiceError>
    {
        unimplemented!("not used in collect fee checks")
    }
}

pub(crate) struct TestAdapterGuard {
    chain_code: String,
}

impl Drop for TestAdapterGuard {
    fn drop(&mut self) {
        clear_test_transaction_adapter_override(&self.chain_code);
    }
}

pub(crate) fn install_collect_test_adapter(
    recipient_missing: bool,
    balance: u64,
) -> TestAdapterGuard {
    let chain_code = ChainCode::Solana.to_string();
    let adapter = Arc::new(CollectSolTestAdapter {
        recipient_missing,
        force_fee_insufficient: false,
        balance,
        fee: 0.000015,
    });
    let tx_adapter: Arc<dyn Tx + Send + Sync> = adapter;
    set_test_transaction_adapter_override(&chain_code, tx_adapter);
    TestAdapterGuard { chain_code }
}

pub(crate) fn install_collect_test_adapter_fee_shortage(
    recipient_missing: bool,
    balance: u64,
) -> TestAdapterGuard {
    let chain_code = ChainCode::Solana.to_string();
    let adapter = Arc::new(CollectSolTestAdapter {
        recipient_missing,
        force_fee_insufficient: true,
        balance,
        fee: 0.000015,
    });
    let tx_adapter: Arc<dyn Tx + Send + Sync> = adapter;
    set_test_transaction_adapter_override(&chain_code, tx_adapter);
    TestAdapterGuard { chain_code }
}

pub(crate) struct EthAdapterGuard {
    chain_code: String,
}

impl Drop for EthAdapterGuard {
    fn drop(&mut self) {
        clear_test_transaction_adapter_override(&self.chain_code);
    }
}

pub(crate) fn install_collect_eth_test_adapter(
    balance_wei: U256,
    fee_amount: f64,
) -> EthAdapterGuard {
    let chain_code = ChainCode::Ethereum.to_string();
    let adapter = Arc::new(CollectEthTestAdapter { balance_wei, fee_amount });
    let tx_adapter: Arc<dyn Tx + Send + Sync> = adapter;
    set_test_transaction_adapter_override(&chain_code, tx_adapter);
    EthAdapterGuard { chain_code }
}
