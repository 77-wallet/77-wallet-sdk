use crate::{
    dispatch,
    domain::{
        self,
        chain::{pare_fee_setting, transaction::ChainTransaction, TransferResp},
    },
    request::transaction::{self},
    response_vo::{self, FeeDetails, TronFeeDetails},
};
use alloy::primitives::{self, U256};
use std::collections::HashMap;
use wallet_chain_interact::{
    self as chain,
    btc::{self},
    eth,
    sol::{self, operations::SolInstructionOperation},
    tron::{
        self,
        operations::{TronConstantOperation as _, TronTxOperation},
    },
    types::ChainPrivateKey,
    BillResourceConsume,
};
use wallet_transport::client::{HttpClient, RpcClient};
use wallet_types::chain::chain::ChainCode as ChainType;
use wallet_utils::unit;

pub enum TransactionAdapter {
    BitCoin(chain::btc::BtcChain),
    Ethereum(chain::eth::EthChain),
    Solana(chain::sol::SolanaChain),
    Tron(chain::tron::TronChain),
}

impl TransactionAdapter {
    pub fn new(
        chain_code: ChainType,
        rpc_url: &str,
        http_url: &str,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<TransactionAdapter, chain::Error> {
        let network = wallet_types::chain::network::NetworkKind::Mainnet;
        let timeout = Some(std::time::Duration::from_secs(30));
        match chain_code {
            ChainType::Bitcoin => {
                // let auth = wallet_chain_interact::btc::provider::RpcAuth {
                //     user: "hello-bitcoin".to_string(),
                //     password: "123456".to_string(),
                // };
                let config = chain::btc::provider::ProviderConfig {
                    rpc_url: rpc_url.to_string(),
                    rpc_auth: None,
                    http_url: http_url.to_string(),
                    http_api_key: None,
                };
                let btc_chain = chain::btc::BtcChain::new(config, network, header_opt, timeout)?;
                Ok(TransactionAdapter::BitCoin(btc_chain))
            }
            ChainType::Ethereum | ChainType::BnbSmartChain => {
                let rpc_client = RpcClient::new(rpc_url, header_opt, timeout)?;
                let provider = eth::Provider::new(rpc_client)?;
                let eth_chain = chain::eth::EthChain::new(provider, network)?;
                Ok(TransactionAdapter::Ethereum(eth_chain))
            }
            ChainType::Solana => {
                let rpc_client = RpcClient::new(rpc_url, header_opt, timeout)?;
                let provider = sol::Provider::new(rpc_client)?;
                let sol_chain = chain::sol::SolanaChain::new(provider)?;
                Ok(TransactionAdapter::Solana(sol_chain))
            }
            ChainType::Tron => {
                let http_client = HttpClient::new(rpc_url, header_opt, timeout)?;
                let provider = tron::Provider::new(http_client)?;

                let tron_chain = chain::tron::TronChain::new(provider)?;
                Ok(TransactionAdapter::Tron(tron_chain))
            }
        }
    }
}

impl TransactionAdapter {
    pub async fn balance(&self, addr: &str, token: Option<String>) -> Result<U256, chain::Error> {
        dispatch!(self, balance, addr, token)
    }

    pub async fn block_num(&self) -> Result<u64, chain::Error> {
        dispatch!(self, block_num,)
    }

    pub async fn query_tx_res(
        &self,
        hash: &str,
    ) -> Result<Option<chain::QueryTransactionResult>, chain::Error> {
        dispatch!(self, query_tx_res, hash)
    }

    pub async fn decimals(&self, token: &str) -> Result<u8, chain::Error> {
        dispatch!(self, decimals, token)
    }

    pub async fn token_symbol(&self, token: &str) -> Result<String, chain::Error> {
        dispatch!(self, token_symbol, token)
    }

    pub async fn token_name(&self, token: &str) -> Result<String, chain::Error> {
        dispatch!(self, token_name, token)
    }

    // Check if an address is a blacklisted address
    pub async fn black_address(
        &self,
        chain_code: ChainType,
        token: &str,
        owner: &str,
    ) -> Result<bool, crate::ServiceError> {
        match chain_code {
            ChainType::Ethereum | ChainType::Solana | ChainType::Tron => match self {
                Self::Ethereum(chain) => Ok(chain.black_address(token, owner).await?),
                Self::Solana(chain) => Ok(chain.black_address(token, owner).await?),
                Self::Tron(chain) => Ok(chain.black_address(token, owner).await?),
                _ => Ok(false),
            },

            _ => Ok(false),
        }
    }

