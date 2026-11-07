pub mod adapter;
pub mod swap;
pub mod transaction;
pub(crate) mod tx_visibility;

use super::{account::AccountDomain, assets::AssetsDomain, wallet::WalletDomain};
use crate::{
    context::Context,
    domain::app::config::ConfigDomain,
    infrastructure::{
        chain_node::chain_node_ensurer::ChainNodeEnsurer,
        task_queue::{
            backend::{BackendApiTask, BackendApiTaskData},
            task::Tasks,
        },
    },
    response_vo,
};
use std::collections::HashMap;
use wallet_chain_interact::{
    BillResourceConsume, btc::ParseBtcAddress, dog::ParseDogAddress, eth::FeeSetting,
    ltc::ParseLtcAddress, ton::address::parse_addr_from_bs64_url,
};
use wallet_database::{
    entities::{
        account::{AccountEntity, AccountWalletMapping},
        api_chain::NodeBindType,
        coin::CoinEntity,
    },
    repositories::{account::AccountRepo, chain::ChainRepo, node::NodeRepo},
};
use wallet_transport_backend::request::{AddressBatchInitReq, ChainRpcListReq, TokenQueryPriceReq};
use wallet_types::chain::{
    chain::ChainCode,
    network::{self, NetworkKind},
};
use wallet_utils::address;

pub struct TransferResp {
    pub tx_hash: String,
    pub fee: String,
    pub consumer: Option<BillResourceConsume>,
    pub transaction_time_ms: Option<u128>,
}

fn has_missing_chain_account(
    mappings: &[AccountWalletMapping],
    accounts: &[AccountEntity],
    chain_code: &str,
) -> bool {
    mappings.iter().any(|mapping| {
        !accounts.iter().any(|account| {
            account.wallet_address == mapping.wallet_address
                && account.account_id == mapping.account_id
                && account.chain_code == chain_code
        })
    })
}

impl TransferResp {
    pub fn new(tx_hash: String, fee: String) -> Self {
        Self { tx_hash, fee, consumer: None, transaction_time_ms: None }
    }
    pub fn with_consumer(&mut self, consumer: BillResourceConsume) {
        self.consumer = Some(consumer);
    }
    pub fn with_transaction_time(&mut self, transaction_time_ms: u128) {
        self.transaction_time_ms = Some(transaction_time_ms);
    }

    pub fn resource_consume(&self) -> Result<String, crate::error::service::ServiceError> {
        if let Some(consumer) = &self.consumer {
            Ok(consumer.to_json_str()?)
        } else {
            Ok(String::new())
        }
    }
}

/// Parses a fee setting string into a `FeeSetting` struct.
pub fn pare_fee_setting(
    fee_setting: &str,
) -> Result<FeeSetting, crate::error::service::ServiceError> {
    fee_setting.try_into().and_then(|s: response_vo::EthereumFeeDetails| FeeSetting::try_from(s))
}

pub fn rpc_need_header(_url: &str) -> Result<bool, crate::error::service::ServiceError> {
    // let url = Url::parse(url).expect("Invalid URL");
    // Ok(url.host_str() == Some(wallet_transport_backend::consts::BASE_RPC_URL))
    Ok(true)
}

pub(crate) fn is_evm_chain(chain: ChainCode) -> bool {
    wallet_types::chain::chain::ChainCodes::try_from(wallet_types::constant::chain_type::ETH_TYPE)
        .map(|supported| supported.0.contains(&chain))
        .unwrap_or(false)
}

