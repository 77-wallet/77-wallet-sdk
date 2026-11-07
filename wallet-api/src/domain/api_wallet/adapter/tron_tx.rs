use crate::{
    context::Context,
    domain::{
        api_wallet::adapter::{
            TIME_OUT,
            tx::{FeeEstimateSnapshot, RawTx, Tx},
        },
        chain::{
            TransferResp,
            swap::{
                EstimateSwapResult,
                evm_swap::{SwapParams, dexSwap1Call},
            },
        },
        coin::TokenCurrencyGetter,
        multisig::MultisigQueueDomain,
    },
    error::{
        business::{BusinessError, chain::ChainError},
        service::ServiceError,
    },
    request::{
        api_wallet::trans::{ApiBaseTransferReq, ApiTransferReq},
        transaction::WithdrawReq,
    },
    response_vo::{TransferParams, TronFeeDetails},
};
use alloy::{
    primitives::U256,
    sol_types::{SolCall, SolValue},
};
use std::collections::HashMap;
use wallet_chain_interact::{
    BillResourceConsume, Error, QueryTransactionResult, abi_encode_u256, tron,
    tron::{
        TronChain,
        operations::{
            TronConstantOperation as _, TronTxOperation,
            contract::{TriggerContractParameter, WarpContract},
            transfer::{ContractTransferOpt, TransferOpt},
        },
        params::ResourceConsumer,
        protocol::account::AccountResourceDetail,
    },
    types::{ChainPrivateKey, MultisigTxResp},
};
use wallet_database::entities::asset_token_key::AssetTokenKey;
use wallet_transport::client::HttpClient;
use wallet_types::chain::chain::ChainCode;
use wallet_utils::unit;

pub(crate) struct TronTx {
    chain: TronChain,
    rpc_url: String,
    ctx: &'static Context,
}

impl TronTx {
    const API_WITHDRAW_RAW_EXPIRATION_SECS: u64 = 3 * 60;

    pub(crate) fn chain(&self) -> &TronChain {
        &self.chain
    }

    fn has_enough_main_balance_for_token_fee(balance: i64, transaction_fee: i64) -> bool {
        transaction_fee <= 0 || balance >= transaction_fee
    }

    fn log_tron_token_resource_estimate(owner_address: &str, consumer: &ResourceConsumer) {
        let energy = consumer.energy.as_ref();
        let energy_limit = energy.map_or(0, |item| item.limit);
        let energy_required = energy.map_or(0, |item| item.consumer);
        let energy_price = energy.map_or(0, |item| item.price);
        let energy_shortfall = consumer.need_extra_energy();
        let energy_fee_sun = energy.map_or(0, |item| item.fee());
        let bandwidth_shortfall = consumer.need_extra_bandwidth();
        let bandwidth_fee_sun = consumer.bandwidth.fee();

        tracing::info!(
            owner_address,
            transaction_fee_sun = consumer.transaction_fee_i64(),
            transaction_fee_trx = %consumer.transaction_fee(),
            energy_limit,
            energy_required,
            energy_price,
            energy_shortfall,
            energy_fee_sun,
            energy_covered = consumer.act_energy(),
            bandwidth_limit = consumer.bandwidth.limit,
            bandwidth_required = consumer.bandwidth.consumer,
            bandwidth_price = consumer.bandwidth.price,
            bandwidth_shortfall,
            bandwidth_fee_sun,
            bandwidth_covered = consumer.act_bandwidth(),
            extra_fee_sun = consumer.extra_fee,
            "transfer ---------------- 12.2"
        );
    }

    fn is_empty_tx_query_payload(err_msg: &str) -> bool {
        crate::infrastructure::chain_rpc_guard::is_empty_chain_rpc_response_message(err_msg)
    }

    fn confirmed_resource_consume(
        net_usage: Option<u64>,
        energy_usage: Option<u64>,
        energy_usage_total: Option<u64>,
    ) -> BillResourceConsume {
        BillResourceConsume::new_tron(
            net_usage.unwrap_or_default(),
            energy_usage_total.or(energy_usage).unwrap_or_default(),
        )
    }

    #[cfg(test)]
    pub fn new_for_test(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        // let network = wallet_types::chain::network::NetworkKind::Mainnet;
        Self::new_with_ctx(ctx, rpc_url, header_opt)
    }

    pub fn new_with_ctx(
        ctx: &'static Context,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<Self, wallet_chain_interact::Error> {
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        let http_client = HttpClient::new(rpc_url, header_opt, timeout)?;
        let provider = tron::Provider::new(http_client)?;

        let tron_chain = TronChain::new(provider)?;
        Ok(Self { chain: tron_chain, rpc_url: rpc_url.to_string(), ctx })
    }
    // 构建多签交易
    pub(super) async fn build_build_tx(
        &self,
        req: &TransferParams,
        token: Option<String>,
        value: U256,
        threshold: i64,
        permission_id: Option<i64>,
    ) -> Result<MultisigTxResp, crate::error::service::ServiceError> {
        let expiration = MultisigQueueDomain::sub_expiration(req.expiration.unwrap_or(1));

        if let Some(token) = token {
            let mut params =
                ContractTransferOpt::new(&token, &req.from, &req.to, value, req.notes.clone())?;

            params.permission_id = permission_id;

            let provider = self.chain.get_provider();
            let constant = params.constant_contract(provider).await?;
            let consumer = provider.contract_fee(constant, threshold as u8, &req.from).await?;
            params.set_fee_limit(consumer);

            Ok(self.chain.build_multisig_transaction(params, expiration as u64).await?)
        } else {
            let mut params = TransferOpt::new(&req.from, &req.to, value, req.notes.clone())?;
            params.permission_id = permission_id;

            Ok(self.chain.build_multisig_transaction(params, expiration as u64).await?)
        }
    }

