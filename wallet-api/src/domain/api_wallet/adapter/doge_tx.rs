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
    dog::{DogChain, operations::transfer::TransferArg, provider::ProviderConfig},
    tron::protocol::account::AccountResourceDetail,
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::asset_token_key::AssetTokenKey, repositories::api_wallet::account::ApiAccountRepo,
};
use wallet_types::chain::address::r#type::DogAddressType;

pub(crate) struct DogeTx {
    chain: DogChain,
    ctx: &'static Context,
}

impl DogeTx {
    #[cfg(test)]
    pub fn new_for_test(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, Error> {
        Self::new_with_ctx(ctx, rpc_url, header_opt)
    }

    pub fn new_with_ctx(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, Error> {
        let network = wallet_types::chain::network::NetworkKind::Mainnet;
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        let config = ProviderConfig {
            rpc_url: rpc_url.to_string(),
            rpc_auth: None,
            http_url: rpc_url.to_string(),
            http_api_key: None,
            access_key: None,
        };
        let dog_chain = DogChain::new(config, network, header_opt, timeout)?;
        Ok(Self { chain: dog_chain, ctx })
    }

    pub fn handle_doge_fee_error(&self, err: wallet_chain_interact::Error) -> ServiceError {
        match err {
            Error::UtxoError(wallet_chain_interact::UtxoError::InsufficientBalance) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .reason("utxo balance is insufficient"),
                    ),
                )
                .into()
            }
            Error::UtxoError(wallet_chain_interact::UtxoError::InsufficientFee(_fee)) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::InsufficientFeeBalance,
                )
                .into()
            }
            Error::UtxoError(wallet_chain_interact::UtxoError::ExceedsMaximum) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ExceedsMaximum,
                )
                .into()
            }
            Error::UtxoError(wallet_chain_interact::UtxoError::DustTx) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::DustTransaction,
                )
                .into()
            }
            Error::UtxoError(wallet_chain_interact::UtxoError::ExceedsMaxFeeRate) => {
                crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::ExceedsMaxFeerate,
                )
                .into()
            }
            _ => err.into(),
        }
    }
}

#[async_trait::async_trait]
impl Tx for DogeTx {
    async fn account_resource(&self, _: &str) -> Result<AccountResourceDetail, ServiceError> {
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

    async fn black_address(&self, _: &str, _: &str) -> Result<bool, ServiceError> {
        Ok(false)
    }

    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        let _ = self.check_min_transfer(&params.base.value, params.base.decimals)?;
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
        let address_type = DogAddressType::try_from(Some(account.address_type))?;

        let arg = TransferArg::new(
            &params.base.from,
            &params.base.to,
            &params.base.value,
            address_type.into(),
            self.chain.network,
        )?
        .with_spend_all(params.base.spend_all);

        let tx = self
            .chain
            .transfer(arg, private_key)
            .await
            .map_err(|e| self.handle_doge_fee_error(e))?;

        Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
    }