pub fn check_address(
    address: &str,
    chain: wallet_types::chain::chain::ChainCode,
    network: network::NetworkKind,
) -> Result<(), crate::error::service::ServiceError> {
    match chain {
        wallet_types::chain::chain::ChainCode::Bitcoin => {
            let parse = ParseBtcAddress::new(network);
            parse.parse_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::BnbSmartChain
        | wallet_types::chain::chain::ChainCode::Ethereum
        | wallet_types::chain::chain::ChainCode::Polygon
        | wallet_types::chain::chain::ChainCode::Avalanche
        | wallet_types::chain::chain::ChainCode::Fantom
        | wallet_types::chain::chain::ChainCode::Arbitrum
        | wallet_types::chain::chain::ChainCode::Base
        | wallet_types::chain::chain::ChainCode::Scroll => {
            wallet_utils::address::parse_eth_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::Tron => {
            if wallet_utils::address::is_tron_address(address) {
                true
            } else {
                return Err(crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                ))?;
            }
        }
        wallet_types::chain::chain::ChainCode::Solana => {
            wallet_utils::address::parse_sol_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::Ton => {
            parse_addr_from_bs64_url(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::Litecoin => {
            let parse = ParseLtcAddress::new(network);
            parse.parse_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::Dogcoin => {
            let parse = ParseDogAddress::new(network);
            parse.parse_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
        wallet_types::chain::chain::ChainCode::Sui => {
            address::parse_sui_address(address).map(|_| true).map_err(|_| {
                crate::error::business::BusinessError::Account(
                    crate::error::business::account::AccountError::AddressNotCorrect,
                )
            })?
        }
    };
    Ok(())
}

pub struct ChainDomain;

impl ChainDomain {
    pub(crate) fn network_kind_from_node_network(network: &str) -> NetworkKind {
        match network.to_ascii_lowercase().as_str() {
            "testnet" => NetworkKind::Testnet,
            "mainnet" | "" => NetworkKind::Mainnet,
            other => {
                tracing::warn!(network = other, "unknown node network value, fallback to mainnet");
                NetworkKind::Mainnet
            }
        }
    }

    pub(crate) async fn network_kind_by_chain_code(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<NetworkKind, crate::error::service::ServiceError> {
        let node = Self::get_node_with_ctx(ctx, chain_code).await?;
        Ok(Self::network_kind_from_node_network(&node.network))
    }

    pub(crate) async fn network_kind_by_chain_code_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<NetworkKind, crate::error::service::ServiceError> {
        let node = Self::get_node_with_ctx(ctx, chain_code).await?;
        Ok(Self::network_kind_from_node_network(&node.network))
    }

    pub(crate) async fn upsert_multi_chain_than_toggle_with_ctx(
        ctx: &'static Context,
        chains: wallet_transport_backend::response_vo::chain::ChainList,
    ) -> Result<bool, crate::error::service::ServiceError> {
        // tracing::warn!("upsert_multi_chain_than_toggle, chains: {:#?}", chains);
        let pool = ctx.core_pool()?;

        let mut input = Vec::new();
        let mut chain_codes = Vec::new();
        let mut backend_chain_codes = Vec::new();
        let mut has_new_chain = false;

        let account_wallet_mappings = AccountRepo::account_wallet_mapping(pool.clone()).await?;
        let account_list = AccountRepo::list(pool.clone()).await?;
        let app_version = super::app::config::ConfigDomain::get_app_version(ctx).await?.app_version;

        if account_wallet_mappings.is_empty() {
            return Ok(false);
        }

        for chain in chains.list {
            backend_chain_codes.push(chain.chain_code.clone());
            let Some(master_token_code) = chain.master_token_code else {
                continue;
            };

            let status = match (
                super::app::config::ConfigDomain::compare_versions(
                    &app_version,
                    &chain.app_version_code,
                ),
                chain.enable,
            ) {
                (std::cmp::Ordering::Less, _) => 0,
                (_, true) => 1,
                (_, false) => 0,
            };

            if chain.enable
                && has_missing_chain_account(
                    &account_wallet_mappings,
                    &account_list,
                    &chain.chain_code,
                )
            {
                has_new_chain = true;
            }
            // if local_backend_nodes
            //     .iter()
            //     .any(|node| node.chain_code == chain.chain_code)
            // {
            //     input.push(
            //         wallet_database::entities::chain::ChainCreateVo::new(
            //             &chain.name,
            //             &chain.chain_code,
            //             &[],
            //             &master_token_code,
            //         )
            //         .with_status(status),
            //     );
            // } else if default_nodes
            //     .iter()
            //     .any(|node| node.chain_code == chain.chain_code)
            // {
            //     input.push(
            //         wallet_database::entities::chain::ChainCreateVo::new(
            //             &chain.name,
            //             &chain.chain_code,
            //             &[],
            //             &master_token_code,
            //         )
            //         .with_status(status),
            //     );
            // }

            input.push(
                wallet_database::entities::chain::ChainCreateVo::new(
                    &chain.name,
                    &chain.chain_code,
                    &[],
                    NodeBindType::AutoBackend,
                    &master_token_code,
                )
                .with_status(status),
            );
            if status == 1 {
                chain_codes.push(chain.chain_code);
            }
        }

        ChainRepo::upsert_multi_chain(&pool, input).await?;
        let enabled_chain_codes = crate::default_data::chain::enabled_chain_codes_for_feature(
            crate::config::Config::active_feature_profile(),
            &chain_codes,
            &backend_chain_codes,
        )?;
        Self::toggle_chains_with_ctx(ctx, &enabled_chain_codes).await?;

        if !chain_codes.is_empty() {
            let chain_rpc_list_req = BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::CHAIN_RPC_LIST,
                &ChainRpcListReq::new(chain_codes),
            )?;
            Tasks::new()
                .push(BackendApiTask::BackendApi(chain_rpc_list_req))
                .send_with_ctx(ctx)
                .await?;
        }

        Ok(has_new_chain)
    }

    pub(crate) async fn toggle_chains_with_ctx(
        ctx: &'static Context,
        chain_codes: &[String],
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        ChainRepo::toggle_chains_status(&pool, chain_codes).await?;
        Ok(())
    }

    pub(crate) async fn get_node_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<NodeInfo, crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;
        let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool);
        let node_id = ensurer.ensure_and_get_standard_chain_node(chain_code).await?;

        let node = NodeRepo::detail(&core_pool, &node_id).await?.ok_or(
            crate::error::business::BusinessError::ChainNode(
                crate::error::business::chain_node::ChainNodeError::NodeNotFound,
            ),
        )?;
        Ok(NodeInfo::new(
            &node.chain_code,
            &node.node_id,
            &node.name,
            &node.rpc_url,
            &node.ws_url,
            &node.http_url,
            &node.network,
            node.status,
        ))
    }

    pub(crate) async fn init_chains_assets(
        ctx: &'static crate::context::Context,
        coins: &[CoinEntity],
        req: &mut TokenQueryPriceReq,
        address_batch_init_task_data: &mut AddressBatchInitReq,
        subkeys: &mut Vec<wallet_tree::file_ops::BulkSubkey>,
        chain_list: &[String],
        seed: &[u8],
        account_index_map: &wallet_utils::address::AccountIndexMap,
        derivation_path: Option<&str>,
        uid: &str,
        wallet_address: &str,
        account_name: &str,
        is_default_name: bool,
    ) -> Result<(), crate::error::service::ServiceError> {
        for chain in chain_list.iter() {
            let code: ChainCode = chain.as_str().try_into()?;
            let address_types = WalletDomain::address_type_by_chain(code);
            let Ok(node) = Self::get_node_with_ctx(ctx, chain).await else {
                tracing::warn!("chain: {:?} node not found", chain);
                continue;
            };

            for address_type in address_types {
                let instance: wallet_chain_instance::instance::ChainObject = (
                    &code,
                    &address_type,
                    ChainDomain::network_kind_from_node_network(&node.network),
                )
                    .try_into()?;
                // (&code, &address_type, "mainnet".into()).try_into()?;

                let (account_address, derivation_path, address_init_req) =
                    AccountDomain::create_account_v2(
                        ctx,
                        seed,
                        &instance,
                        derivation_path,
                        account_index_map,
                        uid,
                        wallet_address,
                        account_name,
                        is_default_name,
                    )
                    .await?;

                if let Some(address_init_req) = address_init_req {
                    address_batch_init_task_data.0.push(address_init_req);
                }

                subkeys.push(
                    AccountDomain::generate_subkey(
                        &instance,
                        seed,
                        &account_address.address,
                        &code.to_string(),
                        account_index_map,
                        derivation_path.as_str(),
                    )
                    .await?,
                );
                AssetsDomain::init_default_assets(
                    ctx,
                    coins,
                    &account_address.address,
                    &code.to_string(),
                    req,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(crate) fn check_token_address(
        token_address: &mut String,
        chain_code: &str,
        net: NetworkKind,
    ) -> Result<(), crate::error::service::ServiceError> {
        let chain: wallet_types::chain::chain::ChainCode = chain_code.try_into()?;

        if is_evm_chain(chain) {
            *token_address = wallet_utils::address::to_checksum_address(token_address);
        }

        match chain {
            wallet_types::chain::chain::ChainCode::Sui => {
                wallet_utils::address::parse_sui_type_tag(token_address).map_err(|_| {
                    crate::error::business::BusinessError::Account(
                        crate::error::business::account::AccountError::AddressNotCorrect,
                    )
                })?;
            }
            _ => check_address(token_address, chain, net)?,
        }
        Ok(())
    }

    pub async fn init_load_default_chain_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let profile = crate::config::Config::active_feature_profile();
        let lists = crate::default_data::chain::default_chain_lists_for_feature(profile)?;

        for list in lists {
            for default_chain in list.chains.values() {
                let status = if default_chain.active { 1 } else { 0 };
                let req = wallet_database::entities::chain::ChainCreateVo::new(
                    &default_chain.name,
                    &default_chain.chain_code,
                    &default_chain.protocols,
                    NodeBindType::AutoLocal,
                    &default_chain.main_symbol,
                )
                .with_status(status);

                let result =
                    if crate::default_data::chain::is_dev_evm_chain(&default_chain.chain_code) {
                        ChainRepo::insert_missing(&pool, req).await.map(|_| ())
                    } else {
                        ChainRepo::add(&pool, req).await.map(|_| ())
                    };
                if let Err(e) = result {
                    tracing::error!(
                        "Init load default chain: failed to create default chain: {:?}",
                        e
                    );
                    continue;
                }
            }
        }

        Ok(())
    }

    pub async fn init_load_backend_chains_with_ctx(
        ctx: &'static Context,
        backend_chains: wallet_transport_backend::response_vo::chain::ChainList,
    ) -> Result<(), crate::error::service::ServiceError> {
        // let backend_chains = Self::load_backend_chain().await?;
        if backend_chains.list.is_empty() {
            tracing::debug!("No backend chain found in backend");
            return Ok(());
        }

        let pool = ctx.core_pool()?;
        let local_chains = ChainRepo::get_chain_list(&pool).await?;
        let backend_chain_map: HashMap<_, _> = backend_chains
            .list
            .iter()
            .filter(|o| o.master_token_code.is_some())
            .map(|chain| (chain.chain_code.clone(), chain))
            .collect();

        // let tx = pool.begin().await.map_err(|e|{
        //     crate::error::service::ServiceError::Database(wallet_database::Error::Database( DatabaseError))
        // });
        for local_chain in &local_chains {
            let bc_chain = backend_chain_map.get(&local_chain.chain_code);
            // && find_bc_chain.enable
            if let Some(_) = bc_chain {
                // 后端有则保留
                continue;
            } else {
                ChainRepo::delete(&pool, &local_chain.chain_code).await?;
            }
        }

        let mut bc_chain_vec = vec![];
        for bc_chain in &backend_chains.list {
            let Some(master_token_code) = &bc_chain.master_token_code else {
                continue;
            };
            bc_chain_vec.push(
                wallet_database::entities::chain::ChainCreateVo::new(
                    &bc_chain.name,
                    &bc_chain.chain_code,
                    &[],
                    NodeBindType::AutoBackend,
                    &master_token_code,
                )
                .with_status(if bc_chain.enable { 1 } else { 0 }),
            );
        }

        ChainRepo::upsert_multi_chain(&pool, bc_chain_vec).await?;

        Ok(())
    }

    pub async fn init_chain_info_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        Self::init_load_default_chain_with_ctx(ctx).await?;

        let app_version = ConfigDomain::get_app_version(ctx).await?;
        let chain_list_req = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::CHAIN_LIST,
            &wallet_transport_backend::request::ChainListReq::new(app_version.app_version),
        )?;
        Tasks::new().push(BackendApiTask::BackendApi(chain_list_req)).send_with_ctx(ctx).await?;
        Ok(())
    }
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub chain_code: String,
    pub node_id: String,
    pub node_name: String,
    pub rpc_url: String,
    pub ws_url: String,
    pub http_url: String,
    pub network: String,
    pub status: u8,
}

impl NodeInfo {
    pub fn new(
        chain_code: &str,
        node_id: &str,
        node_name: &str,
        rpc_url: &str,
        ws_url: &str,
        http_url: &str,
        network: &str,
        status: u8,
    ) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            node_id: node_id.to_string(),
            node_name: node_name.to_string(),
            rpc_url: rpc_url.to_string(),
            ws_url: ws_url.to_string(),
            http_url: http_url.to_string(),
            network: network.to_string(),
            status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChainDomain, has_missing_chain_account};
    use wallet_database::entities::account::{AccountEntity, AccountWalletMapping};
    use wallet_types::chain::{chain::ChainCode, network::NetworkKind};

    fn mapping(wallet_address: &str, account_id: u32) -> AccountWalletMapping {
        AccountWalletMapping {
            account_id,
            account_name: format!("account-{account_id}"),
            wallet_address: wallet_address.to_string(),
            uid: format!("uid-{wallet_address}"),
        }
    }

    fn account(wallet_address: &str, account_id: u32, chain_code: &str) -> AccountEntity {
        let mut account = AccountEntity::default();
        account.wallet_address = wallet_address.to_string();
        account.account_id = account_id;
        account.chain_code = chain_code.to_string();
        account
    }

    #[test]
    fn enabled_chain_is_missing_when_any_wallet_account_lacks_it() {
        let mappings = vec![mapping("wallet-a", 1), mapping("wallet-b", 1)];
        let accounts = vec![account("wallet-a", 1, "eth"), account("wallet-b", 1, "tron")];

        assert!(has_missing_chain_account(&mappings, &accounts, "eth"));
    }

    #[test]
    fn enabled_chain_is_complete_when_every_wallet_account_has_it() {
        let mappings = vec![mapping("wallet-a", 1), mapping("wallet-b", 1)];
        let accounts = vec![account("wallet-a", 1, "eth"), account("wallet-b", 1, "eth")];

        assert!(!has_missing_chain_account(&mappings, &accounts, "eth"));
    }

    #[test]
    fn check_token_address_normalizes_added_evm_chains() {
        let raw = "0x52908400098527886e0f7030069857d2e4169ee7";
        let expected = wallet_utils::address::to_checksum_address(raw);

        for chain in [ChainCode::Polygon, ChainCode::Base] {
            let mut token_address = raw.to_string();
            ChainDomain::check_token_address(
                &mut token_address,
                &chain.to_string(),
                NetworkKind::Mainnet,
            )
            .unwrap();

            assert_eq!(token_address, expected);
        }
    }
}
