use crate::{
    dispatch,
    domain::{self, chain::transaction::ChainTransaction},
    response_vo::{
        self, CommonFeeDetails, FeeDetails, MultisigQueueFeeParams, TransferParams, TronFeeDetails,
    },
};
use std::collections::HashMap;
use wallet_chain_interact::{
    self as chain, btc,
    eth::{self, operations},
    sol::{self, operations::SolInstructionOperation},
    tron::{
        self,
        operations::{TronConstantOperation, TronTxOperation},
    },
    types::{self, ChainPrivateKey},
    BillResourceConsume,
};
use wallet_database::entities::{
    assets::AssetsEntity, multisig_account::MultisigAccountEntity,
    multisig_member::MultisigMemberEntities, multisig_queue::MultisigQueueEntity,
};
use wallet_transport::client::{HttpClient, RpcClient};
use wallet_types::chain::chain::ChainCode as ChainType;
use wallet_utils::{serde_func, unit};

pub enum MultisigAdapter {
    BitCoin(chain::btc::BtcChain),
    Ethereum(chain::eth::EthChain),
    Solana(chain::sol::SolanaChain),
    Tron(chain::tron::TronChain),
}

impl MultisigAdapter {
    pub fn new(
        chain_code: ChainType,
        chian_node: wallet_database::entities::chain::ChainWithNode,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<MultisigAdapter, crate::ServiceError> {
        let network = wallet_types::chain::network::NetworkKind::Mainnet;

        let timeout = Some(std::time::Duration::from_secs(30));
        match chain_code {
            ChainType::Bitcoin => {
                // let auth = wallet_chain_interact::btc::provider::RpcAuth {
                //     user: "hello-bitcoin".to_string(),
                //     password: "123456".to_string(),
                // };
                let config = chain::btc::provider::ProviderConfig {
                    rpc_url: chian_node.rpc_url,
                    rpc_auth: None,
                    http_url: chian_node.http_url,
                    http_api_key: None,
                };

                let btc_chain = chain::btc::BtcChain::new(config, network, header_opt, timeout)?;
                Ok(MultisigAdapter::BitCoin(btc_chain))
            }
            ChainType::Ethereum | ChainType::BnbSmartChain => {
                let rpc_client = RpcClient::new(&chian_node.rpc_url, header_opt, timeout)?;
                let provider = eth::Provider::new(rpc_client)?;

                let eth_chain = chain::eth::EthChain::new(provider, network)?;
                Ok(MultisigAdapter::Ethereum(eth_chain))
            }
            ChainType::Solana => {
                let rpc_client = RpcClient::new(&chian_node.rpc_url, header_opt, timeout)?;
                let provider = sol::Provider::new(rpc_client)?;

                let sol_chain = chain::sol::SolanaChain::new(provider)?;
                Ok(MultisigAdapter::Solana(sol_chain))
            }
            ChainType::Tron => {
                let http_client = HttpClient::new(&chian_node.rpc_url, header_opt, timeout)?;
                let provider = tron::Provider::new(http_client)?;
                let tron_chain = chain::tron::TronChain::new(provider)?;

                Ok(MultisigAdapter::Tron(tron_chain))
            }
        }
    }
}

impl MultisigAdapter {
    pub async fn query_tx_res(
        &self,
        hash: &str,
    ) -> Result<Option<chain::QueryTransactionResult>, chain::Error> {
        dispatch!(self, query_tx_res, hash)
    }

    pub async fn multisig_address(
        &self,
        account: &MultisigAccountEntity,
        member: &MultisigMemberEntities,
    ) -> Result<types::FetchMultisigAddressResp, chain::Error> {
        match self {
            Self::Ethereum(chain) => {
                let params = operations::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold,
                )?
                .with_nonce()
                .with_owners(member.get_owner_str_vec())?;

                chain.multisig_account(params).await
            }
            Self::BitCoin(chain) => {
                let params = btc::operations::multisig::MultisigAccountOpt::new(
                    account.threshold as u8,
                    member.get_owner_pubkey(),
                    &account.address_type,
                )?;
                chain.multisig_address(params).await
            }
            Self::Solana(_chain) => {
                sol::operations::multisig::account::MultisigAccountOpt::multisig_address()
            }
            Self::Tron(_chain) => Ok(types::FetchMultisigAddressResp {
                authority_address: "".to_string(),
                multisig_address: account.address.to_string(),
                salt: "".to_string(),
            }),
        }
    }

