use crate::domain;
use std::sync::Arc;
use tokio::sync::Semaphore;
use wallet_database::{
    dao::assets::CreateAssetsVo,
    entities::{
        account::AccountEntity,
        assets::{AssetsEntity, AssetsId},
        coin::{CoinEntity, CoinMultisigStatus},
    },
    repositories::{account::AccountRepoTrait, assets::AssetsRepoTrait, ResourcesRepo},
};
use wallet_transport_backend::request::TokenQueryPriceReq;

pub struct AssetsDomain {}

impl Default for AssetsDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetsDomain {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn upsert_assets(
        &mut self,
        repo: &mut ResourcesRepo,
        mut req: TokenQueryPriceReq,
        address: &str,
        chain_code: &str,
        symbol: &str,
        decimals: u8,
        token_address: Option<String>,
        protocol: Option<String>,
        is_multisig: i32,
        name: &str,
        price: &str,
    ) -> Result<(), crate::ServiceError> {
        let tx = repo;
        let assets_id =
            wallet_database::entities::assets::AssetsId::new(address, chain_code, symbol);

        let assets = wallet_database::dao::assets::CreateAssetsVo::new(
            assets_id,
            decimals,
            token_address.clone(),
            protocol.clone(),
            is_multisig,
        )
        .with_name(name)
        .with_u256(alloy::primitives::U256::default(), decimals)?;

        if price.is_empty() {
            req.insert(
                chain_code,
                &assets.token_address.clone().unwrap_or_default(),
            );
            let task =
                super::task_queue::Task::Common(super::task_queue::CommonTask::QueryCoinPrice(req));
            super::task_queue::Tasks::new().push(task).send().await?;
        }
        tx.upsert_assets(assets).await?;
        Ok(())
    }

