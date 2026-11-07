use super::{TIME_OUT, tron_tx};
use crate::{
    dispatch,
    domain::{
        self,
        chain::transaction::{ChainTransDomain, DEFAULT_UNITS},
    },
    response_vo::{
        self, CommonFeeDetails, FeeDetails, MultisigQueueFeeParams, TransferParams, TronFeeDetails,
    },
};
use std::collections::HashMap;
use wallet_chain_interact::{
    self as chain, BillResourceConsume,
    btc::{self, MultisigSignParams},
    eth::{self, operations},
    sol::{self, operations::SolInstructionOperation},
    tron::{self, operations::TronTxOperation},
    types::{self, ChainPrivateKey},
};
use wallet_database::entities::{
    asset_token_key::AssetTokenKey, assets::AssetsEntity, bill::BillKind, coin::CoinEntity,
    multisig_account::MultisigAccountEntity, multisig_member::MultisigMemberEntities,
    multisig_queue::MultisigQueueEntity, permission::PermissionEntity,
};
use wallet_transport::client::{HttpClient, RpcClient};
use wallet_types::chain::chain::ChainCode as ChainType;
use wallet_utils::{serde_func, unit};

pub enum MultisigAdapter {
    BitCoin(chain::btc::BtcChain),
    Ethereum(chain::eth::EthChain),
    Solana(chain::sol::SolanaChain),
    Tron(chain::tron::TronChain),
    Ltc(chain::ltc::LtcChain),
    Doge(chain::dog::DogChain),
    Ton(chain::ton::chain::TonChain),
    Sui(chain::sui::SuiChain),
}

#[cfg(test)]
mod evm_chain_tests {
    use super::MultisigAdapter;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    use wallet_database::entities::chain::ChainWithNode;
    use wallet_types::chain::chain::ChainCode;

    fn node(chain_code: &str) -> ChainWithNode {
        ChainWithNode {
            chain_code: chain_code.to_owned(),
            rpc_url: "http://127.0.0.1:8545".to_owned(),
            network: "mainnet".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn polygon_uses_evm_multisig_adapter_without_network_access() {
        let adapter = MultisigAdapter::new(ChainCode::Polygon, node("polygon"), None).unwrap();
        match adapter {
            MultisigAdapter::Ethereum(chain) => assert_eq!(chain.chain_code, ChainCode::Polygon),
            _ => panic!("Polygon must use the EVM multisig adapter"),
        }
    }

    #[test]
    fn avalanche_uses_evm_multisig_adapter_without_network_access() {
        let adapter = MultisigAdapter::new(ChainCode::Avalanche, node("avalanche"), None).unwrap();
        match adapter {
            MultisigAdapter::Ethereum(chain) => assert_eq!(chain.chain_code, ChainCode::Avalanche),
            _ => panic!("Avalanche must use the EVM multisig adapter"),
        }
    }

    #[test]
    fn arbitrum_uses_evm_multisig_adapter_without_network_access() {
        let adapter = MultisigAdapter::new(ChainCode::Arbitrum, node("arbitrum"), None).unwrap();
        match adapter {
            MultisigAdapter::Ethereum(chain) => assert_eq!(chain.chain_code, ChainCode::Arbitrum),
            _ => panic!("Arbitrum must use the EVM multisig adapter"),
        }
    }

    #[test]
    fn base_uses_evm_multisig_adapter_without_network_access() {
        let adapter = MultisigAdapter::new(ChainCode::Base, node("base"), None).unwrap();
        match adapter {
            MultisigAdapter::Ethereum(chain) => assert_eq!(chain.chain_code, ChainCode::Base),
            _ => panic!("Base must use the EVM multisig adapter"),
        }
    }

    #[test]
    fn scroll_uses_evm_multisig_adapter_without_network_access() {
        let adapter = MultisigAdapter::new(ChainCode::Scroll, node("scroll"), None).unwrap();
        match adapter {
            MultisigAdapter::Ethereum(chain) => assert_eq!(chain.chain_code, ChainCode::Scroll),
            _ => panic!("Scroll must use the EVM multisig adapter"),
        }
    }

    #[test]
    fn fantom_remains_outside_this_rollout() {
        assert!(MultisigAdapter::new(ChainCode::Fantom, node("fantom"), None).is_err());
    }

    #[tokio::test]
    async fn polygon_adapter_rejects_wrong_rpc_before_safe_lookup() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut polygon_node = node("polygon");
        polygon_node.rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let (body_start, body_len) = loop {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, len);
                }
            };
            while bytes.len() < body_start + body_len {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            let request: serde_json::Value =
                serde_json::from_slice(&bytes[body_start..body_start + body_len]).unwrap();
            assert_eq!(request["method"], "eth_chainId");
            let response = serde_json::json!({
                "jsonrpc": "2.0", "id": request["id"], "result": "0x1"
            })
            .to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        });

        let adapter = MultisigAdapter::new(ChainCode::Polygon, polygon_node, None).unwrap();
        let error = adapter.preflight_safe_deployment().await.unwrap_err();
        assert!(error.to_string().contains("expected 137"));
        worker.join().unwrap();
    }
}
impl std::fmt::Display for MultisigAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MultisigAdapter::BitCoin(_) => write!(f, "bitcoin"),
            MultisigAdapter::Ethereum(_) => write!(f, "ethereum"),
            MultisigAdapter::Solana(_) => write!(f, "solana"),
            MultisigAdapter::Tron(_) => write!(f, "tron"),
            MultisigAdapter::Ltc(_) => write!(f, "ltc"),
            MultisigAdapter::Doge(_) => write!(f, "doge"),
            MultisigAdapter::Ton(_) => write!(f, "ton"),
            MultisigAdapter::Sui(_) => write!(f, "sui"),
        }
    }
}
impl MultisigAdapter {
    pub(crate) async fn preflight_safe_deployment(&self) -> Result<(), chain::Error> {
        if let Self::Ethereum(chain) = self {
            if matches!(
                chain.chain_code,
                ChainType::Polygon
                    | ChainType::Avalanche
                    | ChainType::Arbitrum
                    | ChainType::Base
                    | ChainType::Scroll
            ) {
                chain.preflight_safe_deployment().await?;
            }
        }
        Ok(())
    }

