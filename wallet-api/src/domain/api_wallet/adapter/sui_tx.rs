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
    sui::{Provider, SuiChain, transfer::TransferOpt},
    tron::protocol::account::AccountResourceDetail,
    types::ChainPrivateKey,
};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_utils::unit;

pub(crate) struct SuiTx {
    chain: SuiChain,
    ctx: &'static Context,
}

impl SuiTx {
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
        let mut provider = Provider::new(rpc_url, header_opt)?;
        let timeout = std::time::Duration::from_secs(TIME_OUT);
        provider.grpc_client = provider
            .grpc_client
            .with_response_headers_timeout(timeout)
            .with_body_idle_timeout(timeout);

        let sui_chain = SuiChain::new(provider)?;
        Ok(Self { chain: sui_chain, ctx })
    }
}

#[async_trait::async_trait]
impl Tx for SuiTx {
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

    async fn black_address(&self, token: &str, owner: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let token_key = params.base.token_address.clone();
        let balance =
            self.chain.balance(&params.base.from, token_key.to_chain_token_option()).await?;
        if balance < transfer_amount {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                    crate::error::business::chain::InsufficientBalanceDetail::new()
                        .from_addr(params.base.from.clone())
                        .to_addr(params.base.to.clone())
                        .chain_code(params.base.chain_code.clone())
                        .token_addr(token_key.to_chain_token_option().unwrap_or_default())
                        .value(transfer_amount.to_string())
                        .balance(balance.to_string())
                        .need(transfer_amount.to_string())
                        .reason("token balance is insufficient for sui transfer"),
                ),
            ))?;
        }

        tracing::info!("transfer ------------------- 9:");

        let req = TransferOpt::new(
            &params.base.from,
            &params.base.to,
            transfer_amount,
            token_key.to_chain_token_option(),
        )?;

        let mut helper = req.select_coin(&self.chain.provider).await?;
        tracing::info!("transfer ------------------- 10:");
        let pt = req.build_pt(&self.chain.provider, &mut helper, None).await?;

        tracing::info!("transfer ------------------- 11:");
        let gas = self.chain.estimate_fee(&params.base.from, pt).await?;

        let mut trans_fee = U256::from(gas.get_fee());
        if token_key.is_native() {
            trans_fee += transfer_amount;
            if balance < trans_fee {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::InsufficientFeeBalance,
                ))?;
            }
        } else {
            let balance = self.chain.balance(&params.base.from, None).await?;
            if balance < trans_fee {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::InsufficientFeeBalance,
                ))?;
            }
        }

        let fee = gas.get_fee_f64();
        let tx_data = req.build_data(&self.chain.provider, helper, gas).await?;
        let tx_hash = self.chain.exec(tx_data, private_key).await?;

        Ok(TransferResp::new(tx_hash, fee.to_string()))
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
            wallet_database::entities::asset_token_key::AssetTokenKey::Native,
        )
        .await?;

        let amount = unit::convert_to_u256(&req.value, req.decimals)?;
        let token_key = req.token_address.clone();
        let params =
            TransferOpt::new(&req.from, &req.to, amount, token_key.to_chain_token_option())?;

        let mut helper = params.select_coin(&self.chain.provider).await?;
        let pt = params.build_pt(&self.chain.provider, &mut helper, None).await?;

        let gas = self.chain.estimate_fee(&req.from, pt).await?;

        let res = CommonFeeDetails::new(gas.get_fee_f64(), token_currency, currency)?;

        let fee = wallet_utils::serde_func::serde_to_string(&res)?;
        Ok(fee)
    }

    // async fn approve(
    //     &self,
    //     _req: &ApproveReq,
    //     _key: ChainPrivateKey,
    //     _value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn approve_fee(
    //     &self,
    //     _req: &ApproveReq,
    //     _value: U256,
    //     _main_symbol: &str,
    // ) -> Result<String, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn allowance(
    //     &self,
    //     _from: &str,
    //     _token: &str,
    //     _spender: &str,
    // ) -> Result<U256, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn swap_quote(
    //     &self,
    //     _req: &QuoteReq,
    //     _quote_resp: &AggQuoteResp,
    //     _symbol: &str,
    // ) -> Result<(U256, String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn swap(
    //     &self,
    //     _req: &SwapReq,
    //     _fee: String,
    //     _key: ChainPrivateKey,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn deposit_fee(
    //     &self,
    //     _req: DepositReq,
    //     _main_coin: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn deposit(
    //     &self,
    //     _req: &DepositReq,
    //     _fee: String,
    //     _key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn withdraw_fee(
    //     &self,
    //     _req: WithdrawReq,
    //     _main_coin: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn withdraw(
    //     &self,
    //     _req: &WithdrawReq,
    //     _fee: String,
    //     _key: ChainPrivateKey,
    //     _value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }
}

