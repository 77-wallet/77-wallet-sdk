use wallet_database::{
    entities::{api_chain::NodeBindType, api_coin::ApiCoinEntity, api_wallet::ApiWalletType},
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, chain::ApiChainRepo, coin::ApiCoinRepo, wallet::ApiWalletRepo,
        },
        node::NodeRepo,
    },
};
use wallet_transport_backend::request::{
    ChainRpcListReq, TokenQueryPriceReq, api_wallet::address::ApiAddressInitReq,
};
use wallet_types::chain::chain::ChainCode;

use crate::{
    context::Context,
    domain::{
        api_wallet::{account::ApiAccountDomain, wallet::ApiWalletDomain},
        app::config::ConfigDomain,
        chain::{ChainDomain, NodeInfo},
        wallet::WalletDomain,
    },
    infrastructure::{
        chain_node::chain_node_ensurer::ChainNodeEnsurer,
        task_queue::{
            CommonTask,
            backend::{BackendApiTask, BackendApiTaskData},
            task::Tasks,
        },
    },
};

#[path = "chain/account_sync.rs"]
mod account_sync;
#[path = "chain/recovery.rs"]
pub(crate) mod recovery;

pub struct ApiChainDomain {}

#[cfg(test)]
#[path = "chain/config_tests.rs"]
mod config_tests;

pub(crate) async fn register_missing_chain_addresses(
    ctx: &'static Context,
    req: ApiAddressInitReq,
) -> Result<(), crate::error::service::ServiceError> {
    account_sync::register_bounded(ctx, req).await
}

impl ApiChainDomain {
    pub(crate) async fn start_missing_chain_recovery(ctx: &'static Context) {
        if let Err(error) = crate::domain::api_wallet::chain::recovery::schedule_chains(ctx).await {
            let (reason, backend_code) = recovery::log_reason(&error);
            tracing::warn!(reason,backend_code=?backend_code,"A reconcile scheduling deferred");
        }
    }
    pub(crate) async fn run_queued_chain_reconcile(
        ctx: &'static Context,
        chain: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        account_sync::run_chain(ctx, chain).await
    }

    pub(crate) async fn network_kind_by_chain_code(
        chain_code: &str,
        ctx: &'static Context,
    ) -> Result<wallet_types::chain::network::NetworkKind, crate::error::service::ServiceError>
    {
        let node = Self::get_node(chain_code, ctx).await?;
        Ok(ChainDomain::network_kind_from_node_network(&node.network))
    }

    pub(crate) async fn init_chains_api_assets(
        ctx: &'static Context,
        _coins: &[ApiCoinEntity],
        _req: &mut TokenQueryPriceReq,
        api_address_init_req: &mut ApiAddressInitReq,
        chain_list: &[String],
        seed: &[u8],
        account_index_map: &wallet_utils::address::AccountIndexMap,
        uid: &str,
        wallet_address: &str,
        account_name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
        is_recover: bool,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        tracing::debug!(account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, chains_count=chain_list.len(), "ApiChainDomain: starting init_chains_api_assets");
        let pool = ctx.api_wallet_pool()?;
        let mut created_addresses = Vec::new();

        for chain in chain_list.iter() {
            let code: ChainCode = chain.as_str().try_into()?;
            let address_types = WalletDomain::address_type_by_chain(code);

            let Ok(node) = Self::get_node(chain, ctx).await else {
                continue;
            };

            if let Some(account) = ApiAccountRepo::find_one_by_wallet_address_account_id_chain_code(
                &pool,
                &wallet_address,
                account_index_map.account_id,
                chain,
            )
            .await?
            {
                if account.is_init == 1 {
                    continue;
                }
            }

            for address_type in address_types {
                let instance: wallet_chain_instance::instance::ChainObject = (
                    &code,
                    &address_type,
                    crate::domain::chain::ChainDomain::network_kind_from_node_network(
                        &node.network,
                    ),
                )
                    .try_into()?;
                let (account_address, address_init_req) = ApiAccountDomain::derive_subkey_with_ctx(
                    ctx,
                    uid,
                    seed,
                    wallet_address,
                    account_index_map,
                    &instance,
                    account_name,
                    is_default_name,
                    api_wallet_type,
                    is_recover,
                )
                .await?;

                if let Some(address_init_req) = address_init_req {
                    api_address_init_req.address_list.add_address(address_init_req);
                }

                created_addresses.push(account_address);
            }
        }

        tracing::debug!(account_id=%account_index_map.account_id, input_index=%account_index_map.input_index, created_addresses_count=%created_addresses.len(), "ApiChainDomain: completed init_chains_api_assets");
        Ok(created_addresses)
    }

