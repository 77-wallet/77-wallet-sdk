use super::{TIME_OUT, eth_tx, evm_fee, ton_tx, tron_tx};
use crate::{
    dispatch,
    domain::{
        self,
        chain::{
            TransferResp,
            adapter::sol_tx::{self, DEFAULT_UNITS_FEE, TOKEN_ACCOUNT_RENT},
            pare_fee_setting,
            swap::{W_SOL, evm_swap::SwapParams},
            transaction::{ChainTransDomain, DEFAULT_UNITS},
        },
        coin::TokenCurrencyGetter,
    },
    infrastructure::swap_client::{AggQuoteResp, SolInstructResp},
    request::transaction::{self, DepositReq, QuoteReq, SwapReq, WithdrawReq},
    response_vo::{self, CommonFeeDetails, FeeDetails, TronFeeDetails},
};
use alloy::primitives::U256;
use std::collections::HashMap;
use wallet_chain_interact::{
    self as chain, BillResourceConsume,
    btc::{self},
    dog,
    eth::{self},
    ltc,
    sol::{self, SolFeeSetting, operations::SolInstructionOperation},
    sui,
    ton::{self},
    tron::{
        self,
        operations::{TronConstantOperation as _, TronTxOperation},
    },
    types::{ChainPrivateKey, Transaction as _},
};
use wallet_database::entities::{asset_token_key::AssetTokenKey, coin::CoinEntity};
use wallet_transport::client::{HttpClient, RpcClient};
use wallet_types::chain::{
    address::r#type::{DogAddressType, LtcAddressType, TonAddressType},
    chain::ChainCode as ChainType,
    network::NetworkKind,
};
use wallet_utils::unit;

pub enum TransactionAdapter {
    BitCoin(chain::btc::BtcChain),
    Ethereum(chain::eth::EthChain),
    Solana(chain::sol::SolanaChain),
    Tron(chain::tron::TronChain),
    Ltc(chain::ltc::LtcChain),
    Doge(chain::dog::DogChain),
    Ton(chain::ton::chain::TonChain),
    Sui(chain::sui::SuiChain),
}

