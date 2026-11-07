#[path = "eth_nonce_conflict.rs"]
mod nonce_conflict;
#[path = "eth_replacement.rs"]
pub(crate) mod replacement;
#[path = "eth_replacement_receipt.rs"]
mod replacement_receipt;

use crate::{
    context::Context,
    domain::{
        api_wallet::adapter::{
            TIME_OUT,
            tx::{Oracle, RawTx, Tx},
        },
        chain::{
            TransferResp, pare_fee_setting,
            swap::{
                EstimateSwapResult,
                evm_swap::{SwapParams, dexSwap1Call},
            },
        },
        coin::TokenCurrencyGetter,
    },
    error::{
        business::{BusinessError, chain::ChainError},
        service::ServiceError,
    },
    request::api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
    response_vo::{FeeDetails, scroll_fee_cap, scroll_fee_transaction},
};
use alloy::{
    network::TransactionBuilder as _,
    primitives::U256,
    rpc::types::TransactionRequest,
    sol_types::{SolCall as _, SolValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use wallet_chain_interact::{
    Error, QueryTransactionResult,
    eth::{self, EthChain, FeeSetting, operations::TransferOpt},
    tron::protocol::account::AccountResourceDetail,
    types::{ChainPrivateKey, Transaction},
};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_transport::{client::RpcClient, types::JsonRpcParams};
use wallet_transport_backend::response_vo::chain::GasOracle;
use wallet_types::chain::{chain::ChainCode, network::NetworkKind};
use wallet_utils::unit;

pub(crate) struct EthTx {
    pub(crate) chain: EthChain,
    provider: eth::Provider,
    rpc_url_for_log: String,
    rpc_header_for_query: Option<HashMap<String, String>>,
    ctx: &'static Context,
}

impl EthTx {
    #[cfg(test)]
    pub(crate) fn new_for_test(
        ctx: &'static Context,
        chain_code: ChainCode,
        rpc_url: &str,
        network: NetworkKind,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        Self::new_with_ctx(ctx, chain_code, rpc_url, network, header_opt)
    }

    pub(crate) fn new_with_ctx(
        ctx: &'static Context,
        chain_code: ChainCode,
        rpc_url: &str,
        network: NetworkKind,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        let rpc_header_for_query = header_opt.clone();
        let rpc_client = RpcClient::new(rpc_url, header_opt.clone(), timeout)?;
        let provider = eth::Provider::new(rpc_client)?;
        let eth_chain = EthChain::new(provider, network, chain_code)?;
        let rpc_client1 = RpcClient::new(rpc_url, header_opt, timeout)?;
        let provider1 = eth::Provider::new(rpc_client1)?;
        Ok(Self {
            chain: eth_chain,
            provider: provider1,
            rpc_url_for_log: rpc_url.to_string(),
            rpc_header_for_query,
            ctx,
        })
    }
    fn evm_raw_tx_hash_hex(raw: &[u8]) -> String {
        let digest = Keccak256::digest(raw);
        format!("0x{}", hex::encode(digest))
    }

    fn short_hex(raw: &[u8], take: usize) -> String {
        let n = raw.len().min(take);
        format!("0x{}", hex::encode(&raw[..n]))
    }

    fn gas_oracle_error() -> ServiceError {
        crate::error::business::BusinessError::ApiWallet(
            crate::error::business::api_wallet::ApiWalletError::GasOracle,
        )
        .into()
    }

    /// Build the metadata-free API Wallet EVM fee using the same propose-tier
    /// semantics as the stable fee quote path.
    ///
    /// `propose_gas_price` is a total gas price (base + priority), not a base
    /// fee. `fast_gas_price` is another total-price tier and must not be added
    /// to propose as if it were a priority fee.
    fn fee_setting_from_gas_oracle(
        gas_oracle: &GasOracle,
        gas_limit: u64,
    ) -> Result<FeeSetting, ServiceError> {
        let base_fee = unit::convert_to_u256(
            gas_oracle.suggest_base_fee.as_deref().ok_or_else(Self::gas_oracle_error)?,
            eth::consts::ETH_GWEI,
        )?;
        let propose_total = unit::convert_to_u256(
            gas_oracle.propose_gas_price.as_deref().ok_or_else(Self::gas_oracle_error)?,
            eth::consts::ETH_GWEI,
        )?;
        let mut priority_fee =
            propose_total.checked_sub(base_fee).ok_or_else(Self::gas_oracle_error)?;

        let mut max_fee_per_gas = propose_total * U256::from(12) / U256::from(10);
        let scale = U256::from(10).pow(U256::from(8)); // 0.1 gwei
        max_fee_per_gas = ((max_fee_per_gas + (scale - U256::from(1))) / scale) * scale;
        max_fee_per_gas = max_fee_per_gas.max(propose_total);

        if priority_fee == U256::ZERO {
            priority_fee = U256::from(10_000);
        }

        Ok(FeeSetting {
            base_fee,
            max_priority_fee_per_gas: priority_fee,
            max_fee_per_gas,
            gas_limit: U256::from(gas_limit),
        })
    }

    async fn estimate_swap(
        &self,
        swap_params: SwapParams,
    ) -> Result<EstimateSwapResult<FeeSetting>, ServiceError> {
        let tx = self.build_base_swap_tx(&swap_params)?;

        // estimate_fee
        let gas_limit = self.chain.provider.estimate_gas(tx.clone()).await?;
        let tx = tx.with_gas_limit(gas_limit.to::<u64>());

        let gas_price = self.chain.provider.gas_price().await?;
        let mut fee = FeeSetting::new_with_price(gas_price);
        fee.gas_limit = gas_limit;

        let result = self.chain.provider.eth_call(tx).await?;
        let bytes = wallet_utils::hex_func::hex_decode(&result[2..])?;

        let (amount_in, amount_out): (U256, U256) = <(U256, U256)>::abi_decode_params(&bytes, true)
            .map_err(|e| crate::error::service::ServiceError::AggregatorError {
                code: -1,
                agg_code: 0,
                msg: e.to_string(),
            })?;

        let resp = EstimateSwapResult { amount_in, amount_out, consumer: fee };
        Ok(resp)
    }

    async fn swap_base_transfer(
        &self,
        swap_params: &SwapParams,
        fee: String,
        key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let fee = pare_fee_setting(fee.as_str())?;
        let transfer_fee = fee.transaction_fee();

        let tx = self.build_base_swap_tx(&swap_params)?;
        let tx = self.chain.provider.set_transaction_fee(tx, fee, self.chain.chain_code).await?;

        let tx_hash = self.chain.provider.send_raw_transaction(tx, &key, None).await?;

        Ok(TransferResp::new(
            tx_hash,
            unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?,
        ))
    }

    fn build_base_swap_tx(
        &self,
        swap_params: &SwapParams,
    ) -> Result<TransactionRequest, crate::error::service::ServiceError> {
        let call_value = dexSwap1Call::try_from((swap_params, ChainCode::Ethereum))?;

        let tx = TransactionRequest::default()
            .from(swap_params.recipient)
            .to(swap_params.aggregator_addr)
            .with_input(call_value.abi_encode());

        // from token 如果是主币添加默认的币
        let tx =
            if swap_params.token_in.is_zero() { tx.with_value(swap_params.amount_in) } else { tx };

        Ok(tx)
    }

    pub async fn check_eth_balance(
        &self,
        from: &str,
        balance: U256,
        token: Option<&str>,
        transfer_amount: U256,
    ) -> Result<U256, ServiceError> {
        let cost_main = match token {
            Some(token) => {
                let token_balance = self.chain.balance(from, Some(token.to_string())).await?;
                if token_balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .chain_code(self.chain.chain_code.to_string())
                                .token_addr(token.to_string())
                                .value(transfer_amount.to_string())
                                .balance(token_balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("token balance is insufficient"),
                        ),
                    ))?;
                }
                balance
            }
            None => {
                if balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(from.to_string())
                                .chain_code(self.chain.chain_code.to_string())
                                .value(transfer_amount.to_string())
                                .balance(balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("main coin balance is insufficient"),
                        ),
                    ))?;
                }
                balance - transfer_amount
            }
        };
        Ok(cost_main)
    }
    fn fee_setting_form_meta_data(metadata: &str) -> Result<FeeSetting, ServiceError> {
        // {"gasLimit":23100,"baseFee":"208968188","priorityFee":"100000","maxFeePerGas":"300000000"},
        #[derive(Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        struct RequestMetaDataETH {
            pub base_fee: U256,
            pub priority_fee: U256,
            pub max_fee_per_gas: U256,
            pub gas_limit: U256,
        }
        let metadata = serde_json::from_str::<RequestMetaDataETH>(metadata).map_err(|e| {
            ServiceError::Parameter(String::from(
                "EthTx:fee_setting:the parameter meta data is invalid",
            ))
        })?;

        Ok(FeeSetting {
            base_fee: metadata.base_fee,
            max_priority_fee_per_gas: metadata.priority_fee,
            max_fee_per_gas: metadata.max_fee_per_gas,
            gas_limit: metadata.gas_limit,
        })
    }
    fn fee_setting_for_chain(
        &self,
        oracle: &GasOracle,
        gas: u64,
    ) -> Result<FeeSetting, ServiceError> {
        let mut setting = Self::fee_setting_from_gas_oracle(oracle, gas)?;
        if self.chain.chain_code == ChainCode::Scroll {
            setting.max_fee_per_gas =
                scroll_fee_cap(setting.base_fee, setting.max_priority_fee_per_gas)?;
        }
        Ok(setting)
    }
    async fn calculate_fee_setting_from_req(
        &self,
        params: &ApiTransferReq,
    ) -> Result<FeeSetting, ServiceError> {
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let from = params.base.from.as_str();
        let to = params.base.to.as_str();
        let token_key = params.base.token_address.clone();
        let token = token_key.to_chain_token_option();
        // 获取主币余额
        let eth_balance = self.chain.balance(&params.base.from, token.clone()).await?;
        tracing::info!(eth_balance=%eth_balance, "transfer ------------------- 13");
        // check balance
        let remain_balance = self
            .check_eth_balance(&params.base.from, eth_balance, token.as_deref(), transfer_amount)
            .await?;

        // 预估gas
        tracing::info!(eth_balance=%eth_balance, "transfer ------------------- 14");
        let transfer_opt = TransferOpt::new(from, to, transfer_amount, token.clone())?;
        tracing::info!(eth_balance=%eth_balance, "transfer ------------------- 15");
        let rc = self.chain.estimate_gas(transfer_opt).await?;

        tracing::info!("transfer -------------------{} 16", rc.consume);
        let gas_oracle = self.gas_oracle().await?;
        self.fee_setting_for_chain(&gas_oracle, rc.consume as u64)
    }
    fn uses_separate_l1_fee(&self) -> bool {
        matches!(self.chain.chain_code, ChainCode::Base | ChainCode::Scroll)
    }

    fn checked_total_fee(fee: &FeeSetting, additional: U256) -> Result<U256, ServiceError> {
        fee.gas_limit
            .checked_mul(fee.max_fee_per_gas)
            .and_then(|execution| execution.checked_add(additional))
            .ok_or_else(|| ServiceError::Parameter("EVM total fee overflow".to_string()))
    }

    async fn total_fee_for_transaction(
        &self,
        transaction: TransactionRequest,
        fee: &FeeSetting,
    ) -> Result<U256, ServiceError> {
        let transaction = if self.chain.chain_code == ChainCode::Scroll {
            scroll_fee_transaction(transaction, fee)?
        } else {
            transaction
        };
        let additional = self.chain.estimate_additional_fee(transaction).await?.unwrap_or_default();
        Self::checked_total_fee(fee, additional)
    }

    // Used only by Base/Scroll funding quotes, not by transfer/raw gas settings.
    async fn l1_quote_fee_setting(
        &self,
        transfer: TransferOpt,
    ) -> Result<FeeSetting, ServiceError> {
        let gas = self.chain.estimate_gas(transfer).await?;
        let oracle = self.gas_oracle().await?;
        self.fee_setting_for_chain(&oracle, gas.consume as u64)
    }

    async fn replacement_fee_setting(
        &self,
        base: &ApiBaseTransferReq,
        nonce: u64,
    ) -> Result<FeeSetting, ServiceError> {
        if base.token_address.is_contract() {
            return Err(ServiceError::Parameter(
                "replacement quote requires a native EVM asset".into(),
            ));
        }
        let mut setting = FeeSetting {
            base_fee: U256::ZERO,
            max_fee_per_gas: U256::ZERO,
            max_priority_fee_per_gas: U256::ZERO,
            gas_limit: U256::ZERO,
        };
        let chain_id = self.provider.chain_id().await?;
        // Validate the complete signed identity before any gas estimate.
        replacement::apply(base.metadata.as_deref(), chain_id, &base.from, nonce, &mut setting)?;
        let value = self.check_min_transfer(&base.value, base.decimals)?;
        let tx = TransferOpt::new(&base.from, &base.to, value, None)?.build_transaction()?;
        let client = RpcClient::new(
            &self.rpc_url_for_log,
            self.rpc_header_for_query.clone(),
            Some(std::time::Duration::from_secs(TIME_OUT)),
        )?;
        let gas = replacement::estimate_gas_latest(tx, nonce, |method, params| {
            let client = &client;
            async move {
                Ok(client
                    .invoke_request::<_, String>(
                        JsonRpcParams::default().method(method).params(params),
                    )
                    .await?)
            }
        })
        .await?;
        let gas = u64::try_from(gas)
            .map_err(|_| ServiceError::Parameter("replacement gas overflow".into()))?;
        setting = self.fee_setting_for_chain(&self.gas_oracle().await?, gas)?;
        replacement::apply(base.metadata.as_deref(), chain_id, &base.from, nonce, &mut setting)?;
        Ok(setting)
    }

    async fn native_fee_quote(&self, req: ApiBaseTransferReq) -> Result<U256, ServiceError> {
        if !self.uses_separate_l1_fee() && replacement::context(req.metadata.as_deref())?.is_none()
        {
            return Err(ServiceError::Parameter(
                "separate L1 fee quote is unsupported".to_string(),
            ));
        }
        let request = ApiTransferReq { base: req, password: String::new(), nonce: 0 };
        let amount = self.check_min_transfer(&request.base.value, request.base.decimals)?;
        let transfer = TransferOpt::new(
            &request.base.from,
            &request.base.to,
            amount,
            request.base.token_address.to_chain_token_option(),
        )?;
        let mut transaction = transfer.build_transaction()?;
        if let Some(context) = replacement::context(request.base.metadata.as_deref())? {
            let setting = self.replacement_fee_setting(&request.base, context.nonce).await?;
            transaction = transaction.with_nonce(context.nonce);
            return self.total_fee_for_transaction(transaction, &setting).await;
        }
        let setting = match self.chain.network {
            NetworkKind::Mainnet => {
                let metadata = request
                    .base
                    .metadata
                    .as_deref()
                    .and_then(|metadata| Self::fee_setting_form_meta_data(metadata).ok());
                match metadata {
                    Some(setting) => setting,
                    None => self.l1_quote_fee_setting(transfer).await?,
                }
            }
            _ => self.provider.get_fee(transaction.clone()).await?,
        };
        self.total_fee_for_transaction(transaction, &setting).await
    }

    async fn ensure_transfer_funded(
        &self,
        req: &ApiBaseTransferReq,
        value: U256,
        fee: U256,
    ) -> Result<(), ServiceError> {
        let native = self.chain.balance(&req.from, None).await?;
        if req.token_address.is_contract() {
            let token = req.token_address.to_chain_token_option();
            let balance = self.chain.balance(&req.from, token.clone()).await?;
            if balance < value {
                return Err(BusinessError::Chain(ChainError::insufficient_balance_with_detail(
                    crate::error::business::chain::InsufficientBalanceDetail::new()
                        .from_addr(req.from.clone())
                        .chain_code(req.chain_code.clone())
                        .token_addr(token.unwrap_or_default())
                        .value(value.to_string())
                        .balance(balance.to_string())
                        .need(value.to_string())
                        .reason("token balance is insufficient"),
                ))
                .into());
            }
        }
        let required = if req.token_address.is_contract() {
            fee
        } else {
            value.checked_add(fee).ok_or_else(|| {
                ServiceError::Parameter("EVM required balance overflow".to_string())
            })?
        };
        if native < required {
            return Err(BusinessError::Chain(ChainError::InsufficientFeeBalance).into());
        }
        Ok(())
    }

    /// ETH 从请求参数metadata中获取fee_setting
    /// 获取失败则重新计算fee_setting
    pub async fn build_fee_setting(
        &self,
        params: &ApiTransferReq,
    ) -> Result<FeeSetting, ServiceError> {
        if replacement::context(params.base.metadata.as_deref())?.is_some() {
            return self.replacement_fee_setting(&params.base, params.nonce).await;
        }
        if let Some(metadata) = &params.base.metadata {
            match Self::fee_setting_form_meta_data(metadata) {
                Ok(fee_setting) => {
                    // 优先使用参数的metadata 作为fee_setting
                    tracing::info!(
                        "transfer -------------------12.1:fee_setting from metadata:{}",
                        metadata
                    );
                    return Ok(fee_setting);
                }
                Err(err) => {
                    tracing::info!(
                        "transfer -------------------12.1:recalculate fee_setting:{}",
                        err.to_string()
                    );
                }
            }
        }
        self.calculate_fee_setting_from_req(params).await
    }
}