    pub(crate) async fn upsert_multi_api_chain_than_toggle(
        ctx: &'static Context,
        chains: wallet_transport_backend::response_vo::api_wallet::chain::ApiChainListResp,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        // tracing::warn!("upsert_multi_chain_than_toggle, chains: {:#?}", chains);
        let pool = ctx.api_wallet_pool()?;
        // let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        // // 本地后端节点
        // let local_backend_nodes =
        //     wallet_database::repositories::node::NodeRepoTrait::list(&mut repo, Some(0)).await?;

        // // 本地配置节点
        // let default_nodes =
        //     wallet_database::repositories::node::NodeRepoTrait::list(&mut repo, Some(1)).await?;

        let mut input = Vec::new();
        let mut chain_codes = Vec::new();
        let mut backend_chain_codes = Vec::new();
        // let mut has_new_chain = false;
        let mut new_chains = Vec::new();
        let app_version = ConfigDomain::get_app_version(ctx).await?.app_version;
        for chain in chains.0 {
            backend_chain_codes.push(chain.chain_code.clone());
            let Some(master_token_code) = chain.master_token_code else {
                // Persist the backend's unusable configuration even before defaults exist.
                // Otherwise a later dev initialization can silently enable this chain.
                input.push(
                    wallet_database::entities::api_chain::ApiChainCreateVo::new(
                        &chain.name,
                        &chain.chain_code,
                        &[],
                        NodeBindType::AutoBackend,
                        "",
                    )
                    .with_status(0),
                );
                continue;
            };

            let status = match (
                ConfigDomain::compare_versions(&app_version, &chain.app_version_code),
                chain.enable,
            ) {
                (std::cmp::Ordering::Less, _) => 0,
                (_, true) => 1,
                (_, false) => 0,
            };

            let has_accounts =
                ApiAccountRepo::exists_by_chain_code(&pool, &chain.chain_code).await?;
            if !has_accounts && chain.enable {
                // has_new_chain = true;
                new_chains.push(chain.chain_code.clone());
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
                wallet_database::entities::api_chain::ApiChainCreateVo::new(
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

        ApiChainRepo::upsert_multi_chain(&pool, input).await?;
        let enabled_chain_codes = crate::default_data::chain::enabled_chain_codes_for_feature(
            crate::config::Config::active_feature_profile(),
            &chain_codes,
            &backend_chain_codes,
        )?;
        Self::toggle_api_chains(ctx, &enabled_chain_codes).await?;
        let ensurer = ChainNodeEnsurer::new(ctx.core_pool()?, pool.clone());
        ensurer.ensure_all().await?;
        super::coin::ApiCoinDomain::sync_dev_native_coins(ctx).await?;

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

        Ok(new_chains)
    }

    pub async fn toggle_api_chains(
        ctx: &'static Context,
        chain_codes: &[String],
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        ApiChainRepo::toggle_chains_status(&pool, chain_codes).await?;
        Ok(())
    }

    pub async fn init_api_chain_info(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let profile = crate::config::Config::active_feature_profile();
        let lists = crate::default_data::chain::default_chain_lists_for_feature(profile)?;

        // tracing::warn!("list {:#?}", list);

        let mut chain_codes = Vec::new();
        for list in lists {
            for (chain_code, default_chain) in &list.chains {
                let status = if default_chain.active { 1 } else { 0 };
                // let node_id =
                //     NodeDomain::gen_node_id(&default_chain.node_name, &default_chain.chain_code);
                let req = wallet_database::entities::api_chain::ApiChainCreateVo::new(
                    &default_chain.name,
                    &default_chain.chain_code,
                    &default_chain.protocols,
                    NodeBindType::AutoLocal,
                    &default_chain.main_symbol,
                )
                .with_status(status);

                let result = ApiChainRepo::insert_missing(&pool, req).await;
                if let Err(e) = result {
                    tracing::error!("Failed to create default chain: {:?}", e);
                    continue;
                }
                if status == 1 {
                    #[cfg(all(feature = "dev", not(feature = "prod")))]
                    if super::dev_defaults::contains(chain_code) {
                        continue;
                    }
                    chain_codes.push(chain_code.to_string());
                }
            }
        }
        let app_version = ConfigDomain::get_app_version(ctx).await?;

        #[cfg(all(feature = "dev", not(feature = "prod")))]
        ApiChainRepo::toggle_chains_status_except(
            &pool,
            &chain_codes,
            &super::dev_defaults::chain_codes(),
        )
        .await?;
        #[cfg(not(all(feature = "dev", not(feature = "prod"))))]
        ApiChainRepo::toggle_chains_status(&pool, &chain_codes).await?;
        ChainNodeEnsurer::new(ctx.core_pool()?, pool.clone()).ensure_all().await?;
        super::coin::ApiCoinDomain::sync_dev_native_coins(ctx).await?;
        let chain_list_req = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::api_wallet::API_WALLET_CHAIN_LIST,
            &wallet_transport_backend::request::ChainListReq::new(app_version.app_version),
        )?;
        Tasks::new().push(BackendApiTask::BackendApi(chain_list_req)).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub async fn sync_withdrawal_wallet_chain_data(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;

        let chain_list: Vec<String> = ApiChainRepo::get_chain_list(&pool)
            .await?
            .into_iter()
            .map(|chain| chain.chain_code)
            .collect();

        let withdrawal_wallet_list =
            ApiWalletRepo::list(&pool, Some(ApiWalletType::Withdrawal)).await?;

        for wallet in withdrawal_wallet_list {
            ApiAccountDomain::create_withdrawal_account_with_ctx(
                ctx,
                &wallet.address,
                chain_list.clone(),
                "账户",
                true,
                false,
            )
            .await?;
        }

        Ok(())
    }

    pub(crate) async fn get_node(
        chain_code: &str,
        ctx: &'static Context,
    ) -> Result<NodeInfo, crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let api_pool = ctx.api_wallet_pool()?;
        let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
        let node_id = ensurer.ensure_and_get_api_chain_node(chain_code).await?;

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

    pub async fn sync_chains(
        ctx: &'static Context,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        let app_version = ConfigDomain::get_app_version(ctx).await?;
        let chain_list = backend.api_wallet_chain_list(&app_version.app_version).await?;
        ApiChainDomain::upsert_multi_api_chain_than_toggle(ctx, chain_list).await
    }

    /// A partial chain rollout remains pending until every logical account is registered.
    pub(crate) async fn pending_chain_codes(
        ctx: &'static Context,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let through = ApiAccountRepo::account_high_watermark(&pool).await?;
        let mut pending = Vec::new();
        for chain in ApiChainRepo::get_chain_list(&pool).await? {
            if chain.node_id.is_none() {
                continue;
            }
            let code: ChainCode = chain.chain_code.as_str().try_into()?;
            let expected = WalletDomain::address_type_by_chain(code).len() as u32;
            if !ApiAccountRepo::missing_chain_account_page_for_variants(
                &pool,
                &chain.chain_code,
                0,
                through,
                1,
                expected,
            )
            .await?
            .is_empty()
            {
                pending.push(chain.chain_code);
            }
        }
        Ok(pending)
    }

    pub(crate) async fn reconcile_accounts_if_unlocked(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        if Self::pending_chain_codes(ctx).await?.is_empty() {
            return Ok(());
        }
        let Some(session) = ctx.wallet_unlock_session_snapshot().await else {
            tracing::info!("API chain expansion deferred until wallet is unlocked");
            return Ok(());
        };
        if session.is_expired() {
            return Ok(());
        }
        crate::domain::api_wallet::chain::recovery::schedule_chains(ctx).await
    }

    #[cfg(test)]
    fn retain_active_account_mappings(
        accounts: &[wallet_database::entities::api_account::ApiAccountEntity],
        mappings: &mut Vec<wallet_database::entities::api_account::ApiAccountWalletMapping>,
    ) {
        let active: std::collections::HashSet<_> = accounts
            .iter()
            .filter(|row| row.status == 1)
            .map(|row| (row.wallet_address.as_str(), row.account_id, row.api_wallet_type as u8))
            .collect();
        let mut seen = std::collections::HashSet::new();
        mappings.retain(|row| {
            active.contains(&(
                row.wallet_address.as_str(),
                row.account_id,
                row.api_wallet_type as u8,
            )) && seen.insert((
                row.wallet_address.clone(),
                row.account_id,
                row.api_wallet_type as u8,
            ))
        });
    }

    pub async fn sync_wallet_chain_data(
        ctx: &'static Context,
        wallet_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        crate::domain::wallet::WalletDomain::validate_password_with_context(ctx, wallet_password)
            .await?;
        crate::domain::api_wallet::chain::recovery::schedule_chains(ctx).await
    }
}

pub struct ApiChainTransDomain;

impl ApiChainTransDomain {
    pub async fn main_coin(
        ctx: &'static Context,
        chain_code: &str,
    ) -> Result<ApiCoinEntity, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let coin = ApiCoinRepo::main_coin(chain_code, &pool).await?;
        Ok(coin)
    }
}