impl TransactionAdapter {
    pub fn new(
        chain_code: ChainType,
        rpc_url: &str,
        header_opt: Option<HashMap<String, String>>,
        network: NetworkKind,
    ) -> Result<TransactionAdapter, chain::Error> {
        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        match chain_code {
            ChainType::Bitcoin => {
                let config = chain::btc::provider::ProviderConfig {
                    rpc_url: rpc_url.to_string(),
                    rpc_auth: None,
                    http_url: rpc_url.to_string(),
                    http_api_key: None,
                };
                let btc_chain = chain::btc::BtcChain::new(config, network, header_opt, timeout)?;
                Ok(TransactionAdapter::BitCoin(btc_chain))
            }
            ChainType::Ethereum
            | ChainType::BnbSmartChain
            | ChainType::Polygon
            | ChainType::Avalanche
            | ChainType::Fantom
            | ChainType::Arbitrum
            | ChainType::Base
            | ChainType::Scroll => {
                let rpc_client = RpcClient::new(rpc_url, header_opt, timeout)?;
                let provider = eth::Provider::new(rpc_client)?;
                let eth_chain = chain::eth::EthChain::new(provider, network, chain_code)?;
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
            ChainType::Dogcoin => {
                let config = chain::dog::provider::ProviderConfig {
                    rpc_url: rpc_url.to_string(),
                    rpc_auth: None,
                    http_url: rpc_url.to_string(),
                    http_api_key: None,
                    access_key: None,
                };

                let doge = chain::dog::DogChain::new(config, network, header_opt, timeout)?;
                Ok(TransactionAdapter::Doge(doge))
            }
            ChainType::Litecoin => {
                let config = chain::ltc::provider::ProviderConfig {
                    rpc_url: rpc_url.to_string(),
                    rpc_auth: None,
                    http_url: rpc_url.to_string(),
                    http_api_key: None,
                    access_key: None,
                };

                let doge = chain::ltc::LtcChain::new(config, network, header_opt, timeout)?;
                Ok(TransactionAdapter::Ltc(doge))
            }
            ChainType::Ton => {
                let http_client = HttpClient::new(rpc_url, header_opt, timeout)?;
                let provider = ton::provider::Provider::new(http_client);

                let ton = chain::ton::chain::TonChain::new(provider)?;

                Ok(TransactionAdapter::Ton(ton))
            }
            ChainType::Sui => {
                let mut provider = sui::Provider::new(rpc_url, header_opt)?;
                let grpc_timeout = std::time::Duration::from_secs(TIME_OUT);
                provider.grpc_client = provider
                    .grpc_client
                    .with_response_headers_timeout(grpc_timeout)
                    .with_body_idle_timeout(grpc_timeout);
                Ok(TransactionAdapter::Sui(sui::SuiChain::new(provider)?))
            }
            _ => Err(chain::Error::Other(format!("unsupported transaction chain: {chain_code}"))),
        }
    }
}

impl TransactionAdapter {
    pub async fn balance(
        &self,
        addr: &str,
        token_key: AssetTokenKey,
    ) -> Result<U256, chain::Error> {
        let token_address = token_key.into_chain_token_option();
        dispatch!(self, balance, addr, token_address)
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

    pub async fn has_pending_tx(&self, hash: &str) -> Result<bool, chain::Error> {
        match self {
            Self::Tron(chain) => Ok(chain.has_pending_tx(hash).await?),
            _ => Ok(false),
        }
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
    ) -> Result<bool, crate::error::service::ServiceError> {
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
        ctx: &'static crate::context::Context,
        params: &mut transaction::TransferReq,
        private_key: ChainPrivateKey,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let transfer_amount = if matches!(self, Self::Solana(_))
            && params.base.spend_all
            && params.base.token_address.is_native()
        {
            unit::convert_to_u256(&params.base.value, params.base.decimals)?
        } else {
            ChainTransDomain::check_min_transfer(&params.base.value, params.base.decimals)?
        };
        let token_address = params.base.token_address.to_chain_token_option();

        match self {
            Self::Ethereum(chain) => {
                // Share the API wallet sender gate without API DB/counter access.
                // Hold through nonce observation, signing and broadcast; Drop
                // releases on both errors and cancellation.
                let _transfer_gate =
                    ctx.transfer_gates.acquire(&params.base.from, &params.base.chain_code).await;
                let mut fee_setting = pare_fee_setting(params.fee_setting.as_str())?;
                let balance = chain.balance(&params.base.from, None).await?;
                let spend_all_native = params.base.spend_all && token_address.is_none();
                if chain.chain_code == ChainType::Scroll {
                    tracing::info!(target: "scroll_diagnostic", stage = "send_balance",
                        from = %params.base.from, to = %params.base.to, token = ?token_address,
                        balance_wei = %balance, value_wei = %transfer_amount,
                        spend_all_native, gas_limit = %fee_setting.gas_limit,
                        max_fee_per_gas = %fee_setting.max_fee_per_gas,
                        max_priority_fee_per_gas = %fee_setting.max_priority_fee_per_gas,
                        "Scroll diagnostic");
                }
                let mut transfer = chain::eth::operations::TransferOpt::try_from(&params.base)?;

                let final_amount = if spend_all_native {
                    let is_contract =
                        chain.provider.is_contract_address(&params.base.to).await? != "0x";
                    // Core refreshes testnet fees; freeze that same fee before calculating max.
                    if chain.network != NetworkKind::Mainnet {
                        let reserve =
                            evm_fee::execution_fee_reserve(chain.chain_code, &fee_setting);
                        if balance <= reserve {
                            return Err(crate::error::business::BusinessError::Chain(
                                crate::error::business::chain::ChainError::InsufficientFeeBalance,
                            ))?;
                        }
                        // Only EOAs can safely use a minimal-value gas probe.
                        transfer.base.value =
                            if is_contract { balance - reserve } else { U256::from(1_u64) };
                        fee_setting = chain.provider.get_fee(transfer.build_transaction()?).await?;
                    }
                    Some(
                        evm_fee::prepare_native_maximum(
                            chain,
                            &mut transfer,
                            balance,
                            &mut fee_setting,
                            is_contract,
                        )
                        .await?,
                    )
                } else {
                    let remain_balance = ChainTransDomain::check_eth_balance(
                        &params.base.from,
                        balance,
                        token_address.as_deref(),
                        chain,
                        transfer_amount,
                    )
                    .await?;
                    // Scroll L1 sizing uses these fields, so freeze the testnet fee
                    // that Core would otherwise refresh after this balance check.
                    if chain.chain_code == ChainType::Scroll
                        && chain.network != NetworkKind::Mainnet
                    {
                        fee_setting = chain.provider.get_fee(transfer.build_transaction()?).await?;
                    }
                    let reserve = evm_fee::execution_fee_reserve(chain.chain_code, &fee_setting);
                    let required =
                        evm_fee::required_send_fee(chain, &transfer, reserve, &fee_setting).await?;
                    evm_fee::ensure_send_fee_balance(remain_balance, required)?;
                    None
                };
                let execution_fee = fee_setting.transaction_fee();
                let actual_value = final_amount
                    .map(|value| unit::format_to_string(value, eth::consts::ETH_DECIMAL))
                    .transpose()?;
                let mut tx = transfer.build_transaction()?;
                if !spend_all_native
                    && chain.network != NetworkKind::Mainnet
                    && chain.chain_code != ChainType::Scroll
                {
                    fee_setting = chain.provider.get_fee(tx.clone()).await?;
                }
                tx = chain.provider.set_transaction_fee(tx, fee_setting, chain.chain_code).await?;
                let tx_hash =
                    super::evm_submission::send(chain, tx, &private_key, Some(&params.fee_setting))
                        .await?;
                // The domain builds its pending bill from this request after successful send.
                if let Some(value) = actual_value {
                    params.base.value = value;
                }
                Ok(TransferResp::new(
                    tx_hash,
                    unit::format_to_string(execution_fee, eth::consts::ETH_DECIMAL)?,
                ))
            }
            Self::BitCoin(chain) => {
                let account = domain::chain::transaction::ChainTransDomain::account(
                    ctx,
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
                )?
                .with_spend_all(params.base.spend_all);

                let tx = chain
                    .transfer(params, private_key)
                    .await
                    .map_err(ChainTransDomain::handle_btc_fee_error)?;

                Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
            }
            Self::Ltc(chain) => {
                let account =
                    ChainTransDomain::account(ctx, &params.base.chain_code, &params.base.from)
                        .await?;

                let address_type = LtcAddressType::try_from(account.address_type())?;

                let params = ltc::operations::transfer::TransferArg::new(
                    &params.base.from,
                    &params.base.to,
                    &params.base.value,
                    address_type,
                    chain.network,
                )?
                .with_spend_all(params.base.spend_all);

                let tx = chain
                    .transfer(params, private_key)
                    .await
                    .map_err(ChainTransDomain::handle_btc_fee_error)?;

                Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
            }
            Self::Doge(chain) => {
                let account =
                    ChainTransDomain::account(ctx, &params.base.chain_code, &params.base.from)
                        .await?;

                let address_type = DogAddressType::try_from(account.address_type())?;

                let params = dog::operations::transfer::TransferArg::new(
                    &params.base.from,
                    &params.base.to,
                    &params.base.value,
                    address_type,
                    chain.network,
                )?
                .with_spend_all(params.base.spend_all);

                let tx = chain
                    .transfer(params, private_key)
                    .await
                    .map_err(ChainTransDomain::handle_btc_fee_error)?;

                Ok(TransferResp::new(tx.tx_hash, tx.fee.to_string()))
            }
            Self::Solana(chain) => {
                let balance = chain.balance(&params.base.from, None).await?;
                let spend_all_native = params.base.spend_all && token_address.is_none();
                let transfer_value = if spend_all_native {
                    unit::format_to_string(U256::from(1_u64), params.base.decimals)?
                } else {
                    params.base.value.clone()
                };

                if !spend_all_native {
                    // check balance
                    let remain_balance = ChainTransDomain::check_sol_balance(
                        &params.base.from,
                        balance,
                        token_address.as_deref(),
                        chain,
                        transfer_amount,
                    )
                    .await?;

                    let token = token_address.clone();
                    let params = sol::operations::transfer::TransferOpt::new(
                        &params.base.from,
                        &params.base.to,
                        &transfer_value,
                        token_address.clone(),
                        params.base.decimals,
                        chain.get_provider(),
                    )?;

                    let instructions = params.instructions().await?;
                    let mut fee_setting = chain.estimate_fee_v1(&instructions, &params).await?;
                    ChainTransDomain::sol_priority_fee(
                        &mut fee_setting,
                        token.as_ref(),
                        DEFAULT_UNITS,
                    );

                    ChainTransDomain::check_sol_transaction_fee(
                        remain_balance,
                        fee_setting.original_fee(),
                    )?;
                    let fee = fee_setting.transaction_fee().to_string();

                    let tx_hash = chain
                        .exec_transaction(params, private_key, Some(fee_setting), instructions, 0)
                        .await?;

                    Ok(TransferResp::new(tx_hash, fee))
                } else {
                    let token = token_address.clone();
                    let probe_params = sol::operations::transfer::TransferOpt::new(
                        &params.base.from,
                        &params.base.to,
                        &transfer_value,
                        token_address.clone(),
                        params.base.decimals,
                        chain.get_provider(),
                    )?;

                    let probe_instructions = probe_params.instructions().await?;
                    let mut fee_setting =
                        chain.estimate_fee_v1(&probe_instructions, &probe_params).await?;
                    ChainTransDomain::sol_priority_fee(
                        &mut fee_setting,
                        token.as_ref(),
                        DEFAULT_UNITS,
                    );

                    let final_transfer_amount =
                        sol_tx::native_spend_all_amount(balance, fee_setting.original_fee())?;
                    let recipient_exists = {
                        let to_addr = wallet_utils::address::parse_sol_address(&params.base.to)?;
                        chain.get_provider().account_info(to_addr).await?.value.is_some()
                    };
                    let minimum_rent = wallet_utils::unit::convert_to_u256(
                        &sol_tx::SYSTEM_ACCOUNT_RENT.to_string(),
                        wallet_chain_interact::sol::consts::SOL_DECIMAL,
                    )?;
                    crate::domain::api_wallet::adapter::sol_tx::SolTx::native_transfer_rent_precheck(
                        &params.base.from,
                        &params.base.to,
                        recipient_exists,
                        balance,
                        final_transfer_amount,
                        minimum_rent,
                    )?;

                    let remain_balance = ChainTransDomain::check_sol_balance(
                        &params.base.from,
                        balance,
                        token_address.as_deref(),
                        chain,
                        final_transfer_amount,
                    )
                    .await?;
                    ChainTransDomain::check_sol_transaction_fee(
                        remain_balance,
                        fee_setting.original_fee(),
                    )?;

                    let final_transfer_value =
                        unit::format_to_string(final_transfer_amount, params.base.decimals)?;
                    let params = sol::operations::transfer::TransferOpt::new(
                        &params.base.from,
                        &params.base.to,
                        &final_transfer_value,
                        token_address.clone(),
                        params.base.decimals,
                        chain.get_provider(),
                    )?;
                    let instructions = params.instructions().await?;
                    let fee = fee_setting.transaction_fee().to_string();
                    let tx_hash = chain
                        .exec_transaction(params, private_key, Some(fee_setting), instructions, 0)
                        .await?;

                    Ok(TransferResp::new(tx_hash, fee))
                }
            }
            Self::Tron(chain) => {
                if let Some(contract) = &token_address {
                    let mut transfer_params = tron::operations::transfer::ContractTransferOpt::new(
                        contract,
                        &params.base.from,
                        &params.base.to,
                        transfer_amount,
                        params.base.notes.clone(),
                    )?;

                    if let Some(signer) = &params.signer {
                        transfer_params = transfer_params.with_permission(signer.permission_id);
                    }

                    let provider = chain.get_provider();
                    let balance = chain.balance(&params.base.from, Some(contract.clone())).await?;
                    if balance < transfer_amount {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                                crate::error::business::chain::InsufficientBalanceDetail::new()
                                    .from_addr(params.base.from.clone())
                                    .to_addr(params.base.to.clone())
                                    .chain_code(params.base.chain_code.to_string())
                                    .token_addr(contract.clone())
                                    .value(transfer_amount.to_string())
                                    .balance(balance.to_string())
                                    .need(transfer_amount.to_string())
                                    .reason("token balance is insufficient for tron contract transfer"),
                            ),
                        ))?;
                    }

                    let account = provider.account_info(&transfer_params.owner_address).await?;
                    // 主币是否有钱(可能账号未被初始化)
                    if account.balance <= 0 {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ))?;
                    }