    pub(super) async fn estimate_swap(
        &self,
        swap_params: &SwapParams,
    ) -> Result<EstimateSwapResult<ResourceConsumer>, crate::error::service::ServiceError> {
        let (params, owner_address) = self.build_base_swap(swap_params)?;

        let wrap = WarpContract { params };

        // 模拟交易结果
        let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
        constant.is_success()?;

        let bytes = wallet_utils::hex_func::hex_decode(&constant.constant_result[0])?;

        // 模拟的结果k
        let (amount_in, amount_out): (U256, U256) = <(U256, U256)>::abi_decode_params(&bytes, true)
            .map_err(|e| crate::error::service::ServiceError::AggregatorError {
                code: -1,
                agg_code: 0,
                msg: e.to_string(),
            })?;

        // get fee
        let mut consumer = self.chain.provider.contract_fee(constant, 1, &owner_address).await?;
        // 手续费增加0.2trx
        consumer.set_extra_fee(200000);

        let resp = EstimateSwapResult { amount_in, amount_out, consumer };

        Ok(resp)
    }

    fn build_base_swap(
        &self,
        swap_params: &SwapParams,
    ) -> Result<(TriggerContractParameter, String), crate::error::service::ServiceError> {
        let call_value = dexSwap1Call::try_from((swap_params, ChainCode::Tron))?;

        // tracing::warn!("call value: {:#?}", call_value);
        let contract_address = swap_params.aggregator_tron_addr()?;
        let owner_address = swap_params.recipient_tron_addr()?;

        let contract_address = wallet_utils::address::bs58_addr_to_hex(&contract_address)?;
        let owner_address = wallet_utils::address::bs58_addr_to_hex(&owner_address)?;

        let mut raw = vec![];
        call_value.abi_encode_raw(&mut raw);

        // 构建调用合约的参数
        let parameter = wallet_utils::hex_func::hex_encode(raw);
        let function_selector = "dexSwap1(((uint16,address,bool,uint256,uint256)[])[],address,address,uint256,uint256,address,bool)";
        let mut value = TriggerContractParameter::new(
            &contract_address,
            &owner_address,
            &function_selector,
            parameter,
        );

        // 主币设置转账的value
        if swap_params.token_in.is_zero() {
            value.call_value = Some(swap_params.amount_in.to::<u64>());
        }

        Ok((value, owner_address))
    }

    fn build_base_withdraw(
        &self,
        req: &WithdrawReq,
        value: U256,
    ) -> Result<TriggerContractParameter, crate::error::service::ServiceError> {
        // 构建调用合约的参数
        let parameter = abi_encode_u256(value);
        let function_selector = "withdraw(uint256)";

        let contract_address = wallet_utils::address::bs58_addr_to_hex(&req.token)?;
        let owner_address = wallet_utils::address::bs58_addr_to_hex(&req.from)?;

        let value = TriggerContractParameter::new(
            &contract_address,
            &owner_address,
            &function_selector,
            parameter,
        );

        Ok(value)
    }
}

#[async_trait::async_trait]
impl Tx for TronTx {
    fn rpc_endpoint_for_log(&self) -> Option<String> {
        Some(self.rpc_url.clone())
    }

    async fn account_resource(
        &self,
        owner_address: &str,
    ) -> Result<AccountResourceDetail, ServiceError> {
        let resource = self.chain.account_resource(owner_address).await?;
        Ok(resource)
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
        let transaction = match self.chain.get_provider().query_tx_info(hash).await {
            Ok(tx) => tx,
            Err(err) => {
                let err_msg = err.to_string();
                if Self::is_empty_tx_query_payload(&err_msg) {
                    tracing::warn!(
                        rpc = %self.rpc_url,
                        tx_hash = %hash,
                        detail = %err_msg,
                        "查询链上状态返回空响应(按未确认处理)"
                    );
                    return Ok(None);
                }

                return Err(err);
            }
        };

        let time = transaction.block_timestamp / 1000;
        let fee = transaction.fee / wallet_chain_interact::tron::consts::TRX_TO_SUN as f64;
        let status = if transaction.result.is_none() { 2 } else { 3 };
        let confirmed_consume = Self::confirmed_resource_consume(
            transaction.receipt.net_usage,
            transaction.receipt.energy_usage,
            transaction.receipt.energy_usage_total,
        );
        tracing::info!(
            tx_hash = %transaction.id,
            net_usage = ?transaction.receipt.net_usage,
            energy_usage = ?transaction.receipt.energy_usage,
            energy_usage_total = ?transaction.receipt.energy_usage_total,
            mapped_net_used = confirmed_consume.net_used,
            mapped_energy_used = confirmed_consume.energy_used,
            "TRON confirmed receipt resource mapped"
        );
        let resource_consume = confirmed_consume.to_json_str()?;

        Ok(Some(QueryTransactionResult::new(
            transaction.id,
            fee,
            resource_consume,
            time,
            status,
            transaction.block_number,
        )))
    }