// #[async_trait::async_trait]
// impl Multisig for SuiTx {
//     async fn multisig_address(
//         &self,
//         _account: &MultisigAccountEntity,
//         _member: &MultisigMemberEntities,
//     ) -> Result<FetchMultisigAddressResp, ServiceError> {
//         todo!()
//     }
//
//     async fn deploy_multisig_account(
//         &self,
//         _account: &MultisigAccountEntity,
//         _member: &MultisigMemberEntities,
//         _fee_setting: Option<String>,
//         _key: ChainPrivateKey,
//     ) -> Result<(String, String), ServiceError> {
//         todo!()
//     }
//
//     async fn deploy_multisig_fee(
//         &self,
//         _account: &MultisigAccountEntity,
//         _member: MultisigMemberEntities,
//         _main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         todo!()
//     }
//
//     async fn build_multisig_fee(
//         &self,
//         _req: &MultisigQueueFeeParams,
//         _account: &MultisigAccountEntity,
//         _decimal: u8,
//         _token: Option<String>,
//         _main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         todo!()
//     }
//
//     async fn build_multisig_with_account(
//         &self,
//         _req: &TransferParams,
//         _account: &MultisigAccountEntity,
//         _assets: &ApiAssetsEntity,
//         _key: ChainPrivateKey,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         todo!()
//     }
//
//     async fn build_multisig_with_permission(
//         &self,
//         _req: &TransferParams,
//         _p: &PermissionEntity,
//         _coin: &CoinEntity,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         todo!()
//     }
//
//     async fn sign_fee(
//         &self,
//         _account: &MultisigAccountEntity,
//         _address: &str,
//         _raw_data: &str,
//         _main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         todo!()
//     }
//
//     async fn sign_multisig_tx(
//         &self,
//         _account: &MultisigAccountEntity,
//         _address: &str,
//         _key: ChainPrivateKey,
//         _raw_data: &str,
//     ) -> Result<MultisigSignResp, ServiceError> {
//         todo!()
//     }
//
//     async fn estimate_multisig_fee(
//         &self,
//         _queue: &MultisigQueueEntity,
//         _coin: &CoinEntity,
//         _backend: &BackendApi,
//         _sign_list: Vec<String>,
//         _main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         todo!()
//     }
// }

#[cfg(test)]
mod sui_grpc_tests {
    use super::*;

    #[tokio::test]
    async fn api_sui_grpc_constructor_is_lazy_and_preserves_endpoint() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let adapter = SuiTx::new_with_ctx(
            ctx,
            "http://127.0.0.1:1",
            Some(HashMap::from([("x-test-auth".into(), "offline".into())])),
        )
        .expect("valid gRPC configuration requires no node connection");
        assert_eq!(adapter.chain.provider.grpc_client.uri().to_string(), "http://127.0.0.1:1/");
    }

    #[tokio::test]
    async fn api_sui_grpc_constructor_rejects_invalid_endpoint() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        assert!(SuiTx::new_with_ctx(ctx, "not a URI", None).is_err());
    }

    #[tokio::test]
    async fn api_sui_grpc_constructor_rejects_invalid_metadata() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        for (key, value) in [("bad header", "offline"), ("x-test-auth", "invalid\nvalue")] {
            assert!(
                SuiTx::new_with_ctx(
                    ctx,
                    "http://127.0.0.1:1",
                    Some(HashMap::from([(key.into(), value.into())]))
                )
                .is_err()
            );
        }
    }
}
