use crate::{
    context::Context,
    domain::{
        api_wallet::adapter::{
            TIME_OUT,
            tx::{RawTx, Tx},
        },
        chain::TransferResp,
        coin::TokenCurrencyGetter,
    },
    error::service::ServiceError,
    request::api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
    response_vo::CommonFeeDetails,
};
use alloy::primitives::U256;
use std::collections::HashMap;
use wallet_chain_interact::{
    Error, QueryTransactionResult,
    ton::{
        Cell,
        chain::TonChain,
        operations::{BuildInternalMsg, token_transfer::TokenTransferOpt, transfer::TransferOpt},
        provider::Provider,
    },
    tron::protocol::account::AccountResourceDetail,
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::asset_token_key::AssetTokenKey, repositories::api_wallet::account::ApiAccountRepo,
};
use wallet_transport::client::HttpClient;
use wallet_types::chain::address::r#type::TonAddressType;
use wallet_utils::unit;

pub(crate) struct TonTx {
    chain: TonChain,
    ctx: &'static Context,
}

impl TonTx {
    #[cfg(test)]
    pub fn new_for_test(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        Self::new_with_ctx(ctx, rpc_url, header_opt)
    }

    pub fn new_with_ctx(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        let http_client = HttpClient::new(rpc_url, header_opt, timeout)?;
        let provider = Provider::new(http_client);

        let ton = TonChain::new(provider)?;
        Ok(Self { chain: ton, ctx })
    }

    pub async fn build_ext_cell(
        &self,
        req: &ApiBaseTransferReq,
        provider: &Provider,
        address_type: TonAddressType,
    ) -> Result<Cell, crate::error::service::ServiceError> {
        let token_key = req.token_address.clone();
        if let Some(token) = token_key.to_chain_token_option() {
            let value = unit::convert_to_u256(&req.value, req.decimals)?;
            let arg = TokenTransferOpt::new(&req.from, &req.to, &token, value, req.spend_all)?;

            Ok(arg.build_trans(address_type, provider).await?)
        } else {
            tracing::info!("transfer ------------------- 16:");
            let arg = TransferOpt::new(&req.from, &req.to, &req.value, req.spend_all)?;

            Ok(arg.build_trans(address_type, provider).await?)
        }
    }
}

#[async_trait::async_trait]
impl Tx for TonTx {
    async fn account_resource(
        &self,
        _owner_address: &str,
    ) -> Result<AccountResourceDetail, ServiceError> {
        todo!()
    }

    async fn balance_token_key(&self, addr: &str, token: AssetTokenKey) -> Result<U256, Error> {
        self.chain.balance(addr, token.to_chain_token_option()).await
    }

    async fn nonce(&self, addr: &str) -> Result<u64, ServiceError> {
        Ok(0)
    }

    async fn block_num(&self) -> Result<u64, Error> {
        self.chain.block_num().await
    }

    async fn query_tx_res(&self, hash: &str) -> Result<Option<QueryTransactionResult>, Error> {
        self.chain.query_tx_res(hash).await
    }

    async fn token_symbol(&self, token: &str) -> Result<String, Error> {
        self.chain.token_symbol(token).await
    }

    async fn token_name(&self, token: &str) -> Result<String, Error> {
        self.chain.token_name(token).await
    }

    async fn decimals(&self, token: &str) -> Result<u8, Error> {
        self.chain.decimals(token).await
    }

    async fn black_address(&self, _token: &str, _owner: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        tracing::info!("transfer ------------------- 11:");

        let token_key = params.base.token_address.clone();
        let chain_token = token_key.to_chain_token_option();
        let balance = self.chain.balance(&params.base.from, chain_token.clone()).await?;
        if balance < transfer_amount {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                    crate::error::business::chain::InsufficientBalanceDetail::new()
                        .from_addr(params.base.from.clone())
                        .to_addr(params.base.to.clone())
                        .chain_code(params.base.chain_code.clone())
                        .token_addr(chain_token.unwrap_or_default())
                        .value(transfer_amount.to_string())
                        .balance(balance.to_string())
                        .need(transfer_amount.to_string())
                        .reason("token balance is insufficient for ton transfer"),
                ),
            ))?;
        }
        tracing::info!("transfer ------------------- 12:");

        let pool = self.ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one_by_address_chain_code(
            &params.base.from,
            &params.base.chain_code,
            &pool,
        )
        .await?
        .ok_or(crate::error::business::BusinessError::Account(
            crate::error::business::account::AccountError::NotFound(params.base.from.to_string()),
        ))?;

        let address_type = TonAddressType::try_from(account.address_type.as_str())?;

        tracing::info!("transfer ------------------- 13:");
        let msg_cell =
            self.build_ext_cell(&params.base, &self.chain.provider, address_type).await?;

        tracing::info!("transfer ------------------- 14:");
        let fee =
            self.chain.estimate_fee(msg_cell.clone(), &params.base.from, address_type).await?;

        let mut trans_fee = U256::from(fee.get_fee());
        if token_key.is_native() {
            if !params.base.spend_all {
                trans_fee += transfer_amount;
                if balance < trans_fee {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::InsufficientFeeBalance,
                    ))?;
                }
            }
        } else {
            let balance = self.chain.balance(&params.base.from, None).await?;
            if balance < trans_fee {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::InsufficientFeeBalance,
                ))?;
            }
        }
        tracing::info!("transfer ------------------- 15:");
        let tx_hash = self.chain.exec(msg_cell, private_key, address_type).await?;

        Ok(TransferResp::new(tx_hash, fee.get_fee_ton().to_string()))
    }

    async fn build_transfer_raw(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), crate::error::service::ServiceError> {
        todo!("build_transfer_raw")
    }

    async fn broadcast_transfer(
        &self,
        raw: RawTx,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        todo!("broadcast_transfer")
    }

    async fn estimate_fee(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();
        let pool = self.ctx.api_wallet_pool()?;

        let token_currency = TokenCurrencyGetter::get_currency_by_token_key_with_pool(
            &self.ctx.core_pool()?,
            currency,
            &req.chain_code,
            main_symbol,
            AssetTokenKey::Native,
        )
        .await?;

        let pool = self.ctx.api_wallet_pool()?;
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(&req.from, &req.chain_code, &pool)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(req.from.to_string()),
                ))?;

        let address_type = TonAddressType::try_from(account.address_type.as_str())?;

        let msg_cell = self.build_ext_cell(&req, &self.chain.provider, address_type).await?;
        let fee = self.chain.estimate_fee(msg_cell, &req.from, address_type).await?;

        let res = CommonFeeDetails::new(fee.get_fee_ton(), token_currency, currency)?;
        let fee = wallet_utils::serde_func::serde_to_string(&res)?;
        Ok(fee)
    }

    async fn transfer_with_ctx(
        &self,
        _ctx: &crate::context::Context,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        self.transfer(params, private_key).await
    }

    async fn estimate_fee_with_ctx(
        &self,
        _ctx: &crate::context::Context,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, ServiceError> {
        self.estimate_fee(req, main_symbol).await
    }
}

#[cfg(test)]
mod tests {
    use super::AssetTokenKey;

    #[test]
    fn from_raw_treats_blank_as_native() {
        assert!(AssetTokenKey::from_raw(None).is_native());
        assert!(AssetTokenKey::from_raw(Some("".trim())).is_native());
        assert!(AssetTokenKey::from_raw(Some("   ")).is_native());
    }
}