    async fn has_pending_tx(&self, hash: &str) -> Result<bool, ServiceError> {
        Ok(self.chain.has_pending_tx(hash).await?)
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

    async fn transfer(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, ServiceError> {
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let token_key = params.base.token_address.clone();
        let token = token_key.to_chain_token_option();
        if let Some(contract) = &token {
            tracing::info!("contract: {}", contract);
            tracing::info!("from: {}", params.base.from);
            tracing::info!("to: {}", params.base.to);
            let mut transfer_params = ContractTransferOpt::new(
                contract,
                &params.base.from,
                &params.base.to,
                transfer_amount,
                params.base.notes.clone(),
            )?;
            tracing::info!("transfer ---------------- 11");
            // if let Some(signer) = &params.signer {
            //     transfer_params = transfer_params.with_permission(signer.permission_id);
            // }

            let provider = self.chain.get_provider();
            let balance = self.chain.balance(&params.base.from, Some(contract.clone())).await?;
            if balance < transfer_amount {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(params.base.from.clone())
                            .to_addr(params.base.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .token_addr(contract.clone())
                            .value(transfer_amount.to_string())
                            .balance(balance.to_string())
                            .need(transfer_amount.to_string())
                            .reason("token balance is insufficient for tron transfer"),
                    ),
                ))?;
            }
            tracing::info!("transfer ---------------- 12");

            // constant contract to fee
            let constant = transfer_params.constant_contract(self.chain.get_provider()).await?;
            tracing::info!("transfer ---------------- 12.1");
            let consumer =
                provider.contract_fee(constant, 1, &transfer_params.owner_address).await?;

            // TRON token transfer can be fully covered by delegated resources.
            // A zero-TRX sender is valid when the simulated fee is already zero.
            let transaction_fee = consumer.transaction_fee_i64();
            Self::log_tron_token_resource_estimate(&transfer_params.owner_address, &consumer);
            if transaction_fee > 0 {
                let account = provider.account_info(&transfer_params.owner_address).await?;
                tracing::info!("transfer ---------------- 13");
                if !Self::has_enough_main_balance_for_token_fee(account.balance, transaction_fee) {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::InsufficientFeeBalance,
                    ))?;
                }
            }

            // 需要实际消耗的资源
            let net_used = consumer.act_bandwidth() as u64;
            let energy_used = consumer.act_energy() as u64;

            let fee = consumer.transaction_fee();
            let bill_consumer = BillResourceConsume::new_tron(net_used, energy_used);
            // fee_limit controls the contract invocation energy cap. Even when
            // delegated resources make transaction_fee zero, TRC20 transfers
            // still need a high enough fee_limit or the chain can fail with a
            // tiny curInvokeEnergyLimit.
            transfer_params.set_fee_limit(consumer);
            let tx_hash = self.chain.exec_transaction(transfer_params, private_key).await?;

            let mut resp = TransferResp::new(tx_hash, fee);
            resp.with_consumer(bill_consumer);