                    // constant contract to fee
                    let constant = transfer_params.constant_contract(chain.get_provider()).await?;
                    let consumer =
                        provider.contract_fee(constant, 1, &transfer_params.owner_address).await?;

                    if account.balance < consumer.transaction_fee_i64() {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ))?;
                    }

                    // 需要实际消耗的资源
                    let net_used = consumer.act_bandwidth() as u64;
                    let energy_used = consumer.act_energy() as u64;

                    let fee = consumer.transaction_fee();
                    transfer_params.set_fee_limit(consumer);

                    let bill_consumer = BillResourceConsume::new_tron(net_used, energy_used);
                    let tx_hash = chain.exec_transaction(transfer_params, private_key).await?;

                    let mut resp = TransferResp::new(tx_hash, fee);
                    resp.with_consumer(bill_consumer);

                    Ok(resp)
                } else {
                    // 转账的金额转换为sun
                    let value_f64 = unit::format_to_f64(transfer_amount, params.base.decimals)?;
                    let value_i64 = (value_f64 * tron::consts::TRX_TO_SUN as f64) as i64;

                    let mut param = tron::operations::transfer::TransferOpt::new(
                        &params.base.from,
                        &params.base.to,
                        transfer_amount,
                        params.base.notes.clone(),
                    )?;
                    if let Some(signer) = &params.signer {
                        param = param.with_permission(signer.permission_id);
                    }
                    let provider = chain.get_provider();
                    let account = provider.account_info(&param.from).await?;
                    if account.balance <= 0 {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                                crate::error::business::chain::InsufficientBalanceDetail::new()
                                    .from_addr(params.base.from.clone())
                                    .to_addr(params.base.to.clone())
                                    .chain_code(params.base.chain_code.to_string())
                                    .value(value_i64.to_string())
                                    .balance(account.balance.to_string())
                                    .need(value_i64.to_string())
                                    .reason("account has no fee balance"),
                            ),
                        ))?;
                    }

                    let tx = param.build_raw_transaction(provider).await?;
                    let consumer = provider
                        .transfer_fee(&param.from, Some(&param.to), &tx.raw_data_hex, 1)
                        .await?;

                    if account.balance < consumer.transaction_fee_i64() + value_i64 {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                                crate::error::business::chain::InsufficientBalanceDetail::new()
                                    .from_addr(params.base.from.clone())
                                    .to_addr(params.base.to.clone())
                                    .chain_code(params.base.chain_code.to_string())
                                    .value(value_i64.to_string())
                                    .balance(account.balance.to_string())
                                    .need((consumer.transaction_fee_i64() + value_i64).to_string())
                                    .fee(consumer.transaction_fee_i64().to_string())
                                    .reason("account balance is insufficient for transfer and fee"),
                            ),
                        ))?;
                    }

                    let bill_consumer =
                        BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0);

                    let tx_hash = chain.exec_transaction_v1(tx, private_key).await?;

                    let mut resp = TransferResp::new(tx_hash, consumer.transaction_fee());
                    resp.with_consumer(bill_consumer);

                    Ok(resp)
                }
            }
            Self::Ton(chain) => {
                // 验证余额
                let balance = chain.balance(&params.base.from, token_address.clone()).await?;
                if balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(params.base.from.clone())
                                .to_addr(params.base.to.clone())
                                .chain_code(params.base.chain_code.to_string())
                                .token_addr(token_address.clone().unwrap_or_default())
                                .value(transfer_amount.to_string())
                                .balance(balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("token balance is insufficient for ton transfer"),
                        ),
                    ))?;
                }

                let account =
                    ChainTransDomain::account(ctx, &params.base.chain_code, &params.base.from)
                        .await?;

                let address_type = match account.address_type() {
                    Some(ty) => TonAddressType::try_from(ty.as_str())?,
                    None => Err(crate::error::service::ServiceError::Types(
                        wallet_types::error::Error::MissAddressType,
                    ))?,
                };

                let msg_cell =
                    ton_tx::build_ext_cell(&params.base, &chain.provider, address_type).await?;

                let fee =
                    chain.estimate_fee(msg_cell.clone(), &params.base.from, address_type).await?;

                let mut trans_fee = U256::from(fee.get_fee());
                if token_address.is_none() {
                    // 主笔转账
                    if !params.base.spend_all {
                        trans_fee += transfer_amount;
                        if balance < trans_fee {
                            return Err(crate::error::business::BusinessError::Chain(
                                crate::error::business::chain::ChainError::InsufficientFeeBalance,
                            ))?;
                        }
                    }
                } else {
                    let balance = chain.balance(&params.base.from, None).await?;
                    if balance < trans_fee {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ))?;
                    }
                }
                let tx_hash = chain.exec(msg_cell, private_key, address_type).await?;

                Ok(TransferResp::new(tx_hash, fee.get_fee_ton().to_string()))
            }
            Self::Sui(chain) => {
                let balance = chain.balance(&params.base.from, token_address.clone()).await?;
                if balance < transfer_amount {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(params.base.from.clone())
                                .to_addr(params.base.to.clone())
                                .chain_code(params.base.chain_code.to_string())
                                .token_addr(token_address.clone().unwrap_or_default())
                                .value(transfer_amount.to_string())
                                .balance(balance.to_string())
                                .need(transfer_amount.to_string())
                                .reason("token balance is insufficient for sui transfer"),
                        ),
                    ))?;
                }

                let req = sui::transfer::TransferOpt::new(
                    &params.base.from,
                    &params.base.to,
                    transfer_amount,
                    token_address.clone(),
                )?;

                let mut helper = req.select_coin(&chain.provider).await?;
                let pt = req.build_pt(&chain.provider, &mut helper, None).await?;

                let gas = chain.estimate_fee(&params.base.from, pt).await?;

                let mut trans_fee = U256::from(gas.get_fee());
                if token_address.is_none() {
                    trans_fee += transfer_amount;
                    if balance < trans_fee {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ))?;
                    }
                } else {
                    let balance = chain.balance(&params.base.from, None).await?;
                    if balance < trans_fee {
                        return Err(crate::error::business::BusinessError::Chain(
                            crate::error::business::chain::ChainError::InsufficientFeeBalance,
                        ))?;
                    }
                }

                let fee = gas.get_fee_f64();
                let tx_data = req.build_data(&chain.provider, helper, gas).await?;
                let tx_hash = chain.exec(tx_data, private_key).await?;

                Ok(TransferResp::new(tx_hash, fee.to_string()))
            }
        }
    }

    pub async fn estimate_fee(
        &self,
        ctx: &'static crate::context::Context,
        req: transaction::BaseTransferReq,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        self.estimate_fee_with_ctx(ctx, req, main_symbol, backend.as_ref()).await
    }

    pub async fn estimate_fee_with_ctx(
        &self,
        ctx: &'static crate::context::Context,
        req: transaction::BaseTransferReq,
        main_symbol: &str,
        backend_api: &wallet_transport_backend::api::BackendApi,
    ) -> Result<String, crate::error::service::ServiceError> {
        let token_address = req.token_address.to_chain_token_option();

        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();
        let pool = ctx.core_pool()?;

        let token_currency =
            domain::coin::token_price::TokenCurrencyGetter::get_currency_by_token_key_with_pool(
                &pool,
                currency,
                &req.chain_code,
                main_symbol,
                AssetTokenKey::Native,
            )
            .await?;

        let res = match self {
            Self::Ethereum(chain) => Ok(evm_fee::estimate_transfer_fee(
                chain,
                &req,
                backend_api,
                currency,
                token_currency,
            )
            .await?),
            Self::BitCoin(chain) => {
                let account = ChainTransDomain::account(ctx, &req.chain_code, &req.from).await?;
                let params = btc::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    account.address_type(),
                    chain.network,
                )?
                .with_spend_all(req.spend_all);

                let fee = chain
                    .estimate_fee(params, None)
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                let res = response_vo::CommonFeeDetails::new(
                    fee.transaction_fee_f64(),
                    token_currency,
                    currency,
                )?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Ltc(chain) => {
                let account = ChainTransDomain::account(ctx, &req.chain_code, &req.from).await?;

                let address_type = LtcAddressType::try_from(account.address_type())?;

                let params = ltc::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    address_type,
                    chain.network,
                )?
                .with_spend_all(req.spend_all);

                let fee = chain
                    .estimate_fee(params)
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                let res = response_vo::CommonFeeDetails::new(
                    fee.transaction_fee_f64(),
                    token_currency,
                    currency,
                )?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Doge(chain) => {
                let account = ChainTransDomain::account(ctx, &req.chain_code, &req.from).await?;

                let address_type = DogAddressType::try_from(account.address_type())?;

                let params = dog::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    address_type,
                    chain.network,
                )?
                .with_spend_all(req.spend_all);

                let fee = chain
                    .estimate_fee(params)
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                let res = response_vo::CommonFeeDetails::new(
                    fee.transaction_fee_f64(),
                    token_currency,
                    currency,
                )?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Solana(chain) => {
                let token = token_address.clone();
                let params = sol::operations::transfer::TransferOpt::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    token_address.clone(),
                    req.decimals,
                    chain.get_provider(),
                )?;

                let instructions = params.instructions().await?;
                let mut fee_setting = chain.estimate_fee_v1(&instructions, &params).await?;

                ChainTransDomain::sol_priority_fee(&mut fee_setting, token.as_ref(), DEFAULT_UNITS);

                let res = response_vo::CommonFeeDetails::new(
                    fee_setting.transaction_fee(),
                    token_currency,
                    currency,
                )?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Tron(chain) => {
                let value = unit::convert_to_u256(&req.value, req.decimals)?;

                let consumer = if let Some(contract) = token_address {
                    let balance = chain.balance(&req.from, Some(contract.clone())).await?;
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
                                .reason("token balance is insufficient for tron fee estimation"),
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

                    chain.contract_fee(&req.from, 1, params).await?
                } else {
                    let params = tron::operations::transfer::TransferOpt::new(
                        &req.from,
                        &req.to,
                        value,
                        req.notes.clone(),
                    )?;

                    chain.simulate_simple_fee(&req.from, &req.to, 1, params).await?
                };
                let token_currency =
                    domain::coin::token_price::TokenCurrencyGetter::get_currency_by_token_key(
                        ctx,
                        currency,
                        &req.chain_code,
                        main_symbol,
                        AssetTokenKey::Native,
                    )
                    .await?;

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Ton(chain) => {
                let account = ChainTransDomain::account(ctx, &req.chain_code, &req.from).await?;

                let address_type = match account.address_type() {
                    Some(ty) => TonAddressType::try_from(ty.as_str())?,
                    None => Err(crate::error::service::ServiceError::Types(
                        wallet_types::error::Error::MissAddressType,
                    ))?,
                };

                let msg_cell = ton_tx::build_ext_cell(&req, &chain.provider, address_type).await?;

                let fee = chain.estimate_fee(msg_cell.clone(), &req.from, address_type).await?;

                let res = response_vo::CommonFeeDetails::new(
                    fee.get_fee_ton(),
                    token_currency,
                    currency,
                )?;

                wallet_utils::serde_func::serde_to_string(&res)
            }
            Self::Sui(chain) => {
                let amount = unit::convert_to_u256(&req.value, req.decimals)?;
                let params =
                    sui::transfer::TransferOpt::new(&req.from, &req.to, amount, token_address)?;

                let mut helper = params.select_coin(&chain.provider).await?;
                let pt = params.build_pt(&chain.provider, &mut helper, None).await?;

                let gas = chain.estimate_fee(&req.from, pt).await?;

                let res = response_vo::CommonFeeDetails::new(
                    gas.get_fee_f64(),
                    token_currency,
                    currency,
                )?;

                wallet_utils::serde_func::serde_to_string(&res)
            }
        };
        Ok(res?)
    }

    pub async fn approve(
        &self,
        req: &transaction::ApproveReq,
        key: ChainPrivateKey,
        value: alloy::primitives::U256,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let hash = match self {
            Self::Ethereum(chain) => eth_tx::approve(chain, req, value, key).await?,
            Self::Tron(chain) => tron_tx::approve(chain, req, value, key).await?,
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(hash)
    }

    pub async fn approve_fee(
        &self,
        ctx: &'static crate::context::Context,
        req: &transaction::ApproveReq,
        value: alloy::primitives::U256,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let currency = {
            let currency = crate::app_state::APP_STATE.read().await;
            currency.currency().to_string()
        };

        let token_currency =
            domain::coin::token_price::TokenCurrencyGetter::get_currency_by_token_key_with_pool(
                &ctx.core_pool()?,
                &currency,
                &req.chain_code,
                main_symbol,
                AssetTokenKey::Native,
            )
            .await?;

        let fee = match self {
            Self::Ethereum(chain) => {
                let fee = eth_tx::approve_fee(chain, req, value).await?;

                let gas_oracle = ChainTransDomain::default_gas_oracle(&chain.provider).await?;
                let fee = FeeDetails::try_from((gas_oracle, fee.consume))?
                    .to_resp(token_currency, &currency)?;

                wallet_utils::serde_func::serde_to_string(&fee)?
                // CommonFeeDetails::new(fee, token_currency, &currency)
            }
            Self::Tron(chain) => {
                let consumer = tron_tx::approve_fee(chain, req, value).await?;

                // let fee = consumer.transaction_fee_i64() as f64 / TRX_TO_SUN as f64;
                // CommonFeeDetails::new(fee, token_currency, &currency)

                let res = TronFeeDetails::new(consumer, token_currency, &currency)?;
                wallet_utils::serde_func::serde_to_string(&res)?
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(fee)
    }

    pub async fn allowance(
        &self,
        from: &str,
        token: &str,
        spender: &str,
    ) -> Result<U256, crate::error::service::ServiceError> {
        let resp = match self {
            Self::Ethereum(chain) => eth_tx::allowance(chain, from, token, spender).await?,
            Self::Tron(chain) => tron_tx::allowance(chain, from, token, spender).await?,
            Self::Solana(_chain) => U256::MAX,
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn sol_swap_fee(
        &self,
        req: &QuoteReq,
        consumer: Option<u64>,
    ) -> Result<SolFeeSetting, crate::error::service::ServiceError> {
        match self {
            Self::Solana(chain) => {
                // 需要预留 账号租金
                let mut fee_setting = SolFeeSetting {
                    base_fee: DEFAULT_UNITS_FEE,
                    priority_fee_per_compute_unit: None,
                    compute_units_consumed: consumer.unwrap_or(0),
                    extra_fee: None,
                };

                // 没有目标代币,需要一部分的费用
                if !req.token_out.token_addr.is_empty() {
                    let account = chain
                        .get_provider()
                        .token_balance(&req.token_out.token_addr, &req.recipient)
                        .await?;

                    if account.value.is_empty() {
                        fee_setting.extra_fee = Some(TOKEN_ACCOUNT_RENT)
                    }
                };

                // 如果没有中间代币 WSOL,考虑手续费
                if req.is_native() {
                    let account = chain.get_provider().token_balance(W_SOL, &req.recipient).await?;

                    if account.value.is_empty() {
                        if let Some(extra_fee) = fee_setting.extra_fee {
                            fee_setting.extra_fee = Some(extra_fee + TOKEN_ACCOUNT_RENT)
                        } else {
                            fee_setting.extra_fee = Some(TOKEN_ACCOUNT_RENT)
                        }
                    }
                }

                Ok(fee_setting)
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        }
    }

    pub async fn swap_quote(
        &self,
        ctx: &'static crate::context::Context,
        req: &QuoteReq,
        quote_resp: &AggQuoteResp,
        symbol: &str,
        sol_instructions: Option<SolInstructResp>,
    ) -> Result<(U256, String, String), crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        self.swap_quote_with_ctx(ctx, req, quote_resp, symbol, sol_instructions, backend.as_ref())
            .await
    }

    pub async fn swap_quote_with_ctx(
        &self,
        ctx: &'static crate::context::Context,
        req: &QuoteReq,
        quote_resp: &AggQuoteResp,
        symbol: &str,
        sol_instructions: Option<SolInstructResp>,
        backend_api: &wallet_transport_backend::api::BackendApi,
    ) -> Result<(U256, String, String), crate::error::service::ServiceError> {
        let min_amount_out = U256::from(1);

        let currency = {
            let currency = crate::app_state::APP_STATE.read().await;
            currency.currency().to_string()
        };
        let token_currency = TokenCurrencyGetter::get_currency_by_token_key(
            ctx,
            &currency,
            &req.chain_code,
            symbol,
            AssetTokenKey::Native,
        )
        .await?;

        let resp = match self {
            Self::Ethereum(chain) => {
                let swap_params = SwapParams {
                    aggregator_addr: req.aggregator_address()?,
                    amount_in: req.amount_in_u256()?,
                    min_amount_out,
                    recipient: wallet_utils::address::parse_eth_address(&req.recipient)?,
                    token_in: SwapParams::eth_parse_or_zero_addr(&req.token_in.token_addr)?,
                    token_out: SwapParams::eth_parse_or_zero_addr(&req.token_out.token_addr)?,
                    dex_router: quote_resp.dex_route_list.clone(),
                    allow_partial_fill: req.allow_partial_fill,
                };

                let resp = eth_tx::estimate_swap(swap_params, chain).await?;
                let gas_oracle =
                    ChainTransDomain::gas_oracle(&req.chain_code, &chain.provider, backend_api)
                        .await?;
                let fee = FeeDetails::try_from((gas_oracle, resp.consumer.to::<i64>()))?
                    .to_resp(token_currency, &currency)?;

                // 消耗的资源
                let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
                // 具体的手续费结构
                let fee = wallet_utils::serde_func::serde_to_string(&fee)?;

                (resp.amount_out, consumer, fee)
            }
            Self::Tron(chain) => {
                let swap_params = SwapParams {
                    aggregator_addr: QuoteReq::addr_tron_to_eth(&req.aggregator_addr)?,
                    amount_in: req.amount_in_u256()?,
                    min_amount_out,
                    recipient: QuoteReq::addr_tron_to_eth(&req.recipient)?,
                    token_in: SwapParams::tron_parse_or_zero_addr(&req.token_in.token_addr)?,
                    token_out: SwapParams::tron_parse_or_zero_addr(&req.token_out.token_addr)?,
                    dex_router: quote_resp.dex_route_list.clone(),
                    allow_partial_fill: req.allow_partial_fill,
                };

                let resp = tron_tx::estimate_swap(&swap_params, chain).await?;

                let consumer = wallet_utils::serde_func::serde_to_string(&resp.consumer)?;

                let res = TronFeeDetails::new(resp.consumer, token_currency, &currency)?;
                let fee = wallet_utils::serde_func::serde_to_string(&res)?;

                (resp.amount_out, consumer, fee)
            }
            Self::Solana(chain) => {
                let resp =
                    sol_tx::estimate_swap(&req.recipient, sol_instructions.unwrap(), chain).await?;

                // let fee_setting = self.sol_swap_fee(&req, Some(resp.consumer)).await?;

                // let fee = fee_setting.transaction_fee();
                // let consumer = wallet_utils::serde_func::serde_to_string(&fee_setting)?;
                // let sol_fee =
                //     CommonFeeDetails::new(fee, token_currency, &currency)?.to_json_str()?;

                (resp.amount_out, "".to_string(), "".to_string())
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn swap(
        &self,
        req: &SwapReq,
        fee: String,
        key: ChainPrivateKey,
        instructions: Option<SolInstructResp>,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let swap_params = SwapParams::try_from(req)?;

        let resp = match self {
            Self::Ethereum(chain) => eth_tx::swap(chain, &swap_params, fee, key).await?,
            Self::Tron(chain) => tron_tx::swap(chain, &swap_params, key).await?,
            Self::Solana(chain) => {
                sol_tx::swap(chain, req, fee, instructions.unwrap(), key).await?
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn deposit_fee(
        &self,
        ctx: &'static crate::context::Context,
        req: DepositReq,
        main_coin: &CoinEntity,
    ) -> Result<(String, String), crate::error::service::ServiceError> {
        let currency = {
            let currency = crate::app_state::APP_STATE.read().await;
            currency.currency().to_string()
        };

        let core_pool = ctx.core_pool()?;
        let token_currency = TokenCurrencyGetter::get_currency_by_token_key_with_pool(
            &core_pool,
            &currency,
            &req.chain_code,
            &main_coin.symbol,
            AssetTokenKey::Native,
        )
        .await?;
        let value = wallet_utils::unit::convert_to_u256(&req.amount, main_coin.decimals)?;
        // 返回具体消耗了多少资源以及费用的包装结构
        let resp = match self {
            Self::Tron(chain) => {
                let resource = tron_tx::deposit_fee(chain, &req, value).await?;

                let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;

                let res = TronFeeDetails::new(resource, token_currency, &currency)?;
                let fee = wallet_utils::serde_func::serde_to_string(&res)?;

                (consumer, fee)
            }
            Self::Ethereum(chain) => {
                let resource = eth_tx::deposit_fee(chain, &req, value).await?;

                let gas_oracle = ChainTransDomain::default_gas_oracle(&chain.provider).await?;
                let fee = FeeDetails::try_from((gas_oracle, resource.consume))?
                    .to_resp(token_currency, &currency)?;

                let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
                let fee = wallet_utils::serde_func::serde_to_string(&fee)?;

                (consumer, fee)
            }
            Self::Solana(chain) => {
                let resource = sol_tx::deposit_fee(chain, &req, value).await?;
                let fee = resource.transaction_fee();

                let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;

                let fee = CommonFeeDetails::new(fee, token_currency, &currency)?.to_json_str()?;

                (consumer, fee)
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn deposit(
        &self,
        req: &DepositReq,
        fee: String,
        key: ChainPrivateKey,
        value: U256,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let resp = match self {
            Self::Tron(chain) => tron_tx::deposit(chain, req, value, key).await?,
            Self::Ethereum(chain) => eth_tx::deposit(chain, req, value, fee, key).await?,
            Self::Solana(chain) => sol_tx::deposit(chain, req, value, fee, key).await?,
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn withdraw_fee(
        &self,
        ctx: &'static crate::context::Context,
        req: WithdrawReq,
        main_coin: &CoinEntity,
    ) -> Result<(String, String), crate::error::service::ServiceError> {
        let currency = {
            let currency = crate::app_state::APP_STATE.read().await;
            currency.currency().to_string()
        };

        let core_pool = ctx.core_pool()?;
        let token_currency = TokenCurrencyGetter::get_currency_by_token_key_with_pool(
            &core_pool,
            &currency,
            &req.chain_code,
            &main_coin.symbol,
            AssetTokenKey::Native,
        )
        .await?;

        let value = wallet_utils::unit::convert_to_u256(&req.amount, main_coin.decimals)?;

        let resp = match self {
            Self::Tron(chain) => {
                let resource = tron_tx::withdraw_fee(chain, &req, value).await?;

                let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;

                let res = TronFeeDetails::new(resource, token_currency, &currency)?;
                let fee = wallet_utils::serde_func::serde_to_string(&res)?;

                (consumer, fee)
            }
            Self::Ethereum(chain) => {
                let resource = eth_tx::withdraw_fee(chain, &req, value).await?;

                let gas_oracle = ChainTransDomain::default_gas_oracle(&chain.provider).await?;
                let fee = FeeDetails::try_from((gas_oracle, resource.consume))?
                    .to_resp(token_currency, &currency)?;

                let consumer = wallet_utils::serde_func::serde_to_string(&fee.data[0].fee_setting)?;
                let fee = wallet_utils::serde_func::serde_to_string(&fee)?;

                (consumer, fee)
            }
            Self::Solana(chain) => {
                let resource = sol_tx::withdraw_fee(chain, &req, value).await?;
                let fee = resource.transaction_fee();

                let consumer = wallet_utils::serde_func::serde_to_string(&resource)?;
                let fee = CommonFeeDetails::new(fee, token_currency, &currency)?.to_json_str()?;

                (consumer, fee)
            }
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }

    pub async fn withdraw(
        &self,
        req: &WithdrawReq,
        fee: String,
        key: ChainPrivateKey,
        value: U256,
    ) -> Result<TransferResp, crate::error::service::ServiceError> {
        let resp = match self {
            Self::Tron(chain) => tron_tx::withdraw(chain, req, value, key).await?,
            Self::Ethereum(chain) => eth_tx::withdraw(chain, req, value, fee, key).await?,
            Self::Solana(chain) => sol_tx::withdraw(chain, req, value, fee, key).await?,
            _ => {
                return Err(crate::error::business::BusinessError::Chain(
                    crate::error::business::chain::ChainError::NotSupportChain,
                ))?;
            }
        };

        Ok(resp)
    }
}

#[cfg(test)]
mod sui_grpc_tests {
    use super::*;

    #[tokio::test]
    async fn sui_grpc_constructor_is_lazy_and_preserves_endpoint() {
        let adapter = TransactionAdapter::new(
            ChainType::Sui,
            "http://127.0.0.1:1",
            Some(HashMap::from([("x-test-auth".into(), "offline".into())])),
            NetworkKind::Mainnet,
        )
        .expect("valid gRPC configuration requires no node connection");
        let TransactionAdapter::Sui(chain) = adapter else { panic!("wrong chain") };
        assert_eq!(chain.provider.grpc_client.uri().to_string(), "http://127.0.0.1:1/");
    }

    #[tokio::test]
    async fn sui_grpc_constructor_rejects_invalid_endpoint() {
        assert!(
            TransactionAdapter::new(ChainType::Sui, "not a URI", None, NetworkKind::Mainnet)
                .is_err()
        );
    }

    #[tokio::test]
    async fn sui_grpc_constructor_rejects_invalid_metadata() {
        for (key, value) in [("bad header", "offline"), ("x-test-auth", "invalid\nvalue")] {
            assert!(
                TransactionAdapter::new(
                    ChainType::Sui,
                    "http://127.0.0.1:1",
                    Some(HashMap::from([(key.into(), value.into())])),
                    NetworkKind::Mainnet
                )
                .is_err()
            );
        }
    }
}