    // deploy multisig account
    // return tx_hash and tx consumer string
    pub async fn deploy_multisig_account(
        &self,
        account: &MultisigAccountEntity,
        member: &MultisigMemberEntities,
        fee_setting: Option<String>,
        key: ChainPrivateKey,
    ) -> Result<(String, String), crate::ServiceError> {
        match self {
            Self::Ethereum(chain) => {
                let params = operations::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold,
                )?
                .set_nonce(&account.salt)?
                .with_owners(member.get_owner_str_vec())?;

                let fee_setting: response_vo::EthereumFeeDetails =
                    serde_func::serde_from_str(&fee_setting.unwrap())?;
                let fee_setting = chain::eth::FeeSetting::try_from(fee_setting)?;

                // check transaction_fee
                let balance = chain.balance(&account.initiator_addr, None).await?;
                if balance < fee_setting.transaction_fee() {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientFeeBalance,
                    ))?;
                }

                let tx_hash = chain.exec_transaction(params, fee_setting, key).await?;
                Ok((tx_hash, "".to_string()))
            }
            Self::BitCoin(_chain) => Ok(("".to_string(), "".to_string())),
            Self::Solana(chain) => {
                let params = sol::operations::multisig::account::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold as u8,
                    member.get_owner_str_vec(),
                    account.salt.clone(),
                    chain.get_provider(),
                )?;

                let instructions = params.instructions().await?;

                // check transaction_fee
                let fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let balance = chain.balance(&account.initiator_addr, None).await?;
                domain::chain::transaction::ChainTransaction::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                let tx_hash = chain
                    .exec_transaction(params, key, None, instructions, 0)
                    .await?;

                Ok((tx_hash, "".to_string()))
            }
            Self::Tron(chain) => {
                let params = tron::operations::multisig::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold as u8,
                    member.get_owner_str_vec(),
                )?;

                // check balance
                let provider = chain.get_provider();
                let tx = params.build_raw_transaction(provider).await?;
                let mut consumer = provider
                    .transfer_fee(&account.initiator_addr, None, &tx, 1)
                    .await?;

                let chain_parameter = chain.provider.chain_params().await?;
                consumer.set_extra_fee(chain_parameter.update_account_fee());

                let fee = consumer.transaction_fee_i64();
                let account = provider.account_info(&account.initiator_addr).await?;
                if account.balance < fee {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }

                let consumer = BillResourceConsume::new_tron(consumer.bandwidth.consumer as u64, 0);
                let tx_hash = chain.exec_transaction_v1(tx, key).await?;

                Ok((tx_hash, consumer.to_json_str()?))
                // Ok(chain.exec_transaction_v1(tx, key).await?)
            }
        }
    }

    pub async fn deploy_multisig_fee(
        &self,
        account: &MultisigAccountEntity,
        member: MultisigMemberEntities,
        main_symbol: &str,
    ) -> Result<String, crate::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let backend = crate::manager::Context::get_global_backend_api()?;

        let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
            currency,
            &account.chain_code,
            main_symbol,
        )
        .await?;

        match self {
            Self::Ethereum(chain) => {
                let owner = member.get_owner_str_vec();

                let params = operations::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold,
                )?
                .with_nonce()
                .with_owners(owner)?;

                let gas_limit = chain.estimate_gas(params).await?;

                let gas_oracle = domain::chain::transaction::ChainTransaction::gas_oracle(
                    &account.chain_code,
                    &chain.provider,
                    backend,
                )
                .await?;

                let fee = FeeDetails::try_from((gas_oracle, gas_limit.consume))?
                    .to_resp(token_currency, currency);
                Ok(wallet_utils::serde_func::serde_to_string(&fee)?)
            }
            Self::BitCoin(_chain) => Ok("0".to_string()),
            Self::Solana(chain) => {
                let owners = member.get_owner_str_vec();

                let salt = sol::consts::TEMP_SOL_KEYPAIR;
                let params = sol::operations::multisig::account::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold as u8,
                    owners,
                    salt.to_string(),
                    chain.get_provider(),
                )?;

                let instructions = params.instructions().await?;
                // check transaction_fee
                let fee = chain
                    .estimate_fee_v1(&instructions, &params)
                    .await?
                    .transaction_fee();

                CommonFeeDetails::new(fee, token_currency, currency).to_json_str()
            }
            Self::Tron(chain) => {
                // check account is init an chain
                let account_info = chain
                    .get_provider()
                    .account_info(&account.initiator_addr)
                    .await?;
                if account_info.address.is_empty() {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::AddressNotInit,
                    ))?;
                }

                let params = tron::operations::multisig::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold as u8,
                    member.get_owner_str_vec(),
                )?;
                let mut consumer = chain.simple_fee(&account.initiator_addr, 1, params).await?;

                let chain_parameter = chain.provider.chain_params().await?;
                consumer.set_extra_fee(chain_parameter.update_account_fee());

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
                    currency,
                    &account.chain_code,
                    main_symbol,
                )
                .await?;

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                Ok(wallet_utils::serde_func::serde_to_string(&res)?)
            }
        }
    }

    pub async fn build_multisig_fee(
        &self,
        req: &MultisigQueueFeeParams,
        account: &MultisigAccountEntity,
        decimal: u8,
        token: Option<String>,
        main_symbol: &str,
    ) -> Result<String, crate::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();
        match self {
            MultisigAdapter::Solana(solana_chain) => {
                let base = sol::operations::transfer::TransferOpt::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    token,
                    decimal,
                    solana_chain.get_provider(),
                )?;

                let params = sol::operations::multisig::transfer::BuildTransactionOpt::new(
                    &account.authority_addr,
                    account.threshold as u8,
                    &account.initiator_addr,
                    base,
                )?;

                // transaction params
                let args = params.build_transaction_arg().await?;
                let instructions = params.instructions(&args).await?;

                // create transaction fee
                let base_fee = solana_chain.estimate_fee_v1(&instructions, &params).await?;
                let fee = params
                    .create_transaction_fee(&args.transaction_message, base_fee)
                    .await?;

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
                    currency,
                    &req.chain_code,
                    main_symbol,
                )
                .await?;

                let fee = CommonFeeDetails::new(fee.transaction_fee(), token_currency, currency);
                Ok(serde_func::serde_to_string(&fee)?)
            }
            _ => Ok("".to_string()),
        }
    }

    pub async fn build_multisig_tx(
        &self,
        req: &TransferParams,
        account: &MultisigAccountEntity,
        decimal: u8,
        token: Option<String>,
        key: ChainPrivateKey,
    ) -> Result<types::MultisigTxResp, crate::ServiceError> {
        let value = ChainTransaction::check_min_transfer(&req.value, decimal)?;

        match self {
            Self::Ethereum(chain) => {
                // check balance
                let balance = chain.balance(&req.from, token.clone()).await?;
                let _ = domain::chain::transaction::ChainTransaction::check_eth_balance(
                    &req.from,
                    balance,
                    token.as_deref(),
                    chain,
                    value,
                )
                .await?;

                let params = eth::operations::MultisigTransferOpt::new(&req.from, &req.to, value)?
                    .with_token(token)?;

                Ok(chain.build_multisig_tx(params).await?)
            }
            Self::BitCoin(chain) => {
                let params = btc::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    account.address_type(),
                    chain.network,
                )?;

                Ok(chain
                    .build_multisig_tx(params)
                    .await
                    .map_err(domain::chain::transaction::ChainTransaction::handle_btc_fee_error)?)
            }
            Self::Solana(chain) => {
                // check multisig account balance
                let multisig_balance = chain.balance(&req.from, token.clone()).await?;
                if multisig_balance < value {
                    return Err(crate::BusinessError::Chain(
                        crate::ChainError::InsufficientBalance,
                    ))?;
                }
                let base = sol::operations::transfer::TransferOpt::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    token,
                    decimal,
                    chain.get_provider(),
                )?;

                let params = sol::operations::multisig::transfer::BuildTransactionOpt::new(
                    &account.authority_addr,
                    account.threshold as u8,
                    &account.initiator_addr,
                    base,
                )?;

                // transaction params
                let args = params.build_transaction_arg().await?;
                let instructions = params.instructions(&args).await?;

                // create transaction fee
                let base_fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let fee = params
                    .create_transaction_fee(&args.transaction_message, base_fee)
                    .await?;
                // check balance
                let balance = chain.balance(&account.initiator_addr, None).await?;
                domain::chain::transaction::ChainTransaction::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                // execute build transfer transaction
                let pda = params.multisig_pda;
                let tx_hash = chain
                    .exec_transaction(params, key, None, instructions, 0)
                    .await?;

                Ok(args.get_raw_data(pda, tx_hash)?)
            }
            Self::Tron(chain) => {
                let expiration = req.expiration.unwrap_or(1) * 3600;

                if let Some(token) = token {
                    let mut params = tron::operations::transfer::ContractTransferOpt::new(
                        &token,
                        &req.from,
                        &req.to,
                        value,
                        req.notes.clone(),
                    )?;
                    // query balance and check balance
                    let balance = chain.balance(&req.from, Some(token)).await?;
                    if balance < value {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientBalance,
                        ))?;
                    }

                    let provider = chain.get_provider();
                    let constant = params.constant_contract(provider).await?;
                    let consumer = provider
                        .contract_fee(constant, account.threshold as u8, &req.from)
                        .await?;
                    params.set_fee_limit(consumer);

                    Ok(chain
                        .build_multisig_transaction(params, expiration as u64)
                        .await?)
                } else {
                    let params = tron::operations::transfer::TransferOpt::new(
                        &req.from,
                        &req.to,
                        value,
                        req.notes.clone(),
                    )?;

                    let fee = chain
                        .simulate_simple_fee(&req.from, &req.to, 1, params.clone())
                        .await?;
                    let fee = unit::u256_from_str(&fee.transaction_fee_i64().to_string())?;
                    let balance = chain.balance(&req.from, None).await?;
                    if balance < value + fee {
                        return Err(crate::BusinessError::Chain(
                            crate::ChainError::InsufficientBalance,
                        ))?;
                    }

                    Ok(chain
                        .build_multisig_transaction(params, expiration as u64)
                        .await?)
                }
            }
        }
    }

    pub async fn sign_fee(
        &self,
        account: &MultisigAccountEntity,
        address: &str,
        raw_data: &str,
        main_symbol: &str,
    ) -> Result<String, crate::ServiceError> {
        match self {
            MultisigAdapter::Solana(solana_chain) => {
                let currency = crate::app_state::APP_STATE.read().await;
                let currency = currency.currency();

                let params = sol::operations::multisig::transfer::SignTransactionOpt::new(
                    address,
                    raw_data.to_string(),
                )?;

                let instructions = params.instructions().await?;
                let fee = solana_chain.estimate_fee_v1(&instructions, &params).await?;

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
                    currency,
                    &account.chain_code,
                    main_symbol,
                )
                .await?;

                let fee = CommonFeeDetails::new(fee.transaction_fee(), token_currency, currency);
                Ok(serde_func::serde_to_string(&fee)?)
            }
            _ => Ok(" ".to_string()),
        }
    }

    pub async fn sign_multisig_tx(
        &self,
        account: &MultisigAccountEntity,
        address: &str,
        key: ChainPrivateKey,
        raw_data: &str,
    ) -> Result<types::MultisigSignResp, crate::ServiceError> {
        match self {
            Self::Ethereum(_chain) => {
                use std::str::FromStr as _;
                let operate = eth::operations::MultisigPayloadOpt::from_str(raw_data)?;
                Ok(operate.sign_message(key)?)
            }
            Self::BitCoin(chain) => {
                let params = btc::operations::multisig::MultisigTransactionOpt::new(
                    account.address.clone(),
                    "0".to_string(),
                    &account.salt,
                    raw_data,
                    &account.address_type,
                )?;
                Ok(chain.sign_multisig_tx(params, key).await?)
            }
            Self::Solana(chain) => {
                let balance = chain.balance(address, None).await?;
                let params = sol::operations::multisig::transfer::SignTransactionOpt::new(
                    address,
                    raw_data.to_string(),
                )?;

                let instructions = params.instructions().await?;
                let fee = chain.estimate_fee_v1(&instructions, &params).await?;
                domain::chain::transaction::ChainTransaction::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                Ok(chain.sign_with_res(instructions, params, key).await?)
            }
            Self::Tron(_chain) => {
                let res =
                    tron::operations::multisig::TransactionOpt::sign_transaction(raw_data, key)?;
                Ok(res)
            }
        }
    }

    pub async fn estimate_fee(
        &self,
        queue: &MultisigQueueEntity,
        assets: &AssetsEntity,
        backend: &wallet_transport_backend::api::BackendApi,
        sign_list: Vec<String>,
        main_symbol: &str,
    ) -> Result<String, crate::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
            currency,
            &queue.chain_code,
            main_symbol,
        )
        .await?;

        match self {
            Self::Ethereum(chain) => {
                let pool = crate::manager::Context::get_global_sqlite_pool()?;
                let value = unit::convert_to_u256(&queue.value, assets.decimals)?;
                let multisig_account = domain::multisig::MultisigDomain::account_by_address(
                    &queue.from_addr,
                    true,
                    &pool,
                )
                .await?;

                let gas_oracle = domain::chain::transaction::ChainTransaction::gas_oracle(
                    &queue.chain_code,
                    &chain.provider,
                    backend,
                )
                .await?;

                let params = eth::operations::MultisigTransferOpt::new(
                    &queue.from_addr,
                    &queue.to_addr,
                    value,
                )?
                .with_token(assets.token_address())?
                .exec_params(
                    &multisig_account.initiator_addr,
                    queue.raw_data.clone(),
                    sign_list.join(""),
                )?;

                let fee = chain.estimate_gas(params).await?;
                let fee = FeeDetails::try_from((gas_oracle, fee.consume))?
                    .to_resp(token_currency, currency);

                Ok(wallet_utils::serde_func::serde_to_string(&fee)?)
            }
            Self::BitCoin(chain) => {
                let pool = crate::manager::Context::get_global_sqlite_pool()?;
                let multisig_account =
                    domain::multisig::MultisigDomain::account_by_id(&queue.account_id, pool)
                        .await?;

                let params = btc::operations::transfer::TransferArg::new(
                    &queue.from_addr,
                    &queue.to_addr,
                    &queue.value,
                    multisig_account.address_type(),
                    chain.network,
                )?;

                let fee = chain
                    .estimate_fee(params)
                    .await
                    .map_err(domain::chain::transaction::ChainTransaction::handle_btc_fee_error)?
                    .transaction_fee_f64();

                CommonFeeDetails::new(fee, token_currency, currency).to_json_str()
            }
            Self::Solana(chain) => {
                let params = sol::operations::multisig::transfer::ExecMultisigOpt::new(
                    &queue.from_addr,
                    queue.raw_data.to_string(),
                )?;

                let instructions = params.instructions().await?;
                let fee = chain
                    .estimate_fee_v1(&instructions, &params)
                    .await?
                    .transaction_fee();
                CommonFeeDetails::new(fee, token_currency, currency).to_json_str()
            }
            Self::Tron(chain) => {
                let signature_num = sign_list.len() as u8;
                let value = unit::convert_to_u256(&queue.value, assets.decimals)?;
                let memo = (!queue.notes.is_empty()).then(|| queue.notes.clone());

                let consumer = if let Some(token) = assets.token_address() {
                    let params = tron::operations::transfer::ContractTransferOpt::new(
                        &token,
                        &queue.from_addr,
                        &queue.to_addr,
                        value,
                        memo,
                    )?;

                    chain
                        .contract_fee(&queue.from_addr, signature_num, params)
                        .await?
                } else {
                    let memo = if queue.notes.is_empty() {
                        None
                    } else {
                        Some(queue.notes.clone())
                    };

                    let params = tron::operations::transfer::TransferOpt::new(
                        &queue.from_addr,
                        &queue.to_addr,
                        value,
                        memo,
                    )?;

                    chain
                        .simulate_simple_fee(
                            &queue.from_addr,
                            &queue.to_addr,
                            signature_num,
                            params,
                        )
                        .await?
                };

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency(
                    currency,
                    &queue.chain_code,
                    main_symbol,
                )
                .await?;

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                Ok(wallet_utils::serde_func::serde_to_string(&res)?)
            }
        }
    }
}