            Ok(resp)
        } else {
            // 转账的金额转换为sun
            let value_f64 = unit::format_to_f64(transfer_amount, params.base.decimals)?;
            let value_i64 = (value_f64 * tron::consts::TRX_TO_SUN as f64) as i64;

            let param = tron::operations::transfer::TransferOpt::new(
                &params.base.from,
                &params.base.to,
                transfer_amount,
                params.base.notes.clone(),
            )?;
            // if let Some(signer) = &params.signer {
            //     param = param.with_permission(signer.permission_id);
            // }
            let provider = self.chain.get_provider();
            let account = provider.account_info(&param.from).await?;
            if account.balance <= 0 {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(param.from.clone())
                            .to_addr(param.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .value(transfer_amount.to_string())
                            .balance(account.balance.to_string())
                            .reason("main coin balance is insufficient for tron transfer"),
                    ),
                ))?;
            }

            let tx = param.build_raw_transaction(provider).await?;
            let consumer =
                provider.transfer_fee(&param.from, Some(&param.to), &tx.raw_data_hex, 1).await?;

            if account.balance < consumer.transaction_fee_i64() + value_i64 {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(param.from.clone())
                            .to_addr(param.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .value(transfer_amount.to_string())
                            .balance(account.balance.to_string())
                            .need((consumer.transaction_fee_i64() + value_i64).to_string())
                            .fee(consumer.transaction_fee_i64().to_string())
                            .reason("main coin balance is insufficient for tron transfer fee"),
                    ),
                ))?;
            }

            let bill_consumer = BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0);

            let tx_hash = self.chain.exec_transaction_v1(tx, private_key).await?;

            let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
            resp.with_consumer(bill_consumer);

            Ok(resp)
        }
    }

    async fn build_transfer_raw(
        &self,
        params: &ApiTransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<(String, RawTx, String), ServiceError> {
        let transfer_amount = self.check_min_transfer(&params.base.value, params.base.decimals)?;
        let token_key = params.base.token_address.clone();
        let token = token_key.to_chain_token_option();
        if let Some(contract) = &token {
            tracing::info!("contract: {}", contract);
            tracing::info!("from: {}", params.base.from);
            tracing::info!("to: {}", params.base.to);
            let mut transfer_params = ContractTransferOpt::new(
                contract,
                &params.base.from,
                &params.base.to,
                transfer_amount,
                params.base.notes.clone(),
            )?;
            tracing::info!("transfer ---------------- 11");
            // if let Some(signer) = &params.signer {
            //     transfer_params = transfer_params.with_permission(signer.permission_id);
            // }

            let provider = self.chain.get_provider();
            let balance = self.chain.balance(&params.base.from, Some(contract.clone())).await?;
            if balance < transfer_amount {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(params.base.from.clone())
                            .to_addr(params.base.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .token_addr(contract.clone())
                            .value(transfer_amount.to_string())
                            .balance(balance.to_string())
                            .need(transfer_amount.to_string())
                            .reason("token balance is insufficient for tron transfer"),
                    ),
                ))?;
            }
            tracing::info!("transfer ---------------- 12");

            // constant contract to fee
            let constant = transfer_params.constant_contract(self.chain.get_provider()).await?;
            tracing::info!("transfer ---------------- 12.1");
            let consumer =
                provider.contract_fee(constant, 1, &transfer_params.owner_address).await?;

            let transaction_fee = consumer.transaction_fee_i64();
            Self::log_tron_token_resource_estimate(&transfer_params.owner_address, &consumer);
            if transaction_fee > 0 {
                let account = provider.account_info(&transfer_params.owner_address).await?;
                tracing::info!("transfer ---------------- 13");
                if !Self::has_enough_main_balance_for_token_fee(account.balance, transaction_fee) {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::InsufficientFeeBalance,
                    ))?;
                }
            }

            // 需要实际消耗的资源
            let net_used = consumer.act_bandwidth() as u64;
            let energy_used = consumer.act_energy() as u64;

            let fee = consumer.transaction_fee();
            let bill_consumer = BillResourceConsume::new_tron(net_used, energy_used);
            // fee_limit controls the contract invocation energy cap. Even when
            // delegated resources make transaction_fee zero, TRC20 transfers
            // still need a high enough fee_limit or the chain can fail with a
            // tiny curInvokeEnergyLimit.
            transfer_params.set_fee_limit(consumer);
            let mut raw = transfer_params
                .build_raw_transaction(&self.chain.get_provider())
                .await?
                .set_expiration_from_timestamp(Self::API_WITHDRAW_RAW_EXPIRATION_SECS)?;
            tracing::info!("transfer ---------------- 14");

            let sign = wallet_utils::sign::sign_tron(&raw.tx_id, &private_key, None)?;
            raw.signature.push(sign);
            let tx_hash = raw.tx_id.clone();

            Ok((tx_hash, RawTx::Tron(raw, bill_consumer, fee.clone()), fee))
        } else {
            // 转账的金额转换为sun
            let value_f64 = unit::format_to_f64(transfer_amount, params.base.decimals)?;
            let value_i64 = (value_f64 * tron::consts::TRX_TO_SUN as f64) as i64;

            let param = tron::operations::transfer::TransferOpt::new(
                &params.base.from,
                &params.base.to,
                transfer_amount,
                params.base.notes.clone(),
            )?;
            // if let Some(signer) = &params.signer {
            //     param = param.with_permission(signer.permission_id);
            // }
            let provider = self.chain.get_provider();
            let account = provider.account_info(&param.from).await?;
            if account.balance <= 0 {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(param.from.clone())
                            .to_addr(param.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .value(transfer_amount.to_string())
                            .balance(account.balance.to_string())
                            .reason("main coin balance is insufficient for tron transfer"),
                    ),
                ))?;
            }

            let mut raw = param
                .build_raw_transaction(provider)
                .await?
                .set_expiration_from_timestamp(Self::API_WITHDRAW_RAW_EXPIRATION_SECS)?;
            let consumer =
                provider.transfer_fee(&param.from, Some(&param.to), &raw.raw_data_hex, 1).await?;

            if account.balance < consumer.transaction_fee_i64() + value_i64 {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(param.from.clone())
                            .to_addr(param.to.clone())
                            .chain_code(params.base.chain_code.clone())
                            .value(transfer_amount.to_string())
                            .balance(account.balance.to_string())
                            .need((consumer.transaction_fee_i64() + value_i64).to_string())
                            .fee(consumer.transaction_fee_i64().to_string())
                            .reason("main coin balance is insufficient for tron transfer fee"),
                    ),
                ))?;
            }

            let bill_consumer = BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0);
            let tx_hash = raw.tx_id.clone();

            let fee = consumer.transaction_fee();
            let sign = wallet_utils::sign::sign_tron(&raw.tx_id, &private_key, None)?;
            raw.signature.push(sign);
            Ok((tx_hash, RawTx::Tron(raw, bill_consumer, fee.clone()), fee))
        }
    }

    async fn broadcast_transfer(&self, raw: RawTx) -> Result<TransferResp, ServiceError> {
        if let RawTx::Tron(raw, bill_consumer, fee) = raw {
            let res = self.chain.get_provider().exec_raw_transaction(raw).await?;
            let mut resp = TransferResp::new(res.tx_id, fee.clone());
            resp.with_consumer(bill_consumer);
            Ok(resp)
        } else {
            Err(ServiceError::Business(BusinessError::Chain(ChainError::InvalidRawTx)))
        }
    }

    async fn estimate_fee(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        // let token_currency =
        //     TokenCurrencyGetter::get_currency(currency, &req.chain_code, main_symbol, None).await?;

        let value = unit::convert_to_u256(&req.value, req.decimals)?;
        let token_key = req.token_address.clone();
        let token = token_key.to_chain_token_option();
        let consumer = if let Some(contract) = token {
            let balance = self.chain.balance(&req.from, Some(contract.clone())).await?;
            if balance < value {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                        crate::error::business::chain::InsufficientBalanceDetail::new()
                            .from_addr(req.from.clone())
                            .to_addr(req.to.clone())
                            .chain_code(req.chain_code.clone())
                            .token_addr(contract.clone())
                            .value(value.to_string())
                            .balance(balance.to_string())
                            .need(value.to_string())
                            .reason("token balance is insufficient for tron estimate fee"),
                    ),
                ))?;
            }

            let params = tron::operations::transfer::ContractTransferOpt::new(
                &contract,
                &req.from,
                &req.to,
                value,
                req.notes.clone(),
            )?;

            self.chain.contract_fee(&req.from, 1, params).await?
        } else {
            let params = tron::operations::transfer::TransferOpt::new(
                &req.from,
                &req.to,
                value,
                req.notes.clone(),
            )?;

            self.chain.simulate_simple_fee(&req.from, &req.to, 1, params).await?
        };
        let pool = self.ctx.api_wallet_pool()?;
        let token_currency = TokenCurrencyGetter::get_currency_by_token_key_with_pool(
            &self.ctx.core_pool()?,
            currency,
            &req.chain_code,
            main_symbol,
            wallet_database::entities::asset_token_key::AssetTokenKey::Native,
        )
        .await?;

        let res = TronFeeDetails::new(consumer, token_currency, currency)?;

        let fee = wallet_utils::serde_func::serde_to_string(&res)?;
        Ok(fee)
    }

    async fn estimate_fee_snapshot(
        &self,
        req: ApiBaseTransferReq,
        main_symbol: &str,
    ) -> Result<FeeEstimateSnapshot, ServiceError> {
        let response = self.estimate_fee_without_balance_check(req, main_symbol).await?;
        let details: TronFeeDetails = wallet_utils::serde_func::serde_from_str(&response)?;
        Ok(FeeEstimateSnapshot::from_response(&response)?
            .with_tron_resources(details.energy, details.bandwidth))
    }

    // async fn approve(
    //     &self,
    //     req: &ApproveReq,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let approve = Approve::new(&req.from, &req.spender, &req.contract, value);
    //     let mut wrap = WarpContract::new(approve)?;
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let consumer = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     // check balance
    //     let balance = self.chain.balance(&req.from, None).await?;
    //     let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    //     if balance < fee {
    //         return Err(crate::error::business::BusinessError::Chain(
    //             crate::error::business::chain::ChainError::InsufficientFeeBalance,
    //         ))?;
    //     }
    //
    //     // get consumer
    //     let bill_consumer = BillResourceConsume::new_tron(
    //         consumer.act_bandwidth() as u64,
    //         consumer.act_energy() as u64,
    //     );
    //
    //     // exec trans
    //     let raw_transaction = wrap.trigger_smart_contract(&self.chain.provider, &consumer).await?;
    //     let result = self.chain.exec_transaction_v1(raw_transaction, key).await?;
    //
    //     let mut resp = TransferResp::new(result, consumer.transaction_fee());
    //     resp.with_consumer(bill_consumer);
    //
    //     Ok(resp)
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
    //     let approve = Approve::new(&req.from, &req.spender, &req.contract, value);
    //     let wrap = WarpContract::new(approve)?;
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let consumer = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     let res = TronFeeDetails::new(consumer, token_currency, &currency)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&res)?;
    //
    //     Ok(fee)
    // }

    // async fn allowance(
    //     &self,
    //     from: &str,
    //     token: &str,
    //     spender: &str,
    // ) -> Result<U256, ServiceError> {
    //     let approve = Allowance::new(from, spender, token);
    //     let wrap = WarpContract::new(approve)?;
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //
    //     Ok(constant.parse_u256()?)
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
    //         aggregator_addr: QuoteReq::addr_tron_to_eth(&req.aggregator_addr)?,
    //         amount_in: req.amount_in_u256()?,
    //         min_amount_out,
    //         recipient: QuoteReq::addr_tron_to_eth(&req.recipient)?,
    //         token_in: SwapParams::tron_parse_or_zero_addr(&req.token_in.token_addr)?,
    //         token_out: SwapParams::tron_parse_or_zero_addr(&req.token_out.token_addr)?,
    //         dex_router: quote_resp.dex_route_list.clone(),
    //         allow_partial_fill: req.allow_partial_fill,
    //     };
    //
    //     let resp = self.estimate_swap(&swap_params).await?;
    //
    //     let consumer = wallet_utils::serde_func::serde_to_string(&resp.consumer)?;
    //
    //     let res = TronFeeDetails::new(resp.consumer, token_currency, &currency)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&res)?;
    //
    //     Ok((resp.amount_out, consumer, fee))
    // }

    // async fn swap(
    //     &self,
    //     req: &SwapReq,
    //     _fee: String,
    //     key: ChainPrivateKey,
    // ) -> Result<TransferResp, ServiceError> {
    //     let swap_params = SwapParams::try_from(req)?;
    //     let (params, owner_address) = self.build_base_swap(&swap_params)?;
    //
    //     let mut wrap = WarpContract { params };
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     // get fee
    //     let mut consumer = self.chain.provider.contract_fee(constant, 1, &owner_address).await?;
    //
    //     // check fee
    //     let balance = self.chain.balance(&swap_params.recipient_tron_addr()?, None).await?;
    //     // 手续费增加0.2trx
    //     consumer.set_extra_fee(200000);
    //
    //     let mut fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    //     if swap_params.main_coin_swap() {
    //         fee += swap_params.amount_in;
    //     }
    //     if balance < fee {
    //         return Err(crate::error::business::BusinessError::Chain(
    //             crate::error::business::chain::ChainError::InsufficientFeeBalance,
    //         ))?;
    //     }
    //
    //     let bill_consumer = BillResourceConsume::new_tron(
    //         consumer.act_bandwidth() as u64,
    //         consumer.act_energy() as u64,
    //     );
    //
    //     let raw_transaction = wrap.trigger_with_fee(&self.chain.provider, 300).await?;
    //
    //     let tx_hash = self.chain.exec_transaction_v1(raw_transaction, key).await?;
    //
    //     let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
    //     resp.with_consumer(bill_consumer);
    //
    //     Ok(resp)
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
    //     let approve = Deposit::new(&req.from, &req.token, value);
    //     let wrap = WarpContract::new(approve)?;
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let resource = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;
    //
    //     let res = TronFeeDetails::new(resource, token_currency, &currency)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&res)?;
    //
    //     Ok((consumer, fee))
    // }

    // async fn deposit(
    //     &self,
    //     req: &DepositReq,
    //     _fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let approve = Deposit::new(&req.from, &req.token, value);
    //     let mut wrap = WarpContract::new(approve)?;
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let consumer = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     // check balance
    //     let balance = self.chain.balance(&req.from, None).await?;
    //     let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64()) + value;
    //     if balance < fee {
    //         return Err(crate::error::business::BusinessError::Chain(
    //             crate::error::business::chain::ChainError::InsufficientFeeBalance,
    //         ))?;
    //     }
    //
    //     // get consumer
    //     let bill_consumer = BillResourceConsume::new_tron(
    //         consumer.act_bandwidth() as u64,
    //         consumer.act_energy() as u64,
    //     );
    //
    //     // exec trans
    //     let raw_transaction = wrap.trigger_smart_contract(&self.chain.provider, &consumer).await?;
    //     let result = self.chain.exec_transaction_v1(raw_transaction, key).await?;
    //
    //     let mut resp = TransferResp::new(result, consumer.transaction_fee());
    //     resp.with_consumer(bill_consumer);
    //
    //     Ok(resp)
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
    //     let trigger = self.build_base_withdraw(&req, value)?;
    //     let wrap = WarpContract { params: trigger };
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let resource = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;
    //
    //     let res = TronFeeDetails::new(resource, token_currency, &currency)?;
    //     let fee = wallet_utils::serde_func::serde_to_string(&res)?;
    //
    //     Ok((consumer, fee))
    // }

    // async fn withdraw(
    //     &self,
    //     req: &WithdrawReq,
    //     _fee: String,
    //     key: ChainPrivateKey,
    //     value: U256,
    // ) -> Result<TransferResp, ServiceError> {
    //     let trigger = self.build_base_withdraw(req, value)?;
    //     let mut wrap = WarpContract { params: trigger };
    //
    //     // get fee
    //     let constant = wrap.trigger_constant_contract(&self.chain.provider).await?;
    //     let consumer = self.chain.provider.contract_fee(constant, 1, &req.from).await?;
    //
    //     let balance = self.chain.balance(&req.from, None).await?;
    //     let fee = alloy::primitives::U256::from(consumer.transaction_fee_i64());
    //     if balance < fee {
    //         return Err(crate::error::business::BusinessError::Chain(
    //             crate::error::business::chain::ChainError::InsufficientFeeBalance,
    //         ))?;
    //     }
    //
    //     // get consumer
    //     let bill_consumer = BillResourceConsume::new_tron(
    //         consumer.act_bandwidth() as u64,
    //         consumer.act_energy() as u64,
    //     );
    //
    //     // exec trans
    //     let raw_transaction = wrap.trigger_smart_contract(&self.chain.provider, &consumer).await?;
    //     let result = self.chain.exec_transaction_v1(raw_transaction, key).await?;
    //
    //     let mut resp = TransferResp::new(result, consumer.transaction_fee());
    //     resp.with_consumer(bill_consumer);
    //
    //     Ok(resp)
    // }
}