    async fn transfer_with_ctx(
        &self,
        _ctx: &crate::context::Context,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        let _ = self.check_min_transfer(&params.base.value, params.base.decimals)?;
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
        let address_type = DogAddressType::try_from(Some(account.address_type))?;

        let arg = TransferArg::new(
            &params.base.from,
            &params.base.to,
            &params.base.value,
            address_type.into(),
            self.chain.network,
        )?
        .with_spend_all(params.base.spend_all);

        let tx = self
            .chain
            .transfer(arg, private_key)
            .await
            .map_err(|e| self.handle_doge_fee_error(e))?;

        Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
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

        let pool = self.ctx.api_wallet_pool()?;
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(&req.from, &req.chain_code, &pool)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(req.from.to_string()),
                ))?;

        let address_type = DogAddressType::try_from(Some(account.address_type))?;

        let params =
            TransferArg::new(&req.from, &req.to, &req.value, address_type, self.chain.network)?
                .with_spend_all(req.spend_all);

        let fee =
            self.chain.estimate_fee(params).await.map_err(|e| self.handle_doge_fee_error(e))?;

        let res = CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        Ok(res)
    }

    async fn estimate_fee_with_ctx(
        &self,
        _ctx: &crate::context::Context,
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

        let pool = self.ctx.api_wallet_pool()?;
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(&req.from, &req.chain_code, &pool)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(req.from.to_string()),
                ))?;

        let address_type = DogAddressType::try_from(Some(account.address_type))?;

        let params =
            TransferArg::new(&req.from, &req.to, &req.value, address_type, self.chain.network)?
                .with_spend_all(req.spend_all);

        let fee =
            self.chain.estimate_fee(params).await.map_err(|e| self.handle_doge_fee_error(e))?;

        let res = CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        Ok(res)
    }

    // async fn approve(
    //     &self,
    //     _: &ApproveReq,
    //     _: ChainPrivateKey,
    //     _: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn approve_fee(&self, _: &ApproveReq, _: U256, _: &str) -> Result<String, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn allowance(&self, _: &str, _: &str, _: &str) -> Result<U256, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn swap_quote(
    //     &self,
    //     _: &QuoteReq,
    //     _: &AggQuoteResp,
    //     _: &str,
    // ) -> Result<(U256, String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn swap(
    //     &self,
    //     _: &SwapReq,
    //     _: String,
    //     _: ChainPrivateKey,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn deposit_fee(
    //     &self,
    //     _: DepositReq,
    //     _: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn deposit(
    //     &self,
    //     _: &DepositReq,
    //     _: String,
    //     _: ChainPrivateKey,
    //     _: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn withdraw_fee(
    //     &self,
    //     _: WithdrawReq,
    //     _: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }

    // async fn withdraw(
    //     &self,
    //     _: &WithdrawReq,
    //     _: String,
    //     _: ChainPrivateKey,
    //     _: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     Err(crate::error::business::BusinessError::Chain(
    //         crate::error::business::chain::ChainError::NotSupportChain,
    //     )
    //     .into())
    // }
}

// #[async_trait::async_trait]
// impl Multisig for DogeTx {
//     async fn multisig_address(
//         &self,
//         _: &MultisigAccountEntity,
//         _: &MultisigMemberEntities,
//     ) -> Result<FetchMultisigAddressResp, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn deploy_multisig_account(
//         &self,
//         _: &MultisigAccountEntity,
//         _: &MultisigMemberEntities,
//         _: Option<String>,
//         _: ChainPrivateKey,
//     ) -> Result<(String, String), ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn deploy_multisig_fee(
//         &self,
//         _: &MultisigAccountEntity,
//         _: MultisigMemberEntities,
//         _: &str,
//     ) -> Result<String, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn build_multisig_fee(
//         &self,
//         _: &MultisigQueueFeeParams,
//         _: &MultisigAccountEntity,
//         _: u8,
//         _: Option<String>,
//         _: &str,
//     ) -> Result<String, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn build_multisig_with_account(
//         &self,
//         _: &TransferParams,
//         _: &MultisigAccountEntity,
//         _: &ApiAssetsEntity,
//         _: ChainPrivateKey,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn build_multisig_with_permission(
//         &self,
//         _: &TransferParams,
//         _: &PermissionEntity,
//         _: &CoinEntity,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn sign_fee(
//         &self,
//         _: &MultisigAccountEntity,
//         _: &str,
//         _: &str,
//         _: &str,
//     ) -> Result<String, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn sign_multisig_tx(
//         &self,
//         _: &MultisigAccountEntity,
//         _: &str,
//         _: ChainPrivateKey,
//         _: &str,
//     ) -> Result<MultisigSignResp, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
//
//     async fn estimate_multisig_fee(
//         &self,
//         _: &MultisigQueueEntity,
//         _: &CoinEntity,
//         _: &BackendApi,
//         _: Vec<String>,
//         _: &str,
//     ) -> Result<String, ServiceError> {
//         Err(crate::error::business::BusinessError::MultisigAccount(
//             crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
//                 ChainCode::Dogcoin.to_string(),
//             )
//             .into(),
//         )
//         .into())
//     }
// }