#[async_trait::async_trait]
impl Oracle for EthTx {
    async fn gas_oracle_with_ctx(
        &self,
        ctx: &crate::context::Context,
    ) -> Result<GasOracle, ServiceError> {
        let _ctx = ctx;
        let gas_oracle =
            self.ctx.get_global_backend_api().gas_oracle(&self.chain.chain_code.to_string()).await;
        tracing::info!("gas_oracle: {:?}", gas_oracle);
        match gas_oracle {
            Ok(gas_oracle) => Ok(gas_oracle),
            Err(err) => {
                tracing::error!(error = ?err, "gas_oracle failed");
                // unit is wei need to gwei
                let eth_fee = self.chain.provider.get_default_fee().await?;

                let propose = eth_fee.base_fee + eth_fee.priority_fee_per_gas;
                let propose = unit::format_to_string(propose, eth::consts::ETH_GWEI)?;
                let base = unit::format_to_string(eth_fee.base_fee, eth::consts::ETH_GWEI)?;

                Ok(GasOracle {
                    safe_gas_price: None,
                    propose_gas_price: Some(propose),
                    fast_gas_price: None,
                    suggest_base_fee: Some(base),
                    gas_used_ratio: None,
                })
            }
        }
    }

    async fn gas_oracle(&self) -> Result<GasOracle, ServiceError> {
        let gas_oracle =
            self.ctx.get_global_backend_api().gas_oracle(&self.chain.chain_code.to_string()).await;
        tracing::info!("gas_oracle: {:?}", gas_oracle);
        if let Ok(gas_oracle) = gas_oracle {
            return Ok(gas_oracle);
        }
        tracing::warn!(
            "gas_oracle: fallback to on-chain fee because backend API is not configured or failed"
        );
        self.default_gas_oracle().await
    }