    // 返回交易hash以及交易消耗的资源
    pub async fn transfer(
        &self,
        params: &transaction::TransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::ServiceError> {
        let transfer_amount =
            ChainTransaction::check_min_transfer(&params.base.value, params.base.decimals)?;

        match self {
            Self::Ethereum(chain) => {
                let fee_setting = pare_fee_setting(params.fee_setting.as_str())?;

                let balance = chain.balance(&params.base.from, None).await?;

                // check balance
                let remain_balance =
                    domain::chain::transaction::ChainTransaction::check_eth_balance(
                        &params.base.from,
                        balance,
                        params.base.token_address.as_deref(),
                        chain,
                        transfer_amount,
                    )
                    .await?;

                let fee = fee_setting.transaction_fee();
                // check transaction_fee
                if remain_balance < fee {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientFeeBalance,
                    ))?;
                }

                let params = chain::eth::operations::TransferOpt::try_from(&params.base)?;
                let tx_hash = chain
                    .exec_transaction(params, fee_setting, private_key)
                    .await?;

                Ok(TransferResp::new(
                    tx_hash,
                    unit::format_to_string(fee, eth::consts::ETH_DECIMAL)?,
                ))
            }
            Self::BitCoin(chain) => {
                let account = domain::chain::transaction::ChainTransaction::account(
                    &params.base.chain_code,
                    &params.base.from,
                )
                .await?;
                let params = btc::operations::transfer::TransferArg::new(
                    &params.base.from,
                    &params.base.to,
                    &params.base.value,
                    account.address_type(),
                    chain.network,
                )?;

                let tx_hash = chain
                    .transfer(params, private_key)
                    .await
                    .map_err(domain::chain::transaction::ChainTransaction::handle_btc_fee_error)?;

                Ok(TransferResp::new(tx_hash, "0".to_string()))
            }
            Self::Solana(chain) => {
                // check balance
                let balance = chain.balance(&params.base.from, None).await?;
                let remain_balance =
                    domain::chain::transaction::ChainTransaction::check_sol_balance(
                        &params.base.from,
                        balance,
                        params.base.token_address.as_deref(),
                        chain,
                        transfer_amount,
                    )
                    .await?;

                let params = sol::operations::transfer::TransferOpt::new(
                    &params.base.from,
                    &params.base.to,
                    &params.base.value,
                    params.base.token_address.clone(),
                    params.base.decimals,
                    chain.get_provider(),
                )?;

                let instructions = params.instructions().await?;
                let fee = chain.estimate_fee_v1(&instructions, &params).await?;
                domain::chain::transaction::ChainTransaction::check_sol_transaction_fee(
                    remain_balance,
                    fee.original_fee(),
                )?;

                let tx_hash = chain
                    .exec_transaction(params, private_key, None, instructions, 0)
                    .await?;

                Ok(TransferResp::new(
                    tx_hash,
                    fee.transaction_fee().to_string(),
                ))
            }
            Self::Tron(chain) => {
                if let Some(contract) = &params.base.token_address {
                    let mut transfer_params = tron::operations::transfer::ContractTransferOpt::new(
                        contract,
                        &params.base.from,
                        &params.base.to,
                        transfer_amount,
                        params.base.notes.clone(),
                    )?;

                    let provider = chain.get_provider();
                    let balance = chain
                        .balance(&params.base.from, Some(contract.clone()))
                        .await?;
                    if balance < transfer_amount {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientBalance,
                        ))?;
                    }

                    let account = provider
                        .account_info(&transfer_params.owner_address)
                        .await?;
                    // 主币是否有钱(可能账号未被初始化)
                    if account.balance <= 0 {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientFeeBalance,
                        ))?;
                    }

                    // constant contract to fee
                    let constant = transfer_params
                        .constant_contract(chain.get_provider())
                        .await?;
                    let consumer = provider
                        .contract_fee(constant, 1, &transfer_params.owner_address)
                        .await?;