// #[async_trait::async_trait]
// impl Multisig for TronTx {
//     async fn multisig_address(
//         &self,
//         account: &MultisigAccountEntity,
//         _member: &MultisigMemberEntities,
//     ) -> Result<FetchMultisigAddressResp, ServiceError> {
//         Ok(FetchMultisigAddressResp {
//             authority_address: "".to_string(),
//             multisig_address: account.address.to_string(),
//             salt: "".to_string(),
//         })
//     }
//
//     async fn deploy_multisig_account(
//         &self,
//         account: &MultisigAccountEntity,
//         member: &MultisigMemberEntities,
//         _fee_setting: Option<String>,
//         key: ChainPrivateKey,
//     ) -> Result<(String, String), ServiceError> {
//         let params = tron::operations::multisig::MultisigAccountOpt::new(
//             &account.initiator_addr,
//             account.threshold as u8,
//             member.get_owner_str_vec(),
//         )?;
//
//         // check balance
//         let provider = self.chain.get_provider();
//         let tx = params.build_raw_transaction(provider).await?;
//         let mut consumer =
//             provider.transfer_fee(&account.initiator_addr, None, &tx.raw_data_hex, 1).await?;
//
//         let chain_parameter = self.chain.provider.chain_params().await?;
//         consumer.set_extra_fee(chain_parameter.update_account_fee());
//
//         let fee = consumer.transaction_fee_i64();
//         let account = provider.account_info(&account.initiator_addr).await?;
//         if account.balance < fee {
//             return Err(crate::error::business::BusinessError::Chain(
//                 crate::error::business::chain::ChainError::insufficient_balance(),
//             ))?;
//         }
//
//         let consumer = BillResourceConsume::new_tron(consumer.bandwidth.consumer as u64, 0);
//         let tx_hash = self.chain.exec_transaction_v1(tx, key).await?;
//
//         Ok((tx_hash, consumer.to_json_str()?))
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
//         // let backend = crate::manager::Context::get_global_backend_api()?;
//
//         let account_info = self.chain.get_provider().account_info(&account.initiator_addr).await?;
//         if account_info.address.is_empty() {
//             return Err(crate::error::business::BusinessError::Chain(
//                 crate::error::business::chain::ChainError::AddressNotInit,
//             ))?;
//         }
//
//         let params = tron::operations::multisig::MultisigAccountOpt::new(
//             &account.initiator_addr,
//             account.threshold as u8,
//             member.get_owner_str_vec(),
//         )?;
//         let mut consumer = self.chain.simple_fee(&account.initiator_addr, 1, params).await?;
//
//         let chain_parameter = self.chain.provider.chain_params().await?;
//         consumer.set_extra_fee(chain_parameter.update_account_fee());
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &account.chain_code, main_symbol, None)
//                 .await?;
//
//         let res = TronFeeDetails::new(consumer, token_currency, currency)?;
//         Ok(wallet_utils::serde_func::serde_to_string(&res)?)
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
//         account: &MultisigAccountEntity,
//         assets: &ApiAssetsEntity,
//         key: ChainPrivateKey,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         let decimal = assets.decimals;
//         let token = assets.token_address();
//
//         let value = self.check_min_transfer(&req.value, decimal)?;
//         let balance = self.chain.balance(&req.from, token.clone()).await?;
//         if balance < value {
//             return Err(crate::error::business::BusinessError::Chain(
//                 crate::error::business::chain::ChainError::insufficient_balance(),
//             ))?;
//         }
//
//         self.build_build_tx(req, token, value, account.threshold as i64, None).await
//     }
//
//     async fn build_multisig_with_permission(
//         &self,
//         req: &TransferParams,
//         p: &PermissionEntity,
//         coin: &CoinEntity,
//     ) -> Result<MultisigTxResp, ServiceError> {
//         let decimal = coin.decimals;
//         let token = coin.token_address();
//
//         let value = self.check_min_transfer(&req.value, decimal)?;
//         let balance = self.chain.balance(&req.from, token.clone()).await?;
//         if balance < value {
//             return Err(crate::error::business::BusinessError::Chain(
//                 crate::error::business::chain::ChainError::insufficient_balance(),
//             ))?;
//         }
//
//         let permission_id = Some(p.active_id);
//         self.build_build_tx(req, token, value, p.threshold, permission_id).await
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
//         let res = TransactionOpt::sign_transaction(raw_data, key)?;
//         Ok(res)
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
//         // let token_currency =
//         //     TokenCurrencyGetter::get_currency(currency, &queue.chain_code, main_symbol, None)
//         //         .await?;
//
//         let signature_num = sign_list.len() as u8;
//         let value = unit::convert_to_u256(&queue.value, coin.decimals)?;
//         let memo = (!queue.notes.is_empty()).then(|| queue.notes.clone());
//
//         let consumer = if let Some(token) = coin.token_address() {
//             let params = tron::operations::transfer::ContractTransferOpt::new(
//                 &token,
//                 &queue.from_addr,
//                 &queue.to_addr,
//                 value,
//                 memo,
//             )?;
//
//             self.chain.contract_fee(&queue.from_addr, signature_num, params).await?
//         } else {
//             let params =
//                 tron::operations::multisig::TransactionOpt::data_from_str(&queue.raw_data)?;
//
//             let to = (!queue.to_addr.is_empty()).then_some(queue.to_addr.as_str());
//
//             self.chain
//                 .provider
//                 .transfer_fee(&queue.from_addr, to, &params.raw_data_hex, signature_num)
//                 .await?
//         };
//
//         let token_currency =
//             TokenCurrencyGetter::get_currency(currency, &queue.chain_code, main_symbol, None)
//                 .await?;
//
//         // if queue.transfer_type == ApiBillKind::UpdatePermission.to_i8() {
//         //     let chain = self.chain.provider.chain_params().await?;
//         //     consumer.set_extra_fee(chain.update_account_fee());
//         // }
//
//         let res = TronFeeDetails::new(consumer, token_currency, currency)?;
//         Ok(wallet_utils::serde_func::serde_to_string(&res)?)
//     }
// }