    async fn default_gas_oracle(&self) -> Result<GasOracle, ServiceError> {
        let eth_fee = self.chain.provider.get_default_fee().await?;

        let propose = eth_fee.base_fee + eth_fee.priority_fee_per_gas;
        let propose = unit::format_to_string(propose, eth::consts::ETH_GWEI)?;
        let base = unit::format_to_string(eth_fee.base_fee, eth::consts::ETH_GWEI)?;

        let gas_oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some(propose),
            fast_gas_price: None,
            suggest_base_fee: Some(base),
            gas_used_ratio: None,
        };

        Ok(gas_oracle)
    }
}

#[async_trait::async_trait]
impl Tx for EthTx {
    fn rpc_endpoint_for_log(&self) -> Option<String> {
        Some(self.rpc_url_for_log.clone())
    }

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
        let n = self.provider.nonce(addr).await?;
        Ok(n)
    }

    async fn replacement_latest_nonce(&self, address: &str) -> Result<u64, ServiceError> {
        let client = RpcClient::new(
            &self.rpc_url_for_log,
            self.rpc_header_for_query.clone(),
            Some(std::time::Duration::from_secs(TIME_OUT)),
        )?;
        replacement::latest_nonce(address, |method, params| {
            let client = &client;
            async move {
                Ok(client
                    .invoke_request::<_, String>(
                        JsonRpcParams::default().method(method).params(params),
                    )
                    .await?)
            }
        })
        .await
    }

    async fn block_num(&self) -> Result<u64, Error> {
        self.chain.block_num().await
    }

    async fn query_tx_res(&self, hash: &str) -> Result<Option<QueryTransactionResult>, Error> {
        self.chain.query_tx_res(hash).await
    }

    async fn finalized_nonce_conflict(
        &self,
        chain_code: &str,
        address: &str,
        nonce: i32,
        original_hash: &str,
    ) -> Result<
        Option<wallet_database::repositories::api_wallet::collect::FinalizedCollectNonceConflict>,
        ServiceError,
    > {
        let client = RpcClient::new(
            &self.rpc_url_for_log,
            self.rpc_header_for_query.clone(),
            Some(std::time::Duration::from_secs(TIME_OUT)),
        )?;
        tokio::time::timeout(
            std::time::Duration::from_secs(TIME_OUT),
            nonce_conflict::probe(chain_code, address, nonce, original_hash, |method, params| {
                let client = &client;
                async move {
                    Ok(client
                        .invoke_request::<_, Value>(
                            JsonRpcParams::default().method(method).params(params),
                        )
                        .await?)
                }
            }),
        )
        .await
        .map_err(|_| ServiceError::Timeout)?
    }

    async fn replacement_receipt(
        &self,
        hash: &str,
        address: &str,
        nonce: u64,
    ) -> Result<Option<crate::domain::api_wallet::adapter::tx::ReplacementReceipt>, ServiceError>
    {
        let client = RpcClient::new(
            &self.rpc_url_for_log,
            self.rpc_header_for_query.clone(),
            Some(std::time::Duration::from_secs(TIME_OUT)),
        )?;
        tokio::time::timeout(
            std::time::Duration::from_secs(TIME_OUT),
            replacement_receipt::probe(
                hash,
                address,
                nonce,
                self.uses_separate_l1_fee(),
                |method, params| {
                    let client = &client;
                    async move {
                        Ok(client
                            .invoke_request::<_, Value>(
                                JsonRpcParams::default().method(method).params(params),
                            )
                            .await?)
                    }
                },
            ),
        )
        .await
        .map_err(|_| ServiceError::Timeout)?
    }

    async fn query_tx_seen_on_node(&self, hash: &str) -> Result<bool, ServiceError> {
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        let rpc_client =
            RpcClient::new(&self.rpc_url_for_log, self.rpc_header_for_query.clone(), timeout)?;
        crate::domain::chain::tx_visibility::evm_transaction_seen(
            &rpc_client,
            hash,
            crate::domain::chain::tx_visibility::EvmLookupContract::ApiWallet(
                &self.rpc_url_for_log,
            ),
        )
        .await
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
        let res = self.chain.black_address(token, owner).await?;
        Ok(res)
    }

    /// Legacy direct-sign-and-send entry point, retained for compatibility.
    /// Production EVM flows should use `build_transfer_raw`, persist the signed raw
    /// transaction and hash, then broadcast through their durable flow owner.
    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        tracing::info!("transfer ------------------- 11:");
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let from = params.base.from.as_str();
        let to = params.base.to.as_str();
        tracing::info!(from=%from,to=%to,value=%transfer_amount, "transfer ------------------- 12");

        let fee_setting = self.build_fee_setting(params).await?;

        let fee = fee_setting.transaction_fee();
        tracing::info!(fee=%fee, base_fee=%fee_setting.base_fee, "transfer ------------------- 16");
        let token_key = params.base.token_address.clone();
        let transfer_opt =
            TransferOpt::new(from, to, transfer_amount, token_key.to_chain_token_option())?;
        let tx_hash = self
            .chain
            .exec_transaction(transfer_opt, fee_setting, private_key, Some(params.nonce))
            .await?;

        tracing::info!("transfer ------------------- 16: {tx_hash}");
        Ok(TransferResp::new(tx_hash, unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?))
    }

    async fn build_transfer_raw(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), crate::error::service::ServiceError> {
        tracing::info!("transfer ------------------- 11:");
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let from = params.base.from.as_str();
        let to = params.base.to.as_str();
        tracing::info!(from=%from,to=%to,value=%transfer_amount, "transfer ------------------- 12");

        let fee_setting = self.build_fee_setting(params).await?;

        let token_key = params.base.token_address.clone();
        let transfer_opt =
            TransferOpt::new(from, to, transfer_amount, token_key.to_chain_token_option())?;
        let scoped = replacement::context(params.base.metadata.as_deref())?.is_some();
        let mut trans_req = transfer_opt.build_transaction()?;
        if scoped {
            trans_req = trans_req.with_nonce(params.nonce);
        }
        let mut fee_setting = match self.chain.network {
            NetworkKind::Mainnet => fee_setting,
            _ if !scoped => self.provider.get_fee(trans_req.clone()).await?,
            _ => fee_setting,
        };
        // Applies only to explicit scoped replacement metadata, after current quote.
        if params.base.metadata.as_deref().is_some_and(|m| m.contains("collectReplacement")) {
            replacement::apply(
                params.base.metadata.as_deref(),
                self.provider.chain_id().await?,
                &params.base.from,
                params.nonce,
                &mut fee_setting,
            )?;
        }

        // Automatic API Wallet raw builds must fund the signed EIP-1559
        // maximum, because nodes validate balance against
        // gas_limit * max_fee_per_gas.
        let fee = if self.uses_separate_l1_fee() || scoped {
            let total = self.total_fee_for_transaction(trans_req.clone(), &fee_setting).await?;
            self.ensure_transfer_funded(&params.base, transfer_amount, total).await?;
            total
        } else {
            fee_setting.gas_limit * fee_setting.max_fee_per_gas
        };

        let transfer_params = self
            .provider
            .set_transaction_fee(trans_req, fee_setting, self.chain.chain_code)
            .await?;
        let (raw_tx, tx_hash) = self
            .provider
            .build_signed_raw_transaction(transfer_params, &private_key, Some(params.nonce))
            .await?;
        let local_raw_hash = Self::evm_raw_tx_hash_hex(&raw_tx);
        tracing::info!(
            chain_code = %self.chain.chain_code,
            rpc = %self.rpc_url_for_log,
            from = %from,
            to = %to,
            nonce = %params.nonce,
            build_tx_hash = %tx_hash,
            local_raw_hash = %local_raw_hash,
            raw_len = raw_tx.len(),
            raw_prefix = %Self::short_hex(&raw_tx, 16),
            "evm build_transfer_raw completed"
        );
        if !tx_hash.eq_ignore_ascii_case(&local_raw_hash) {
            tracing::warn!(
                chain_code = %self.chain.chain_code,
                rpc = %self.rpc_url_for_log,
                nonce = %params.nonce,
                build_tx_hash = %tx_hash,
                local_raw_hash = %local_raw_hash,
                "evm build_transfer_raw hash mismatch between provider and local keccak(raw)"
            );
        }
        //         let fee = match self.chain.network {
        //     NetworkKind::Mainnet => fee_setting,
        //     _ => self.provider.get_fee(params.clone()).await?,
        // };
        // raw = self
        // .provider
        // .set_transaction_fee(raw, fee, self.chain_code)
        // .await?;

        Ok((
            tx_hash,
            RawTx::Evm(raw_tx, fee),
            unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?,
        ))
    }

    async fn broadcast_transfer(
        &self,
        raw: RawTx,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        if let RawTx::Evm(raw, fee) = raw {
            let local_raw_hash = Self::evm_raw_tx_hash_hex(&raw);
            tracing::info!(
                chain_code = %self.chain.chain_code,
                rpc = %self.rpc_url_for_log,
                local_raw_hash = %local_raw_hash,
                raw_len = raw.len(),
                raw_prefix = %Self::short_hex(&raw, 16),
                "evm broadcast_transfer submitting raw transaction"
            );
            let tx_hash = self.provider.broadcast_raw_transaction(&raw).await?;
            tracing::info!(
                chain_code = %self.chain.chain_code,
                rpc = %self.rpc_url_for_log,
                local_raw_hash = %local_raw_hash,
                provider_tx_hash = %tx_hash,
                "evm broadcast_transfer rpc returned tx hash"
            );
            if !tx_hash.eq_ignore_ascii_case(&local_raw_hash) {
                tracing::warn!(
                    chain_code = %self.chain.chain_code,
                    rpc = %self.rpc_url_for_log,
                    local_raw_hash = %local_raw_hash,
                    provider_tx_hash = %tx_hash,
                    "evm broadcast_transfer provider tx hash differs from local keccak(raw)"
                );
            }
            Ok(TransferResp::new(tx_hash, unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?))
        } else {
            Err(ServiceError::Business(BusinessError::Chain(ChainError::InvalidRawTx)))
        }
    }

    async fn estimate_native_fee_wei(&self, req: ApiBaseTransferReq) -> Result<U256, ServiceError> {
        self.native_fee_quote(req).await
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
        let value = unit::convert_to_u256(&req.value, req.decimals)?;
        let token_key = req.token_address.clone();
        let token = token_key.to_chain_token_option();
        let balance = self.chain.balance(&req.from, token.clone()).await?;
        if balance < value {
            tracing::error!(
                "estimate_fee: amount is less than value, balance: {}, value: {}",
                balance,
                value
            );
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                    crate::error::business::chain::InsufficientBalanceDetail::new()
                        .from_addr(req.from.clone())
                        .chain_code(self.chain.chain_code.to_string())
                        .token_addr(token.clone().unwrap_or_default())
                        .value(value.to_string())
                        .balance(balance.to_string())
                        .need(value.to_string())
                        .reason("balance is less than transfer amount"),
                ),
            ))?;
        }

        let gas_oracle = self.gas_oracle().await?;
        let params = TransferOpt::new(&req.from, &req.to, value, token)?;
        let tx = params.build_transaction()?;
        let additional =
            if self.uses_separate_l1_fee() && self.chain.chain_code != ChainCode::Scroll {
                self.chain.estimate_additional_fee(tx.clone()).await?
            } else {
                None
            };
        let fee = self.chain.estimate_gas(params).await?;
        let details = FeeDetails::for_chain(gas_oracle, fee.consume, self.chain.chain_code)?;
        let additional = if self.chain.chain_code == ChainCode::Scroll {
            self.chain
                .estimate_additional_fee(scroll_fee_transaction(
                    tx,
                    &details.reserve_fee_setting()?,
                )?)
                .await?
        } else {
            additional
        };
        let details = match additional {
            Some(additional) => {
                let details = details.with_additional_native_fee(additional)?;
                if self.chain.chain_code == ChainCode::Base {
                    details.with_base_l1_fee_margin(additional)?
                } else {
                    details
                }
            }
            None => details,
        };
        let fee = details.to_resp(token_currency, currency)?;

        let res = wallet_utils::serde_func::serde_to_string(&fee)?;
        Ok(res)
    }

    // async fn approve(
    //     &self,
    //     req: &ApproveReq,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let approve = Approve::new(&req.from, &req.spender, value, &req.contract)?;
    //
    //     // 使用默认的手续费配置
    //     let gas_price = self.chain.provider.gas_price().await?;
    //     let fee_setting = FeeSetting::new_with_price(gas_price);
    //
    //     let fee = fee_setting.transaction_fee();
    //
    //     // exec tx
    //     let tx_hash = self.chain.exec_transaction(approve, fee_setting, key, None).await?;
    //
    //     Ok(TransferResp::new(tx_hash, unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?))
    // }

    // async fn approve_fee(
    //     &self,
    //     req: &ApproveReq,
    //     value: U256,
    //     main_symbol: &str,
    // ) -> Result<String, ServiceError> {
    //     let currency = {
    //         let currency = crate::app_state::APP_STATE.read().await;
    //         currency.currency().to_string()
    //     };
    //
    //     let token_currency =
    //         TokenCurrencyGetter::get_currency(&currency, &req.chain_code, main_symbol, None)
    //             .await?;
    //
    //     let approve = Approve::new(&req.from, &req.spender, value, &req.contract)?;
    //
    //     let fee = self.chain.estimate_gas(approve).await?;
    //
    //     let gas_oracle = self.default_gas_oracle().await?;
    //     let fee =
    //         FeeDetails::try_from((gas_oracle, fee.consume))?.to_resp(token_currency, &currency);
    //
    //     let res = wallet_utils::serde_func::serde_to_string(&fee)?;
    //     Ok(res)
    // }

    // async fn allowance(
    //     &self,
    //     from: &str,
    //     token: &str,
    //     spender: &str,
    // ) -> Result<U256, ServiceError> {
    //     let approve = Allowance::new(from, token, spender)?;
    //     let amount = self.chain.eth_call::<_, U256>(approve).await?;
    //     Ok(amount)
    // }

    // async fn swap_quote(
    //     &self,
    //     req: &QuoteReq,
    //     quote_resp: &AggQuoteResp,
    //     symbol: &str,
    // ) -> Result<(U256, String, String), ServiceError> {
    //     let amount_out = quote_resp.amount_out_u256()?;
    //
    //     // 考虑滑点计算最小金额
    //     let min_amount_out =
    //         calc_slippage(amount_out, req.get_slippage(quote_resp.default_slippage));
    //
    //     let currency = {
    //         let currency = crate::app_state::APP_STATE.read().await;
    //         currency.currency().to_string()
    //     };
    //
    //     let token_currency =
    //         TokenCurrencyGetter::get_currency(&currency, &req.chain_code, symbol, None).await?;
    //
    //     let swap_params = SwapParams {
    //         aggregator_addr: req.aggregator_address()?,
    //         amount_in: req.amount_in_u256()?,
    //         min_amount_out,
    //         recipient: wallet_utils::address::parse_eth_address(&req.recipient)?,
    //         token_in: SwapParams::eth_parse_or_zero_addr(&req.token_in.token_addr)?,
    //         token_out: SwapParams::eth_parse_or_zero_addr(&req.token_out.token_addr)?,
    //         dex_router: quote_resp.dex_route_list.clone(),
    //         allow_partial_fill: req.allow_partial_fill,
    //     };
    //
    //     let resp = self.estimate_swap(swap_params).await?;
    //
    //     let gas_oracle = self.default_gas_oracle().await?;
    //     let fee = FeeDetails::try_from((gas_oracle, resp.consumer.gas_limit.to::<i64>()))?
    //         .to_resp(token_currency, &currency);
    //
    //     // 消耗的资源
    //     let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
    //     // 具体的手续费结构
    //     let fee = wallet_utils::serde_func::serde_to_string(&fee)?;
    //
    //     Ok((resp.amount_out, consumer, fee))
    // }

    // async fn swap(
    //     &self,
    //     req: &SwapReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    // ) -> Result<TransferResp, ServiceError> {
    //     let swap_params = SwapParams::try_from(req)?;
    //     self.swap_base_transfer(&swap_params, fee, key).await
    // }

    // async fn deposit_fee(
    //     &self,
    //     req: DepositReq,
    //     main_coin: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     let currency = {
    //         let currency = crate::app_state::APP_STATE.read().await;
    //         currency.currency().to_string()
    //     };
    //
    //     let token_currency =
    //         TokenCurrencyGetter::get_currency(&currency, &req.chain_code, &main_coin.symbol, None)
    //             .await?;
    //     let value = wallet_utils::unit::convert_to_u256(&req.amount, main_coin.decimals)?;
    //
    //     let approve = Deposit::new(&req.from, &req.token, value)?;
    //
    //     let resource = self.chain.estimate_gas(approve).await?;
    //
    //     let gas_oracle = self.default_gas_oracle().await?;
    //     let fee = FeeDetails::try_from((gas_oracle, resource.consume))?
    //         .to_resp(token_currency, &currency);
    //
    //     let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&fee)?;
    //
    //     Ok((consumer, fee))
    // }

    // async fn deposit(
    //     &self,
    //     req: &DepositReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let approve = Deposit::new(&req.from, &req.token, value)?;
    //
    //     // 使用默认的手续费配置
    //     let fee_setting = pare_fee_setting(fee.as_str())?;
    //     let transfer_fee = fee_setting.transaction_fee();
    //
    //     // exec tx
    //     let tx_hash = self.chain.exec_transaction(approve, fee_setting, key, None).await?;
    //
    //     Ok(TransferResp::new(
    //         tx_hash,
    //         unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?,
    //     ))
    // }

    // async fn withdraw_fee(
    //     &self,
    //     req: WithdrawReq,
    //     main_coin: &CoinEntity,
    // ) -> Result<(String, String), ServiceError> {
    //     let currency = {
    //         let currency = crate::app_state::APP_STATE.read().await;
    //         currency.currency().to_string()
    //     };
    //
    //     let token_currency =
    //         TokenCurrencyGetter::get_currency(&currency, &req.chain_code, &main_coin.symbol, None)
    //             .await?;
    //
    //     let value = wallet_utils::unit::convert_to_u256(&req.amount, main_coin.decimals)?;
    //
    //     let withdraw = Withdraw::new(&req.from, &req.token, value)?;
    //
    //     let resource = self.chain.estimate_gas(withdraw).await?;
    //     let gas_oracle = self.default_gas_oracle().await?;
    //     let fee = FeeDetails::try_from((gas_oracle, resource.consume))?
    //         .to_resp(token_currency, &currency);
    //
    //     let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&fee)?;
    //
    //     Ok((consumer, fee))
    // }

    // async fn withdraw(
    //     &self,
    //     req: &WithdrawReq,
    //     fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let withdraw = Withdraw::new(&req.from, &req.token, value)?;
    //
    //     // 使用默认的手续费配置
    //     let fee_setting = pare_fee_setting(fee.as_str())?;
    //     let transfer_fee = fee_setting.transaction_fee();
    //
    //     // exec tx
    //     let tx_hash = self.chain.exec_transaction(withdraw, fee_setting, key, None).await?;
    //
    //     Ok(TransferResp::new(
    //         tx_hash,
    //         unit::format_to_string(transfer_fee, eth::consts::ETH_DECIMAL)?,
    //     ))
    // }
}