    pub async fn get_account_assets_entity(
        &mut self,
        repo: &mut ResourcesRepo,
        account_id: u32,
        wallet_address: &str,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::ServiceError> {
        let tx = repo;
        let accounts = tx
            .get_account_list_by_wallet_address_and_account_id(
                Some(wallet_address),
                Some(account_id),
            )
            .await?;
        let addresses = accounts.into_iter().map(|info| info.address).collect();
        let data = tx.get_coin_assets_in_address(addresses).await?;
        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                return Ok(data
                    .into_iter()
                    .filter(|val| val.is_multisig == 2)
                    .collect());
            } else {
                return Ok(data
                    .into_iter()
                    .filter(|val| val.is_multisig == 0 || val.is_multisig == 1)
                    .collect());
            }
        }
        Ok(data)
    }

    pub async fn get_local_coin_list(
        &self,
        repo: &mut ResourcesRepo,
        addresses: Vec<String>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<crate::response_vo::coin::CoinInfoList, crate::ServiceError> {
        let tx = repo;

        let _is_multisig = if let Some(is_multisig) = is_multisig
            && !is_multisig
        {
            None
        } else {
            is_multisig
        };

        let coin_list = tx
            .lists(addresses, chain_code, keyword, _is_multisig)
            .await
            .map_err(|e| crate::ServiceError::System(crate::SystemError::Database(e)))?;
        let mut res = crate::response_vo::coin::CoinInfoList::default();
        for coin in coin_list {
            if let Some(info) = res.iter_mut().find(|info| info.symbol == coin.symbol) {
                info.chain_list.insert(crate::response_vo::coin::ChainInfo {
                    chain_code: coin.chain_code,
                    token_address: Some(coin.token_address),
                    protocol: coin.protocol,
                });
            } else {
                res.push(crate::response_vo::coin::CoinInfo {
                    symbol: coin.symbol,
                    name: Some(coin.name),
                    chain_list: std::collections::HashSet::from([
                        crate::response_vo::coin::ChainInfo {
                            chain_code: coin.chain_code,
                            token_address: Some(coin.token_address),
                            protocol: coin.protocol,
                        },
                    ]),
                    is_multichain: false,
                });
            }
        }

        Ok(res)
    }

    pub async fn sync_assets_by_address(
        addr: Vec<String>,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        // 1. 查询所有的资产
        let mut assets =
            AssetsEntity::all_assets(pool.as_ref(), addr, chain_code, None, None).await?;
        if !symbol.is_empty() {
            assets.retain(|asset| symbol.contains(&asset.symbol));
        }

        // 2. 同步资产余额
        domain::assets::AssetsDomain::sync_address_balance(assets).await
    }

    // 根据钱包地址来同步资产余额
    pub async fn sync_assets_by_wallet(
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let list =
            AccountEntity::lists_by_wallet_address(&wallet_address, account_id, pool.as_ref())
                .await?;

        // 获取地址
        let addr = list
            .iter()
            .map(|a| a.address.clone())
            .collect::<Vec<String>>();

        let mut assets = AssetsEntity::all_assets(pool.as_ref(), addr, None, None, None).await?;

        if !symbol.is_empty() {
            assets.retain(|asset| symbol.contains(&asset.symbol));
        }

        // 2. 同步资产余额
        domain::assets::AssetsDomain::sync_address_balance(assets).await
    }

    pub async fn sync_address_balance(
        assets: Vec<AssetsEntity>,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let semaphore = Arc::new(Semaphore::new(10));
        let tasks = assets.iter().map(|coin| {
            let semaphore = semaphore.clone();

            async move {
                // 获取信号量的许可，限制并发数量
                let _permit = semaphore.acquire().await.unwrap();

                let adapter = domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(
                    &coin.chain_code,
                )
                .await;
                let adapter = match adapter {
                    Ok(adapter) => adapter,
                    Err(e) => {
                        tracing::error!("获取链详情出错: {}，链代码: {}", e, coin.chain_code);
                        return None;
                    }
                };

                let balance = match adapter.balance(&coin.address, coin.token_address()).await {
                    Ok(balance) => balance,
                    Err(e) => {
                        tracing::error!(
                            "获取余额出错: 地址={}, 链代码={}, 符号={}, token = {:?},错误:{}",
                            coin.address,
                            coin.chain_code,
                            coin.symbol,
                            coin.token_address(),
                            e
                        );
                        return None;
                    }
                };
                let balance = wallet_utils::unit::format_to_string(balance, coin.decimals)
                    .unwrap_or_else(|_| "0".to_string());

                // 准备更新数据
                let assets_id = AssetsId {
                    address: coin.address.to_string(),
                    chain_code: coin.chain_code.to_string(),
                    symbol: coin.symbol.to_string(),
                };

                Some((assets_id, balance))
            }
        });

        // 并发执行所有任务，并收集结果
        let results: Vec<(AssetsId, String)> = futures::future::join_all(tasks)
            .await
            .into_iter()
            .flatten()
            .collect();

        // 在单个事务中批量更新数据库
        let mut tx = pool.begin().await.expect("开启事务失败");
        for (assets_id, balance) in results {
            if let Err(e) = AssetsEntity::update_balance(tx.as_mut(), &assets_id, &balance).await {
                tracing::error!("更新余额出错: {}", e);
            }
        }
        // 提交事务
        if let Err(e) = tx.commit().await {
            tracing::error!("提交事务失败: {}", e);
        }
        Ok(())
    }

    // 根据地址和链初始化多签账号里面的资产
    // address :multisig account address ,
    pub async fn init_default_multisig_assets(
        address: String,
        chain_code: String,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;
        let default_coins =
            CoinEntity::list(&*pool, vec![], Some(chain_code.clone()), Some(1)).await?;
        let mut symbols = Vec::new();
        for coin in default_coins {
            let assets_id = AssetsId::new(&address, &chain_code, &coin.symbol);
            let assets = CreateAssetsVo::new(
                assets_id,
                coin.decimals,
                coin.token_address.clone(),
                coin.protocol.clone(),
                CoinMultisigStatus::IsMultisig.to_i8() as i32,
            )
            .with_name(&coin.name)
            .with_u256(alloy::primitives::U256::default(), coin.decimals)?;

            AssetsEntity::upsert_assets(&*pool, assets).await?;
            symbols.push(coin.symbol);
        }

        // 同步资产余额
        AssetsDomain::sync_assets_by_address(vec![address], Some(chain_code), symbols).await?;
        Ok(())
    }
}