                    if account.balance < consumer.transaction_fee_i64() {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientFeeBalance,
                        ))?;
                    }

                    // 将需要的额外资源先写入订单表
                    let bandwidth = consumer.need_extra_bandwidth() as u64;
                    let energy = consumer.need_extra_energy() as u64;

                    let fee = consumer.transaction_fee();
                    transfer_params.set_fee_limit(consumer);

                    let bill_consumer = BillResourceConsume::new_tron(bandwidth, energy);
                    let tx_hash = chain.exec_transaction(transfer_params, private_key).await?;

                    let mut resp = TransferResp::new(tx_hash, fee);
                    resp.with_consumer(bill_consumer);

                    Ok(resp)
                } else {
                    // 转账的金额转换为sun
                    let value_f64 = unit::format_to_f64(transfer_amount, params.base.decimals)?;
                    let value_i64 = (value_f64 * tron::consts::TRX_TO_SUN as f64) as i64;

                    let params = tron::operations::transfer::TransferOpt::new(
                        &params.base.from,
                        &params.base.to,
                        transfer_amount,
                        params.base.notes.clone(),
                    )?;

                    let provider = chain.get_provider();
                    let account = provider.account_info(&params.from).await?;
                    if account.balance <= 0 {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientBalance,
                        ))?;
                    }

                    let tx = params.build_raw_transaction(provider).await?;
                    let consumer = provider
                        .transfer_fee(&params.from, Some(&params.to), &tx, 1)
                        .await?;

                    if account.balance < consumer.transaction_fee_i64() + value_i64 {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientBalance,
                        ))?;
                    }

                    let bill_consumer =
                        BillResourceConsume::new_tron(consumer.bandwidth.consumer as u64, 0);

                    let tx_hash = chain.exec_transaction_v1(tx, private_key).await?;

                    let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
                    resp.with_consumer(bill_consumer);

                    Ok(resp)
                }
            }
        }
    }

    pub async fn estimate_fee(
        &self,
        req: transaction::BaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::ServiceError> {
        let backend = crate::manager::Context::get_global_backend_api()?;

        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let token_currency = domain::coin::token_price::TokenCurrencyGetter::get_currency(
            currency,
            &req.chain_code,
            main_symbol,
        )
        .await?;

        let res = match self {
            Self::Ethereum(chain) => {
                let value = unit::convert_to_u256(&req.value, req.decimals)?;
                let balance = chain.balance(&req.from, req.token_address.clone()).await?;
                if balance <= primitives::U256::default() {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }

                let gas_oracle = domain::chain::transaction::ChainTransaction::gas_oracle(
                    &req.chain_code,
                    &chain.provider,
                    backend,
                )
                .await?;

                let params = eth::operations::TransferOpt::new(
                    &req.from,
                    &req.to,
                    value,
                    req.token_address,
                )?;
                let fee = chain.estimate_gas(params).await?;
                let fee = FeeDetails::try_from((gas_oracle, fee.consume))?
                    .to_resp(token_currency, currency);

                wallet_utils::serde_func::serde_to_string(&fee)
            }
            Self::BitCoin(chain) => {
                let account = domain::chain::transaction::ChainTransaction::account(
                    &req.chain_code,
                    &req.from,
                )
                .await?;
                let params = btc::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    account.address_type(),
                    chain.network,
                )?
                .with_spend_all(req.spend_all);

                let fee = chain
                    .estimate_fee(params)
                    .await
                    .map_err(domain::chain::transaction::ChainTransaction::handle_btc_fee_error)?;

                let res = response_vo::CommonFeeDetails::new(
                    fee.transaction_fee_f64(),
                    token_currency,
                    currency,
                );
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Solana(chain) => {
                let params = sol::operations::transfer::TransferOpt::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    req.token_address,
                    req.decimals,
                    chain.get_provider(),
                )?;

                let instructions = params.instructions().await?;
                let fee = chain
                    .estimate_fee_v1(&instructions, &params)
                    .await?
                    .transaction_fee();

                let res = response_vo::CommonFeeDetails::new(fee, token_currency, currency);
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Tron(chain) => {
                let value = unit::convert_to_u256(&req.value, req.decimals)?;

                let consumer = if let Some(contract) = req.token_address {
                    let params = tron::operations::transfer::ContractTransferOpt::new(
                        &contract,
                        &req.from,
                        &req.to,
                        value,
                        req.notes.clone(),
                    )?;

                    chain.contract_fee(&req.from, 1, params).await?
                } else {
                    let params = tron::operations::transfer::TransferOpt::new(
                        &req.from,
                        &req.to,
                        value,
                        req.notes.clone(),
                    )?;

                    chain
                        .simulate_simple_fee(&req.from, &req.to, 1, params)
                        .await?
                };
                let token_currency = domain::coin::token_price::TokenCurrencyGetter::get_currency(
                    currency,
                    &req.chain_code,
                    main_symbol,
                )
                .await?;

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
        };
        Ok(res?)
    }
}