// #[async_trait::async_trait]
// impl Multisig for EthTx {
//     async fn multisig_address(
//         &self,
//         account: &MultisigAccountEntity,
//         member: &MultisigMemberEntities,
//     ) -> Result<FetchMultisigAddressResp, ServiceError> {
//         let params = MultisigAccountOpt::new(&account.initiator_addr, account.threshold)?
//             .with_nonce()
//             .with_owners(member.get_owner_str_vec())?;
//         Ok(self.chain.multisig_account(params).await?)
//     }
//
//     async fn deploy_multisig_account(
//         &self,
//         account: &MultisigAccountEntity,
//         member: &MultisigMemberEntities,
//         fee_setting: Option<String>,
//         key: ChainPrivateKey,
//     ) -> Result<(String, String), ServiceError> {
//         let params = MultisigAccountOpt::new(&account.initiator_addr, account.threshold)?
//             .set_nonce(&account.salt)?
//             .with_owners(member.get_owner_str_vec())?;
//
//         let fee_setting: EthereumFeeDetails = serde_func::serde_from_str(&fee_setting.unwrap())?;
//         let fee_setting = FeeSetting::try_from(fee_setting)?;
//
//         // check transaction_fee
//         let balance = self.chain.balance(&account.initiator_addr, None).await?;
//         if balance < fee_setting.transaction_fee() {
//             return Err(crate::error::business::BusinessError::Chain(
//                 crate::error::business::chain::ChainError::InsufficientFeeBalance,
//             ))?;
//         }
//
//         let tx_hash = self.chain.exec_transaction(params, fee_setting, key, None).await?;
//         Ok((tx_hash, "".to_string()))
//     }
//
//     async fn deploy_multisig_fee(
//         &self,
//         account: &MultisigAccountEntity,
//         member: MultisigMemberEntities,
//         main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         let currency_lock = crate::app_state::APP_STATE.read().await;
//         let currency = currency_lock.currency();
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &account.chain_code, main_symbol, None)
//                 .await?;
//
//         let owner = member.get_owner_str_vec();
//         let params = MultisigAccountOpt::new(&account.initiator_addr, account.threshold)?
//             .with_nonce()
//             .with_owners(owner)?;
//
//         let gas_limit = self.chain.estimate_gas(params).await?;
//
//         let gas_oracle = self.gas_oracle().await?;
//
//         let fee = FeeDetails::try_from((gas_oracle, gas_limit.consume))?
//             .to_resp(token_currency, currency);
//         Ok(wallet_utils::serde_func::serde_to_string(&fee)?)
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
//         Ok("".to_string())
//     }
//
//     async fn build_multisig_with_account(
//         &self,
//         req: &TransferParams,
//         _account: &MultisigAccountEntity,
//         assets: &ApiAssetsEntity,
//         _key: ChainPrivateKey,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         let decimal = assets.decimals;
//         let token = assets.token_address();
//
//         let value = self.check_min_transfer(&req.value, decimal)?;
//         let balance = self.chain.balance(&req.from, token.clone()).await?;
//         let _ = self.check_eth_balance(&req.from, balance, token.as_deref(), value).await?;
//
//         let params = MultisigTransferOpt::new(&req.from, &req.to, value)?.with_token(token)?;
//
//         Ok(self.chain.build_multisig_tx(params).await?)
//     }
//
//     async fn build_multisig_with_permission(
//         &self,
//         _req: &TransferParams,
//         _p: &PermissionEntity,
//         _coin: &CoinEntity,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         Err(crate::error::business::BusinessError::Permission(
//             crate::error::business::permission::PermissionError::UnSupportPermissionChain,
//         )
//         .into())
//     }
//
//     async fn sign_fee(
//         &self,
//         _account: &MultisigAccountEntity,
//         _address: &str,
//         _raw_data: &str,
//         _main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         Ok(" ".to_string())
//     }
//
//     async fn sign_multisig_tx(
//         &self,
//         _account: &MultisigAccountEntity,
//         _address: &str,
//         key: ChainPrivateKey,
//         raw_data: &str,
//     ) -> Result<MultisigSignResp, ServiceError> {
//         use std::str::FromStr as _;
//         let operate = eth::operations::MultisigPayloadOpt::from_str(raw_data)?;
//         Ok(operate.sign_message(key)?)
//     }
//
//     async fn estimate_multisig_fee(
//         &self,
//         queue: &MultisigQueueEntity,
//         coin: &CoinEntity,
//         _backend: &BackendApi,
//         sign_list: Vec<String>,
//         main_symbol: &str,
//     ) -> Result<String, ServiceError> {
//         let currency = crate::app_state::APP_STATE.read().await;
//         let currency = currency.currency();
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &queue.chain_code, main_symbol, None)
//                 .await?;
//
//         let value = unit::convert_to_u256(&queue.value, coin.decimals)?;
//         let multisig_account =
//             MultisigDomain::account_by_address(&queue.from_addr, true, &pool).await?;
//
//         let gas_oracle = self.gas_oracle().await?;
//
//         let params = MultisigTransferOpt::new(&queue.from_addr, &queue.to_addr, value)?
//             .with_token(coin.token_address())?
//             .exec_params(
//                 &multisig_account.initiator_addr,
//                 queue.raw_data.clone(),
//                 sign_list.join(""),
//             )?;
//
//         let fee = self.chain.estimate_gas(params).await?;
//         let fee =
//             FeeDetails::try_from((gas_oracle, fee.consume))?.to_resp(token_currency, currency);
//
//         Ok(wallet_utils::serde_func::serde_to_string(&fee)?)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore as _;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    fn start_chain_id_rpc(
        expected_calls: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let worker = thread::spawn(move || {
            for _ in 0..expected_calls {
                let (mut socket, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0_u8; 4096];
                let (body_start, length) = loop {
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0, "incomplete HTTP headers");
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < body_start + length {
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0, "incomplete HTTP body");
                    bytes.extend_from_slice(&buf[..count]);
                }
                let request: Value =
                    serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
                let method = request["method"].as_str().unwrap().to_string();
                recorded.lock().unwrap().push(method);
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "result": match request["method"].as_str().unwrap() {
                        "eth_getBalance" => "0x3635c9adc5dea00000".to_string(),
                        "eth_call" => format!("0x{:064x}", 1),
                        _ => "0x1".to_string(),
                    }
                })
                .to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
        });
        (url, calls, worker)
    }

    fn insufficient_balance_chain_code(err: ServiceError) -> Option<String> {
        match err {
            ServiceError::Business(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::InsufficientBalance(detail),
            )) => detail.chain_code,
            _ => None,
        }
    }

    #[test]
    fn metadata_free_fee_uses_propose_total_instead_of_adding_fast_total() {
        let oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some("10".to_string()),
            fast_gas_price: Some("12".to_string()),
            suggest_base_fee: Some("8".to_string()),
            gas_used_ratio: None,
        };

        let fee = EthTx::fee_setting_from_gas_oracle(&oracle, 21_000).unwrap();

        assert_eq!(fee.base_fee, U256::from(8_000_000_000_u64));
        assert_eq!(fee.max_priority_fee_per_gas, U256::from(2_000_000_000_u64));
        assert_eq!(fee.max_fee_per_gas, U256::from(12_000_000_000_u64));
        assert_eq!(fee.gas_limit, U256::from(21_000_u64));
    }

    #[test]
    fn metadata_free_fee_rejects_oracle_without_base_fee() {
        let oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some("10".to_string()),
            fast_gas_price: Some("10".to_string()),
            suggest_base_fee: None,
            gas_used_ratio: None,
        };

        let err = EthTx::fee_setting_from_gas_oracle(&oracle, 21_000).unwrap_err();
        assert!(matches!(
            err,
            ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::GasOracle
            ))
        ));
    }

    #[tokio::test]
    async fn api_wallet_balance_error_reports_actual_evm_chain() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let tx = EthTx::new_for_test(
            ctx,
            ChainCode::Polygon,
            "https://example.invalid",
            NetworkKind::Mainnet,
            None,
        )
        .expect("EVM adapter should be creatable without network access");

        let err = tx
            .check_eth_balance(
                "0x0000000000000000000000000000000000000001",
                U256::ZERO,
                None,
                U256::from(1),
            )
            .await
            .unwrap_err();

        assert_eq!(insufficient_balance_chain_code(err).as_deref(), Some("polygon"));
    }

    #[tokio::test]
    async fn standard_wallet_balance_error_reports_actual_evm_chain() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let tx = EthTx::new_for_test(
            ctx,
            ChainCode::Base,
            "https://example.invalid",
            NetworkKind::Mainnet,
            None,
        )
        .expect("EVM adapter should be creatable without network access");

        let err = crate::domain::chain::transaction::ChainTransDomain::check_eth_balance(
            "0x0000000000000000000000000000000000000001",
            U256::ZERO,
            None,
            &tx.chain,
            U256::from(1),
        )
        .await
        .unwrap_err();

        assert_eq!(insufficient_balance_chain_code(err).as_deref(), Some("base"));
    }

    #[tokio::test]
    async fn added_evm_chains_build_signed_transfer_offline() {
        let chains = [
            ChainCode::Polygon,
            ChainCode::Avalanche,
            ChainCode::Fantom,
            ChainCode::Arbitrum,
            ChainCode::Base,
            ChainCode::Scroll,
        ];
        let (rpc_url, calls, worker) = start_chain_id_rpc(chains.len() + 4);
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let mut private_key = [0_u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut private_key);
        let private_key_hex = hex::encode(private_key);
        let signer: alloy::signers::local::PrivateKeySigner =
            private_key_hex.parse().expect("runtime-generated secp256k1 key must parse");
        let from = signer.address().to_string();
        let private_key = ChainPrivateKey::from(private_key_hex);

        for chain_code in chains {
            let tx = EthTx::new_for_test(ctx, chain_code, &rpc_url, NetworkKind::Mainnet, None)
                .expect("EVM adapter should be creatable with a local fake RPC");
            let mut base = ApiBaseTransferReq::new(
                &from,
                "0x0000000000000000000000000000000000000002",
                "1",
                &chain_code.to_string(),
            );
            base.metadata = Some(
                json!({
                    "baseFee": "1",
                    "priorityFee": "1",
                    "maxFeePerGas": "3",
                    "gasLimit": "21000"
                })
                .to_string(),
            );
            let request = ApiTransferReq { base, password: String::new(), nonce: 7 };

            let (hash, raw, _) = tx
                .build_transfer_raw(&request, private_key.clone())
                .await
                .unwrap_or_else(|err| panic!("{chain_code} must build and sign offline: {err}"));

            let RawTx::Evm(raw, fee) = raw else {
                panic!("{chain_code} must produce EVM raw transaction bytes");
            };
            assert!(!raw.is_empty(), "{chain_code} raw transaction must not be empty");
            assert_eq!(hash, EthTx::evm_raw_tx_hash_hex(&raw), "{chain_code}");
            assert_eq!(
                fee,
                U256::from(
                    63_000_u64
                        + u64::from(matches!(chain_code, ChainCode::Base | ChainCode::Scroll))
                ),
                "{chain_code}"
            );
        }

        worker.join().unwrap();
        assert_eq!(
            calls.lock().unwrap().iter().filter(|m| m.as_str() == "eth_chainId").count(),
            chains.len()
        );
    }
}

#[cfg(test)]
#[path = "eth_tx/fee_tests.rs"]
mod fee_tests;

#[cfg(test)]
#[path = "eth_tx/replacement_receipt_tests.rs"]
mod receipt_tests;
