#![allow(unused_variables)]
use crate::{
    domain::{
        api_wallet::adapter::tx::{RawTx, Tx},
        chain::TransferResp,
    },
    error::service::ServiceError,
    request::api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
};
use alloy::primitives::U256;
use std::sync::Mutex;
use wallet_chain_interact::{
    tron::protocol::account::AccountResourceDetail, types::ChainPrivateKey,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;

#[derive(Default)]
pub(super) struct BalanceAdapter {
    pub calls: Mutex<Vec<(String, AssetTokenKey)>>,
}
#[async_trait::async_trait]
impl Tx for BalanceAdapter {
    async fn account_resource(
        &self,
        owner_address: &str,
    ) -> Result<AccountResourceDetail, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn balance_token_key(
        &self,
        addr: &str,
        token: AssetTokenKey,
    ) -> Result<U256, wallet_chain_interact::Error> {
        self.calls.lock().unwrap().push((addr.to_owned(), token));
        Ok(U256::from(20))
    }
    async fn nonce(&self, addr: &str) -> Result<u64, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn block_num(&self) -> Result<u64, wallet_chain_interact::Error> {
        panic!("unexpected adapter call")
    }
    async fn query_tx_res(
        &self,
        hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, wallet_chain_interact::Error>
    {
        panic!("unexpected adapter call")
    }
    async fn token_symbol(&self, token: &str) -> Result<String, wallet_chain_interact::Error> {
        panic!("unexpected adapter call")
    }
    async fn token_name(&self, token: &str) -> Result<String, wallet_chain_interact::Error> {
        panic!("unexpected adapter call")
    }
    async fn decimals(&self, token: &str) -> Result<u8, wallet_chain_interact::Error> {
        panic!("unexpected adapter call")
    }
    async fn black_address(
        &self,
        token: &str,
        owner: &str,
    ) -> Result<bool, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn estimate_fee(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn build_transfer_raw(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
    async fn broadcast_transfer(
        &self,
        raw: RawTx,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        panic!("unexpected adapter call")
    }
}