    pub fn new(
        chain_code: ChainType,
        chian_node: wallet_database::entities::chain::ChainWithNode,
        header_opt: Option<HashMap<String, String>>,
    ) -> Result<MultisigAdapter, crate::error::service::ServiceError> {
        let network =
            crate::domain::chain::ChainDomain::network_kind_from_node_network(&chian_node.network);

        let timeout = Some(std::time::Duration::from_secs(TIME_OUT));
        match chain_code {
            ChainType::Bitcoin => {
                let config = chain::btc::provider::ProviderConfig {
                    rpc_url: chian_node.rpc_url.clone(),
                    rpc_auth: None,
                    http_url: chian_node.rpc_url,
                    http_api_key: None,
                };

                let btc_chain = chain::btc::BtcChain::new(config, network, header_opt, timeout)?;
                Ok(MultisigAdapter::BitCoin(btc_chain))
            }
            ChainType::Ethereum
            | ChainType::BnbSmartChain
            | ChainType::Polygon
            | ChainType::Avalanche
            | ChainType::Arbitrum
            | ChainType::Base
            | ChainType::Scroll => {
                let rpc_client = RpcClient::new(&chian_node.rpc_url, header_opt, timeout)?;
                let provider = eth::Provider::new(rpc_client)?;

                let eth_chain = chain::eth::EthChain::new(provider, network, chain_code)?;
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
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    chain_code.to_string(),
                ),
            ))?,
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
    ) -> Result<types::FetchMultisigAddressResp, crate::error::service::ServiceError> {
        match self {
            Self::Ethereum(chain) => {
                self.preflight_safe_deployment().await?;
                let params = operations::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold,
                )?
                .with_nonce()
                .with_owners(member.get_owner_str_vec())?;

                Ok(chain.multisig_account(params).await?)
            }
            Self::BitCoin(chain) => {
                let params = btc::operations::multisig::MultisigAccountOpt::new(
                    account.threshold as u8,
                    member.get_owner_pubkey(),
                    &account.address_type,
                )?;
                Ok(chain.multisig_address(params).await?)
            }
            Self::Solana(_chain) => {
                Ok(sol::operations::multisig::account::MultisigAccountOpt::multisig_address()?)
            }
            Self::Tron(_chain) => Ok(types::FetchMultisigAddressResp {
                authority_address: "".to_string(),
                multisig_address: account.address.to_string(),
                salt: "".to_string(),
            }),
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
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
    ) -> Result<(String, String), crate::error::service::ServiceError> {
        match self {
            Self::Ethereum(chain) => {
                self.preflight_safe_deployment().await?;
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
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::InsufficientFeeBalance,
                    ))?;
                }

                let tx_hash = chain.exec_transaction(params, fee_setting, key, None).await?;
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
                domain::chain::transaction::ChainTransDomain::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                let tx_hash = chain.exec_transaction(params, key, None, instructions, 0).await?;

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
                    .transfer_fee(&account.initiator_addr, None, &tx.raw_data_hex, 1)
                    .await?;

                let chain_parameter = chain.provider.chain_params().await?;
                consumer.set_extra_fee(chain_parameter.update_account_fee());

                let fee = consumer.transaction_fee_i64();
                let account_info = provider.account_info(&account.initiator_addr).await?;
                if account_info.balance < fee {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(account.initiator_addr.clone())
                                .chain_code(account.chain_code.clone())
                                .value("0".to_string())
                                .balance(account_info.balance.to_string())
                                .need(fee.to_string())
                                .fee(fee.to_string())
                                .reason(
                                    "main coin balance is insufficient for multisig deploy fee",
                                ),
                        ),
                    ))?;
                }

                let consumer = BillResourceConsume::new_tron(consumer.bandwidth.consumer as u64, 0);
                let tx_hash = chain.exec_transaction_v1(tx, key).await?;

                Ok((tx_hash, consumer.to_json_str()?))
                // Ok(chain.exec_transaction_v1(tx, key).await?)
            }
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
        }
    }

    pub async fn deploy_multisig_fee(
        &self,
        ctx: &'static crate::context::Context,
        account: &MultisigAccountEntity,
        member: MultisigMemberEntities,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        self.deploy_multisig_fee_with_ctx(ctx, account, member, main_symbol, backend.as_ref()).await
    }

    pub async fn deploy_multisig_fee_with_ctx(
        &self,
        ctx: &'static crate::context::Context,
        account: &MultisigAccountEntity,
        member: MultisigMemberEntities,
        main_symbol: &str,
        backend_api: &wallet_transport_backend::api::BackendApi,
    ) -> Result<String, crate::error::service::ServiceError> {
        let currency_lock = crate::app_state::APP_STATE.read().await;
        let currency = currency_lock.currency();

        let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
            ctx,
            currency,
            &account.chain_code,
            main_symbol,
            AssetTokenKey::Native,
        )
        .await?;

        match self {
            Self::Ethereum(chain) => {
                self.preflight_safe_deployment().await?;
                let owner = member.get_owner_str_vec();

                let params = operations::MultisigAccountOpt::new(
                    &account.initiator_addr,
                    account.threshold,
                )?
                .with_nonce()
                .with_owners(owner)?;

                let gas_limit = chain.estimate_gas(params).await?;

                let gas_oracle = domain::chain::transaction::ChainTransDomain::gas_oracle(
                    &account.chain_code,
                    &chain.provider,
                    backend_api,
                )
                .await?;

                let fee = FeeDetails::try_from((gas_oracle, gas_limit.consume))?
                    .to_resp(token_currency, currency)?;
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
                let fee = chain.estimate_fee_v1(&instructions, &params).await?.transaction_fee();

                CommonFeeDetails::new(fee, token_currency, currency)?.to_json_str()
            }
            Self::Tron(chain) => {
                // check account is init an chain
                let account_info =
                    chain.get_provider().account_info(&account.initiator_addr).await?;
                if account_info.address.is_empty() {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::AddressNotInit,
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

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
                    ctx,
                    currency,
                    &account.chain_code,
                    main_symbol,
                    AssetTokenKey::Native,
                )
                .await?;

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                Ok(wallet_utils::serde_func::serde_to_string(&res)?)
            }
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
        }
    }

    pub async fn build_multisig_fee(
        &self,
        ctx: &'static crate::context::Context,
        req: &MultisigQueueFeeParams,
        account: &MultisigAccountEntity,
        decimal: u8,
        token: Option<String>,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let currency: tokio::sync::RwLockReadGuard<'_, crate::app_state::AppState> =
            crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
            ctx,
            currency,
            &req.chain_code,
            main_symbol,
            AssetTokenKey::Native,
        )
        .await?;
        match self {
            MultisigAdapter::Solana(solana_chain) => {
                let base = sol::operations::transfer::TransferOpt::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    token.clone(),
                    decimal,
                    solana_chain.get_provider(),
                )?;

                let params = sol::operations::multisig::transfer::BuildTransactionOpt::new(
                    &account.authority_addr,
                    account.member_num as usize,
                    &account.initiator_addr,
                    base,
                )?;

                // transaction params
                let args = params.build_transaction_arg().await?;
                let instructions = params.instructions(&args).await?;

                // create transaction fee
                let base_fee = solana_chain.estimate_fee_v1(&instructions, &params).await?;
                let mut fee_setting =
                    params.create_transaction_fee(&args.transaction_message, base_fee).await?;

                ChainTransDomain::sol_priority_fee(&mut fee_setting, token.as_ref(), DEFAULT_UNITS);

                let fee =
                    CommonFeeDetails::new(fee_setting.transaction_fee(), token_currency, currency)?;
                Ok(serde_func::serde_to_string(&fee)?)
            }
            MultisigAdapter::BitCoin(chain) => {
                let params = btc::operations::transfer::TransferArg::new(
                    &req.from,
                    &req.to,
                    &req.value,
                    account.address_type(),
                    chain.network,
                )?
                .with_spend_all(req.spend_all.unwrap_or(false));

                let multisig_parmas = MultisigSignParams::new(
                    account.threshold as i8,
                    account.member_num as i8,
                    account.salt.clone(),
                )
                .with_inner_key(account.authority_addr.clone());

                let fee = chain
                    .estimate_fee(params, Some(multisig_parmas))
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                let fee =
                    CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?;
                Ok(serde_func::serde_to_string(&fee)?)
            }
            _ => Ok("".to_string()),
        }
    }

    pub async fn build_multisig_with_account(
        &self,
        req: &TransferParams,
        account: &MultisigAccountEntity,
        assets: &AssetsEntity,
        key: ChainPrivateKey,
    ) -> Result<types::MultisigTxResp, crate::error::service::ServiceError> {
        let decimal = assets.decimals;
        let token = assets.token_key().to_chain_token_option();

        let value = ChainTransDomain::check_min_transfer(&req.value, decimal)?;

        match self {
            Self::Ethereum(chain) => {
                // check balance
                let balance = chain.balance(&req.from, token.clone()).await?;
                let _ = domain::chain::transaction::ChainTransDomain::check_eth_balance(
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
                )?
                .with_spend_all(req.spend_all);

                let multisig_parmas = MultisigSignParams::new(
                    account.threshold as i8,
                    account.member_num as i8,
                    account.salt.clone(),
                )
                .with_inner_key(account.authority_addr.clone());

                Ok(chain
                    .build_multisig_tx(params, multisig_parmas)
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?)
            }
            Self::Solana(chain) => {
                // check multisig account balance
                let multisig_balance = chain.balance(&req.from, token.clone()).await?;
                if multisig_balance < value {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(req.from.clone())
                                .to_addr(req.to.clone())
                                .chain_code(req.chain_code.clone())
                                .token_addr(token.clone().unwrap_or_default())
                                .value(value.to_string())
                                .balance(multisig_balance.to_string())
                                .need(value.to_string())
                                .reason(
                                    "token balance is insufficient for multisig solana transaction",
                                ),
                        ),
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
                    account.member_num as usize,
                    &account.initiator_addr,
                    base,
                )?;

                // transaction params
                let args = params.build_transaction_arg().await?;
                let instructions = params.instructions(&args).await?;

                // create transaction fee
                let base_fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let fee =
                    params.create_transaction_fee(&args.transaction_message, base_fee).await?;
                // check balance
                let balance = chain.balance(&account.initiator_addr, None).await?;
                domain::chain::transaction::ChainTransDomain::check_sol_transaction_fee(
                    balance,
                    fee.original_fee(),
                )?;

                // execute build transfer transaction
                let pda = params.multisig_pda;
                let tx_hash = chain.exec_transaction(params, key, None, instructions, 0).await?;

                Ok(args.get_raw_data(pda, tx_hash)?)
            }
            Self::Tron(chain) => {
                tracing::info!(
                    msq_step = "adapter_tron_build_with_account_start",
                    from = %req.from,
                    to = %req.to,
                    chain_code = %req.chain_code,
                    symbol = %req.symbol,
                    token = ?token,
                    threshold = account.threshold,
                    "enter tron multisig build with account"
                );
                let balance = chain.balance(&req.from, token.clone()).await?;
                if balance < value {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(req.from.clone())
                                .to_addr(req.to.clone())
                                .chain_code(req.chain_code.clone())
                                .token_addr(token.clone().unwrap_or_default())
                                .value(value.to_string())
                                .balance(balance.to_string())
                                .need(value.to_string())
                                .reason(
                                    "token balance is insufficient for tron multisig transaction",
                                ),
                        ),
                    ))?;
                }

                let rs = tron_tx::build_build_tx(
                    req,
                    token,
                    value,
                    chain,
                    account.threshold as i64,
                    None,
                )
                .await?;
                tracing::info!(
                    msq_step = "adapter_tron_build_with_account_done",
                    from = %req.from,
                    to = %req.to,
                    chain_code = %req.chain_code,
                    tx_hash = %rs.tx_hash,
                    "tron multisig build with account done"
                );
                Ok(rs)
            }
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
        }
    }

    // 权限构建交易队列
    pub async fn build_multisig_with_permission(
        &self,
        req: &TransferParams,
        p: &PermissionEntity,
        coin: &CoinEntity,
    ) -> Result<types::MultisigTxResp, crate::error::service::ServiceError> {
        let decimal = coin.decimals;
        let token = coin.token_address.to_chain_token_option();

        let value = ChainTransDomain::check_min_transfer(&req.value, decimal)?;

        match self {
            Self::Tron(chain) => {
                tracing::info!(
                    msq_step = "adapter_tron_build_with_permission_start",
                    from = %req.from,
                    to = %req.to,
                    chain_code = %req.chain_code,
                    symbol = %req.symbol,
                    token = ?token,
                    permission_id = p.active_id,
                    threshold = p.threshold,
                    "enter tron multisig build with permission"
                );
                let balance = chain.balance(&req.from, token.clone()).await?;
                if balance < value {
                    return Err(crate::error::business::BusinessError::Chain(
                        crate::error::business::chain::ChainError::insufficient_balance_with_detail(
                            crate::error::business::chain::InsufficientBalanceDetail::new()
                                .from_addr(req.from.clone())
                                .to_addr(req.to.clone())
                                .chain_code(req.chain_code.clone())
                                .token_addr(token.clone().unwrap_or_default())
                                .value(value.to_string())
                                .balance(balance.to_string())
                                .need(value.to_string())
                                .reason("token balance is insufficient for tron multisig permission transaction"),
                        ),
                    ))?;
                }

                let permission_id = Some(p.active_id);
                let rs =
                    tron_tx::build_build_tx(req, token, value, chain, p.threshold, permission_id)
                        .await?;
                tracing::info!(
                    msq_step = "adapter_tron_build_with_permission_done",
                    from = %req.from,
                    to = %req.to,
                    chain_code = %req.chain_code,
                    tx_hash = %rs.tx_hash,
                    "tron multisig build with permission done"
                );
                Ok(rs)
            }
            _ => Err(crate::error::business::BusinessError::Permission(
                crate::error::business::permission::PermissionError::UnSupportPermissionChain,
            ))?,
        }
    }

    pub async fn sign_fee(
        &self,
        ctx: &'static crate::context::Context,
        account: &MultisigAccountEntity,
        address: &str,
        raw_data: &str,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
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

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
                    ctx,
                    currency,
                    &account.chain_code,
                    main_symbol,
                    AssetTokenKey::Native,
                )
                .await?;

                let fee = CommonFeeDetails::new(fee.transaction_fee(), token_currency, currency)?;
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
    ) -> Result<types::MultisigSignResp, crate::error::service::ServiceError> {
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
                domain::chain::transaction::ChainTransDomain::check_sol_transaction_fee(
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
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
        }
    }

    pub async fn estimate_fee(
        &self,
        ctx: &'static crate::context::Context,
        queue: &MultisigQueueEntity,
        coin: &CoinEntity,
        backend: &wallet_transport_backend::api::BackendApi,
        sign_list: Vec<String>,
        main_symbol: &str,
    ) -> Result<String, crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        self.estimate_fee_with_ctx(ctx, queue, coin, backend, sign_list, main_symbol, &pool).await
    }

    pub async fn estimate_fee_with_ctx(
        &self,
        ctx: &'static crate::context::Context,
        queue: &MultisigQueueEntity,
        coin: &CoinEntity,
        backend: &wallet_transport_backend::api::BackendApi,
        sign_list: Vec<String>,
        main_symbol: &str,
        pool: &std::sync::Arc<sqlx::SqlitePool>,
    ) -> Result<String, crate::error::service::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
            ctx,
            currency,
            &queue.chain_code,
            main_symbol,
            AssetTokenKey::Native,
        )
        .await?;

        match self {
            Self::Ethereum(chain) => {
                let value = unit::convert_to_u256(&queue.value, coin.decimals)?;
                let multisig_account = domain::multisig::MultisigDomain::account_by_address(
                    &queue.from_addr,
                    true,
                    &pool,
                )
                .await?;

                let gas_oracle = domain::chain::transaction::ChainTransDomain::gas_oracle(
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
                .with_token(coin.token_address.to_chain_token_option())?
                .exec_params(
                    &multisig_account.initiator_addr,
                    queue.raw_data.clone(),
                    sign_list.join(""),
                )?;

                let fee = chain.estimate_gas(params).await?;
                let fee = FeeDetails::try_from((gas_oracle, fee.consume))?
                    .to_resp(token_currency, currency)?;

                Ok(wallet_utils::serde_func::serde_to_string(&fee)?)
            }
            Self::BitCoin(chain) => {
                let multisig_account = domain::multisig::MultisigDomain::account_by_id(
                    &queue.account_id,
                    pool.clone(),
                )
                .await?;

                let multisig_parmas = MultisigSignParams::new(
                    multisig_account.threshold as i8,
                    multisig_account.member_num as i8,
                    multisig_account.salt.clone(),
                )
                .with_inner_key(multisig_account.authority_addr.clone());

                let fee = chain
                    .estimate_multisig_fee(
                        &queue.raw_data,
                        multisig_parmas,
                        &multisig_account.address_type,
                    )
                    .await
                    .map_err(domain::chain::transaction::ChainTransDomain::handle_btc_fee_error)?;

                CommonFeeDetails::new(fee.transaction_fee_f64(), token_currency, currency)?
                    .to_json_str()
            }
            Self::Solana(chain) => {
                let params = sol::operations::multisig::transfer::ExecMultisigOpt::new(
                    &queue.from_addr,
                    queue.raw_data.to_string(),
                )?;

                let instructions = params.instructions().await?;
                let mut fee = chain.estimate_fee_v1(&instructions, &params).await?;
                let token = queue.token_addr.to_chain_token_option();
                ChainTransDomain::sol_priority_fee(&mut fee, token.as_ref(), 200_000);

                CommonFeeDetails::new(fee.transaction_fee(), token_currency, currency)?
                    .to_json_str()
            }
            Self::Tron(chain) => {
                let signature_num = sign_list.len() as u8;
                let value = unit::convert_to_u256(&queue.value, coin.decimals)?;
                let memo = (!queue.notes.is_empty()).then(|| queue.notes.clone());

                let mut consumer = if let Some(token) = coin.token_address.to_chain_token_option() {
                    let params = tron::operations::transfer::ContractTransferOpt::new(
                        &token,
                        &queue.from_addr,
                        &queue.to_addr,
                        value,
                        memo,
                    )?;

                    chain.contract_fee(&queue.from_addr, signature_num, params).await?
                } else {
                    let params =
                        tron::operations::multisig::TransactionOpt::data_from_str(&queue.raw_data)?;

                    let to = (!queue.to_addr.is_empty()).then_some(queue.to_addr.as_str());

                    chain
                        .provider
                        .transfer_fee(&queue.from_addr, to, &params.raw_data_hex, signature_num)
                        .await?
                };

                let token_currency = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
                    ctx,
                    currency,
                    &queue.chain_code,
                    main_symbol,
                    AssetTokenKey::Native,
                )
                .await?;

                if queue.transfer_type == BillKind::UpdatePermission.to_i8() {
                    let chain = chain.provider.chain_params().await?;
                    consumer.set_extra_fee(chain.update_account_fee());
                }

                let res = TronFeeDetails::new(consumer, token_currency, currency)?;
                Ok(wallet_utils::serde_func::serde_to_string(&res)?)
            }
            _ => Err(crate::error::business::BusinessError::MultisigAccount(
                crate::error::business::multisig_account::MultisigAccountError::NotSupportChain(
                    self.to_string(),
                ),
            ))?,
        }
    }
}