#[cfg(test)]
mod tests {
    use super::TronTx;

    #[test]
    fn tron_token_fee_balance_gate_allows_zero_trx_when_fee_is_zero() {
        // Arrange
        let balance = 0;
        let transaction_fee = 0;

        // Act
        let allowed = TronTx::has_enough_main_balance_for_token_fee(balance, transaction_fee);

        // Assert
        assert!(allowed);
    }

    #[test]
    fn tron_token_fee_balance_gate_rejects_positive_fee_without_balance() {
        // Arrange
        let balance = 0;
        let transaction_fee = 1;

        // Act
        let allowed = TronTx::has_enough_main_balance_for_token_fee(balance, transaction_fee);

        // Assert
        assert!(!allowed);
    }

    #[test]
    fn tron_token_fee_balance_gate_allows_positive_fee_with_enough_balance() {
        // Arrange
        let balance = 1_000;
        let transaction_fee = 1_000;

        // Act
        let allowed = TronTx::has_enough_main_balance_for_token_fee(balance, transaction_fee);

        // Assert
        assert!(allowed);
    }

    #[test]
    fn confirmed_resource_consume_prefers_total_energy() {
        let consume = TronTx::confirmed_resource_consume(Some(0), None, Some(13_532));

        assert_eq!(consume.net_used, 0);
        assert_eq!(consume.energy_used, 13_532);
    }

