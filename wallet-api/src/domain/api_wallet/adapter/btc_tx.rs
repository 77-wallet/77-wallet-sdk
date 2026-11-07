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
    Error,
    btc::{BtcChain, operations::transfer::TransferArg, provider::ProviderConfig},
    tron::protocol::account::AccountResourceDetail,
    types::ChainPrivateKey,
};
use wallet_database::{
    entities::asset_token_key::AssetTokenKey, repositories::api_wallet::account::ApiAccountRepo,
};
use wallet_utils::serde_func::serde_to_string;

pub(crate) struct BtcTx {
    chain: BtcChain,
    ctx: &'static Context,
}

impl BtcTx {
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
        };
        let btc_chain = BtcChain::new(config, network, header_opt, timeout)?;
        Ok(Self { chain: btc_chain, ctx })
    }

    pub fn handle_btc_fee_error(&self, err: wallet_chain_interact::Error) -> ServiceError {
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
impl Tx for BtcTx {
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

    async fn query_tx_res(
        &self,
        hash: &str,
    ) -> Result<Option<wallet_chain_interact::QueryTransactionResult>, Error> {
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
        tracing::info!("transfer ------------------- 11:");
        let pool = self.ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one_by_address_chain_code(
            &params.base.from,
            &params.base.chain_code,
            &pool,
        )
        .await?
        .ok_or(crate::error::business::BusinessError::ApiWallet(
            crate::error::business::api_wallet::ApiWalletError::NotFoundAccount,
        ))?;
        let params = TransferArg::new(
            &params.base.from,
            &params.base.to,
            &params.base.value,
            Some(account.address_type),
            self.chain.network,
        )?
        .with_spend_all(params.base.spend_all);

        let tx = self
            .chain
            .transfer(params, private_key)
            .await
            .map_err(|e| self.handle_btc_fee_error(e))?;

        Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
    }

    async fn transfer_with_ctx(
        &self,
        _ctx: &crate::context::Context,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        tracing::info!("transfer ------------------- 11:");
        let pool = self.ctx.api_wallet_pool()?;
        let account = ApiAccountRepo::find_one_by_address_chain_code(
            &params.base.from,
            &params.base.chain_code,
            &pool,
        )
        .await?
        .ok_or(crate::error::business::BusinessError::ApiWallet(
            crate::error::business::api_wallet::ApiWalletError::NotFoundAccount,
        ))?;
        let params = TransferArg::new(
            &params.base.from,
            &params.base.to,
            &params.base.value,
            Some(account.address_type),
            self.chain.network,
        )?
        .with_spend_all(params.base.spend_all);

        let tx = self
            .chain
            .transfer(params, private_key)
            .await
            .map_err(|e| self.handle_btc_fee_error(e))?;

        Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
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

        // 获取账号
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(&req.from, &req.chain_code, &pool)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(req.from.to_string()),
                ))?;
        let params = TransferArg::new(
            &req.from,
            &req.to,
            &req.value,
            Some(account.address_type),
            self.chain.network,
        )?
        .with_spend_all(req.spend_all);

        let fee = self
            .chain
            .estimate_fee(params, None)
            .await
            .map_err(|e| self.handle_btc_fee_error(e))?;

        let res = CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
        let res = serde_to_string(&res)?;
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
        // 获取账号
        let pool = self.ctx.api_wallet_pool()?;
        let account =
            ApiAccountRepo::find_one_by_address_chain_code(&req.from, &req.chain_code, &pool)
                .await?
                .ok_or(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::NotFound(req.from.to_string()),
                ))?;
        let params = TransferArg::new(
            &req.from,
            &req.to,
            &req.value,
            Some(account.address_type),
            self.chain.network,
        )?
        .with_spend_all(req.spend_all);

        let fee = self
            .chain
            .estimate_fee(params, None)
            .await
            .map_err(|e| self.handle_btc_fee_error(e))?;

        let res = CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
        let res = serde_to_string(&res)?;
        Ok(res)
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
// impl Multisig for BtcTx {
//     async fn multisig_address(
//         &self,
//         account: &MultisigAccountEntity,
//         member: &MultisigMemberEntities,
//     ) -> Result<FetchMultisigAddressResp, ServiceError> {
//         let params = MultisigAccountOpt::new(
//             account.threshold as u8,
//             member.get_owner_pubkey(),
//             &account.address_type,
//         )?;
//         Ok(self.chain.multisig_address(params).await?)
//     }
//
//     async fn deploy_multisig_account(
//         &self,
//         _: &MultisigAccountEntity,
//         _: &MultisigMemberEntities,
//         _: Option<String>,
//         _: ChainPrivateKey,
//     ) -> Result<(String, String), ServiceError> {
//         Ok(("".to_string(), "".to_string()))
//     }
//
//     async fn deploy_multisig_fee(
//         &self,
//         _: &MultisigAccountEntity,
//         _: MultisigMemberEntities,
//         _: &str,
//     ) -> Result<String, ServiceError> {
//         Ok("0".to_string())
//     }
//
//     async fn build_multisig_fee(
//         &self,
//         req: &MultisigQueueFeeParams,
//         account: &MultisigAccountEntity,
//         _: u8,
//         _: Option<String>,
//         main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         let currency = crate::app_state::APP_STATE.read().await;
//         let currency = currency.currency();
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &req.chain_code, main_symbol, None).await?;
//         let params = TransferArg::new(
//             &req.from,
//             &req.to,
//             &req.value,
//             account.address_type(),
//             self.chain.network,
//         )?
//         .with_spend_all(req.spend_all.unwrap_or(false));
//
//         let multisig_parmas = MultisigSignParams::new(
//             account.threshold as i8,
//             account.member_num as i8,
//             account.salt.clone(),
//         )
//         .with_inner_key(account.authority_addr.clone());
//
//         let fee = self
//             .chain
//             .estimate_fee(params, Some(multisig_parmas))
//             .await
//             .map_err(|e| self.handle_btc_fee_error(e))?;
//
//         let fee = CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
//         Ok(serde_to_string(&fee)?)
//     }
//
//     async fn build_multisig_with_account(
//         &self,
//         req: &TransferParams,
//         account: &MultisigAccountEntity,
//         _: &ApiAssetsEntity,
//         _: ChainPrivateKey,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         let params = TransferArg::new(
//             &req.from,
//             &req.to,
//             &req.value,
//             account.address_type(),
//             self.chain.network,
//         )?
//         .with_spend_all(req.spend_all);
//
//         let multisig_parmas = MultisigSignParams::new(
//             account.threshold as i8,
//             account.member_num as i8,
//             account.salt.clone(),
//         )
//         .with_inner_key(account.authority_addr.clone());
//
//         Ok(self
//             .chain
//             .build_multisig_tx(params, multisig_parmas)
//             .await
//             .map_err(|e| self.handle_btc_fee_error(e))?)
//     }
//
//     async fn build_multisig_with_permission(
//         &self,
//         _: &TransferParams,
//         _: &PermissionEntity,
//         _: &CoinEntity,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         Err(crate::error::business::BusinessError::Permission(
//             crate::error::business::permission::PermissionError::UnSupportPermissionChain,
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
//         Ok(" ".to_string())
//     }
//
//     async fn sign_multisig_tx(
//         &self,
//         account: &MultisigAccountEntity,
//         _: &str,
//         key: ChainPrivateKey,
//         raw_data: &str,
//     ) -> Result<MultisigSignResp, ServiceError> {
//         let params = MultisigTransactionOpt::new(
//             account.address.clone(),
//             "0".to_string(),
//             &account.salt,
//             raw_data,
//             &account.address_type,
//         )?;
//         Ok(self.chain.sign_multisig_tx(params, key).await?)
//     }
//
//     async fn estimate_multisig_fee(
//         &self,
//         queue: &MultisigQueueEntity,
//         _: &CoinEntity,
//         _: &BackendApi,
//         _: Vec<String>,
//         main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         let currency = crate::app_state::APP_STATE.read().await;
//         let currency = currency.currency();
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &queue.chain_code, main_symbol, None)
//                 .await?;
//
//         let multisig_account = MultisigDomain::account_by_id(&queue.account_id, pool).await?;
//
//         let multisig_parmas = MultisigSignParams::new(
//             multisig_account.threshold as i8,
//             multisig_account.member_num as i8,
//             multisig_account.salt.clone(),
//         )
//         .with_inner_key(multisig_account.authority_addr.clone());
//
//         let fee = self
//             .chain
//             .estimate_multisig_fee(&queue.raw_data, multisig_parmas, &multisig_account.address_type)
//             .await
//             .map_err(|e| self.handle_btc_fee_error(e))?;
//
//         CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?.to_json_str()
//     }
// }

#[cfg(test)]
mod tests {
    use wallet_utils::init_test_log;

    #[tokio::test]
    async fn test_estimate_swap() {
        init_test_log();

        // let chain_code = "tron";
        // let rpc_url = "http://127.0.0.1:8545";
        // let rpc_url = "http://100.78.188.103:8090";
        // let rpc_url = "https://api.nileex.io";
    }
}
