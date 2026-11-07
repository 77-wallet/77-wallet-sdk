use std::collections::HashMap;

use crate::{
    domain::{self, app::config::ConfigDomain, chain::ChainDomain, coin::CoinDomain},
    infrastructure::{
        chain_node::chain_node_ensurer::ChainNodeEnsurer,
        task_queue::{
            CommonTask,
            backend::{BackendApiTask, BackendApiTaskData},
            task::Tasks,
        },
    },
    response_vo::standard_wallet::chain::ChainAssets,
};
use wallet_database::{
    entities::{
        api_chain::NodeBindType,
        chain::{ChainCreateVo, ChainEntity, ChainWithNode},
    },
    repositories::{
        account::AccountRepo, api_wallet::chain::ApiChainRepo, assets::AssetsRepo,
        chain::ChainRepo, coin::CoinRepo,
    },
};
use wallet_transport_backend::request::{AddressBatchInitReq, TokenQueryPriceReq};

pub struct ChainService {
    ctx: &'static crate::context::Context,
}

#[cfg(test)]
#[path = "chain/evm_token_identity_tests.rs"]
mod ordinary_evm_token_identity_tests;

impl ChainService {
    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }

    pub async fn add(
        self,
        name: &str,
        chain_code: &str,
        protocols: &[String],
        main_symbol: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let input =
            ChainCreateVo::new(name, chain_code, protocols, NodeBindType::AutoLocal, main_symbol);
        let _res = ChainRepo::add(&core_pool, input).await?;

        Ok(())
    }

    pub async fn set_chain_node(
        self,
        chain_code: &str,
        node_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let api_pool = self.ctx.api_wallet_pool()?;
        ChainRepo::set_chain_node_with_type(
            &core_pool,
            chain_code,
            node_id,
            NodeBindType::ManualUser,
        )
        .await?;
        ApiChainRepo::set_chain_node_with_type(
            &api_pool,
            chain_code,
            node_id,
            NodeBindType::ManualUser,
        )
        .await?;
        let ensurer = ChainNodeEnsurer::new(core_pool.clone(), api_pool.clone());
        ensurer.after_user_select(chain_code).await?;
        Ok(())
    }

    pub async fn sync_chains(self) -> Result<bool, crate::error::service::ServiceError> {
        let backend = self.ctx.get_global_backend_api();

        let app_version = ConfigDomain::get_app_version(self.ctx).await?;

        let req = wallet_transport_backend::request::ChainListReq::new(app_version.app_version);
        let chain_list = backend.chain_list(req).await?;
        ChainDomain::upsert_multi_chain_than_toggle_with_ctx(self.ctx, chain_list).await
    }

    pub async fn sync_wallet_chain_data(
        self,
        wallet_password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;

        domain::wallet::WalletDomain::validate_password_with_context(self.ctx, wallet_password)
            .await?;
        let chain_list: Vec<String> = ChainRepo::get_chain_node_list(&core_pool)
            .await?
            .into_iter()
            .map(|chain| chain.chain_code)
            .collect();

        tracing::info!("sync_wallet_chain_data: start");
        let account_wallet_mapping = AccountRepo::account_wallet_mapping(core_pool.clone()).await?;
        let mut req = TokenQueryPriceReq(Vec::new());
        tracing::info!("sync_wallet_chain_data: start ---------------- 1");
        let coins = CoinRepo::default_coin_list(&core_pool).await?;

        let mut address_batch_init_task_data = AddressBatchInitReq(Vec::new());
        for wallet in account_wallet_mapping {
            let mut subkeys = Vec::<wallet_tree::file_ops::BulkSubkey>::new();
            let account_index_map =
                wallet_utils::address::AccountIndexMap::from_account_id(wallet.account_id)?;

            let seed = domain::wallet::WalletDomain::get_seed(
                self.ctx,
                &wallet.wallet_address,
                wallet_password,
            )
            .await?;

            tracing::info!("sync_wallet_chain_data: init_chains_assets");
            ChainDomain::init_chains_assets(
                self.ctx,
                &coins,
                &mut req,
                &mut address_batch_init_task_data,
                &mut subkeys,
                &chain_list,
                &seed,
                &account_index_map,
                None,
                &wallet.uid,
                &wallet.wallet_address,
                &wallet.account_name,
                false,
            )
            .await?;

            crate::application::wallet::WalletApplication::store_subkeys(
                self.ctx,
                &wallet.wallet_address,
                subkeys,
                wallet_password,
            )
            .await?;
        }

        // let device_bind_address_task_data =
        //     DeviceDomain::gen_device_bind_address_task_data().await?;

        let address_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::ADDRESS_BATCH_INIT,
            &address_batch_init_task_data,
        )?;
        Tasks::new()
            .push(CommonTask::QueryCoinPrice(req))
            .push(BackendApiTask::BackendApi(address_init_task_data))
            .send_with_ctx(self.ctx)
            .await?;

        Ok(())
    }

    pub async fn get_hot_chain_list(
        self,
    ) -> Result<Vec<ChainEntity>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(ChainRepo::get_chain_list_v2(&core_pool).await?)
    }

    pub async fn get_market_chain_list(
        self,
    ) -> Result<Vec<String>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(CoinRepo::get_market_chain_list(&core_pool).await?)
    }

    pub async fn get_chain_list_with_node_info(
        self,
    ) -> Result<Vec<ChainWithNode>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(ChainRepo::get_chain_node_list(&core_pool).await?)
    }

    pub async fn get_protocol_list(
        self,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        Ok(ChainRepo::detail(&core_pool, chain_code).await?)
    }

    pub async fn get_chain_assets_list(
        self,
        address: &str,
        account_id: Option<u32>,
        // symbol: &str,
        chain_list: HashMap<String, String>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError> {
        let pool = self.ctx.core_pool()?;
        let token_currencies = CoinDomain::get_token_currencies_v2_with_ctx(self.ctx).await?;

        let mut account_addresses = Vec::<String>::new();

        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                // 查询多签账户下的资产
                let account = domain::multisig::MultisigDomain::account_by_address(
                    address,
                    true,
                    &pool.into_inner(),
                )
                .await?;
                account_addresses.push(account.address);
            } else {
                // 获取钱包下的这个账户的所有地址
                let accounts = AccountRepo::get_account_list_by_wallet_address_and_account_id(
                    pool.clone(),
                    Some(address),
                    account_id,
                )
                .await?;
                for account in accounts {
                    if !account_addresses.iter().any(|address| address == &account.address) {
                        account_addresses.push(account.address);
                    }
                }
            }
        } else {
            // 获取钱包下的这个账户的所有地址
            let accounts = AccountRepo::get_account_list_by_wallet_address_and_account_id(
                pool.clone(),
                Some(address),
                account_id,
            )
            .await?;
            for account in accounts {
                if !account_addresses.iter().any(|address| address == &account.address) {
                    account_addresses.push(account.address);
                }
            }
        }
        let datas =
            AssetsRepo::get_assets_by_address(&pool, account_addresses, is_multisig).await?;

        let datas = datas
            .into_iter()
            .filter(|data| {
                chain_list.get(&data.chain_code).is_some_and(|token_address| {
                    data.token_key().matches_for_chain(
                        &wallet_database::entities::asset_token_key::AssetTokenKey::from_raw(Some(
                            token_address,
                        )),
                        &data.chain_code,
                    )
                })
            })
            .collect();

        let chains = ChainRepo::get_chain_list(&pool).await?;
        let res = token_currencies.calculate_chain_assets_list(datas, chains, self.ctx).await?;

        Ok(res)
    }
}