    #[test]
    fn confirmed_resource_consume_falls_back_to_energy_usage() {
        let consume = TronTx::confirmed_resource_consume(Some(321), Some(14_650), None);

        assert_eq!(consume.net_used, 321);
        assert_eq!(consume.energy_used, 14_650);
    }

    #[tokio::test]
    #[ignore = "requires Nile TRON RPC"]
    async fn confirmed_resource_consume_reads_live_withdraw_receipts() {
        let client = wallet_transport::client::HttpClient::new(
            "https://nile.trongrid.io",
            None,
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("Nile HTTP client should initialize");
        let provider = wallet_chain_interact::tron::Provider::new(client)
            .expect("Nile TRON provider should initialize");
        let cases = [
            ("c44efcd57ae7401bb2821c7a93ee8f43df73f8826b79bfc8f7438e293f64f8bf", 0, 14_650),
            ("5b7e5cf72721ece1abee13e84ab29fb9a22f3fc568f05c8c2865be73be624794", 345, 14_650),
            ("eb5bfe0c35755b9261469aa1caf9888889fe81a87cf7533facc9b450da2f31de", 0, 13_532),
            ("585b0f40dc42196700f199400edfa03c5f7135795c9cfe70aa64846ffbbcb458", 0, 14_650),
            ("b34c3378b45ed8be9ad929646d0d932d09776566ed2c799698e839f102da7e5a", 0, 14_650),
        ];

        for (tx_hash, expected_net, expected_energy) in cases {
            let tx = provider
                .query_tx_info(tx_hash)
                .await
                .unwrap_or_else(|error| panic!("withdraw {tx_hash} should be queryable: {error}"));
            let consume = TronTx::confirmed_resource_consume(
                tx.receipt.net_usage,
                tx.receipt.energy_usage,
                tx.receipt.energy_usage_total,
            );

            assert_eq!(consume.net_used, expected_net, "tx {tx_hash}");
            assert_eq!(consume.energy_used, expected_energy, "tx {tx_hash}");
        }
    }
}
