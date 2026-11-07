#[cfg(test)]
mod coin_list_name_tests;
pub(crate) mod refresh_order;
use super::chain::adapter::ChainAdapterFactory;
use crate::{
    context::Context,
    domain::coin::CoinDomain,
    error::service::ServiceError,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
    request::transaction::SwapTokenInfo,
    response_vo::standard_wallet::{chain::ChainList, coin::CoinInfoList},
};
use futures::{StreamExt, stream};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Semaphore;
use wallet_database::{
    CoreDbPool,
    dao::assets::CreateAssetsVo,
    entities::{
        api_assets::ApiAssetsEntity,
        asset_token_key::AssetTokenKey,
        assets::{AssetsEntity, AssetsId},
        coin::{CoinEntity, CoinMultisigStatus},
    },
    repositories::{account::AccountRepo, assets::AssetsRepo, coin::CoinRepo, wallet::WalletRepo},
};
use wallet_transport_backend::request::TokenQueryPriceReq;

pub struct AssetsDomain;

enum SyncFilter {
    Symbol(Vec<String>),
    Token(AssetTokenKey),
}

fn filter_assets_for_sync(
    assets: Vec<AssetsEntity>,
    token_address: &AssetTokenKey,
) -> (Vec<AssetsEntity>, Vec<String>) {
    let mut matched = Vec::new();
    let mut filtered_out = Vec::new();

    for asset in assets {
        if asset.token_key().matches_for_chain(token_address, &asset.chain_code) {
            matched.push(asset);
        } else {
            filtered_out
                .push(format!("{}/{}/{}", asset.symbol, asset.address, asset.token_address));
        }
    }

    (matched, filtered_out)
}

fn select_assets_for_sync(
    assets: Vec<AssetsEntity>,
    filter: &SyncFilter,
) -> (Vec<AssetsEntity>, Vec<String>) {
    match filter {
        SyncFilter::Token(token_address) => filter_assets_for_sync(assets, token_address),
        SyncFilter::Symbol(symbol) => {
            if symbol.is_empty() {
                return (assets, Vec::new());
            }

            let mut matched = Vec::new();
            let mut filtered_out = Vec::new();
            for asset in assets {
                if symbol.contains(&asset.symbol) {
                    matched.push(asset);
                } else {
                    filtered_out.push(format!(
                        "{}/{}/{}",
                        asset.symbol, asset.address, asset.token_address
                    ));
                }
            }
            (matched, filtered_out)
        }
    }
}

impl Default for AssetsDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetsDomain {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn get_account_assets_entity(
        &mut self,
        core_pool: &CoreDbPool,
        account_id: u32,
        wallet_address: &str,
        chain_codes: Vec<String>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, ServiceError> {
        let accounts = AccountRepo::account_list_by_wallet_address_and_account_id_and_chain_codes(
            core_pool.clone(),
            Some(wallet_address),
            Some(account_id),
            chain_codes,
        )
        .await?;
        let addresses = accounts.into_iter().map(|info| info.address).collect();
        let data = AssetsRepo::get_coin_assets_in_address(core_pool, addresses, Some(1)).await?;
        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                return Ok(data.into_iter().filter(|val| val.is_multisig == 2).collect());
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
        ctx: &'static Context,
        core_pool: &CoreDbPool,
        addresses: Vec<String>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<crate::response_vo::standard_wallet::coin::CoinInfoList, ServiceError> {
        let _is_multisig = if let Some(is_multisig) = is_multisig
            && !is_multisig
        {
            None
        } else {
            is_multisig
        };

        let assets_list =
            AssetsRepo::all_assets(core_pool, addresses, chain_code, keyword, _is_multisig).await?;

        let show_contract = keyword.is_some();
        let mut res = crate::response_vo::standard_wallet::coin::CoinInfoList::default();
        for assets in assets_list {
            let coin = CoinDomain::get_coin_by_token_key_with_ctx(
                ctx,
                &assets.chain_code,
                assets.token_key(),
            )
            .await?;
            if let Some(info) =
                res.iter_mut().find(|info| info.symbol == assets.symbol && coin.is_default == 1)
            {
                info.chain_list
                    .entry(assets.chain_code.clone())
                    .or_insert(assets.token_address.as_db_str().to_string());
            } else {
                res.push(crate::response_vo::standard_wallet::coin::CoinInfo {
                    symbol: assets.symbol,
                    name: Some(coin.name),

                    chain_list: ChainList(HashMap::from([(
                        assets.chain_code.clone(),
                        assets.token_address.as_db_str().to_string(),
                    )])),
                    is_default: coin.is_default == 1,
                    hot_coin: coin.status == 1,
                    show_contract,
                });
            }
        }

        Ok(res)
    }

    // keyword 存在都要展示合约地址
    // 链相同，symbol相同 大于2 显示地址
    pub async fn show_contract(
        pool: &CoreDbPool,
        keyword: Option<&str>,
        res: &mut CoinInfoList,
    ) -> Result<(), ServiceError> {
        let has_keyword = keyword.is_some();

        for coin in res.iter_mut() {
            let chain_len = coin.chain_list.len();

            if has_keyword || coin.is_default {
                // 有 keyword：只有恰好 1 条链才显示
                coin.show_contract = chain_len == 1;
                continue;
            }

            // 无 keyword 的逻辑
            match chain_len {
                1 => {
                    let chain_code =
                        coin.chain_list.keys().next().expect("len()==1 已保证存在 key");

                    let same_coin_num =
                        CoinRepo::same_coin_num(pool, &coin.symbol, chain_code).await?;

                    coin.show_contract = same_coin_num > 1;
                }
                _ => {
                    // 0 或 >1 条链都不显示
                    coin.show_contract = false;
                }
            }
        }

        Ok(())
    }

    // 根据钱包地址来同步资产余额( 目前不需要在进行使用 )
    pub async fn sync_assets_by_wallet(
        ctx: &'static Context,
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;

        let list =
            AccountRepo::lists_by_wallet_address(pool.clone(), &wallet_address, account_id, None)
                .await?;

        // 获取地址
        let addr = list.iter().map(|a| a.address.clone()).collect::<Vec<String>>();

        tracing::debug!(
            "按 symbol 兼容接口同步钱包资产: wallet_address={}, account_id={:?}, symbol={:?}",
            wallet_address,
            account_id,
            symbol
        );

        Self::do_async_balance(ctx, pool, addr, None, SyncFilter::Symbol(symbol)).await
    }

    pub(crate) async fn request_identity_refresh(
        ctx: &'static Context,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;
        let pending =
            AssetsRepo::pending_identity_refreshes(&pool, wallet_address, account_id, chain_code)
                .await?;
        if pending.is_empty() {
            return Ok(());
        }
        let Some(handles) = ctx.get_global_handles().await.upgrade() else {
            // No running event loop (e.g. an isolated test Context). Preserve
            // markers so a later real account query can retry the same work.
            return Ok(());
        };
        let events = handles.get_global_inner_event_handle();
        let mut dispatched = Vec::new();
        for asset in pending {
            let data = crate::infrastructure::inner_event::SyncAssetsData::new_with_token_key(
                vec![asset.address.clone()],
                asset.chain_code.clone(),
                asset.token_key(),
            );
            events.send(crate::infrastructure::inner_event::InnerEvent::SyncAssets(data))?;
            dispatched.push(asset.get_assets_id());
        }
        AssetsRepo::mark_identity_refresh_dispatched(&pool, &dispatched).await?;
        Ok(())
    }

    pub async fn sync_assets_by_addr_chain(
        ctx: &'static Context,
        addr: Vec<String>,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;

        Self::do_async_balance(ctx, pool, addr, chain_code, SyncFilter::Symbol(symbol)).await
    }

    pub async fn sync_assets_by_addr_chain_token(
        ctx: &'static Context,
        addr: Vec<String>,
        chain_code: Option<String>,
        token_address: AssetTokenKey,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;

        Self::do_async_balance(ctx, pool, addr, chain_code, SyncFilter::Token(token_address)).await
    }

    // 从后端同步余额(根据地址-链)
    pub async fn async_balance_from_backend_addr(
        ctx: &Context,
        addr: String,
        chain_code: Option<String>,
    ) -> Result<(), ServiceError> {
        // 单个地址处理
        let pool = ctx.core_pool()?;

        let backhand = ctx.get_global_backend_api();

        // 获取这个地址对应的链码,如果未传
        let codes = if let Some(chain_code) = chain_code.clone() {
            vec![chain_code]
        } else {
            let account = AccountRepo::list_in_address(pool.clone(), &[addr.clone()], None).await?;

            account.iter().map(|a| a.chain_code.clone()).collect::<Vec<String>>()
        };

        for code in codes {
            let assets =
                AssetsRepo::backend_balance_assets(&pool, Some(&addr), None, Some(&code), None)
                    .await?;
            let refresh = ctx
                .ordinary_balance_order
                .batch(assets.iter().map(AssetsEntity::get_assets_id))
                .await;
            let resp = backhand.wallet_assets_chain_list(&addr, &code).await?;

            for item in resp.list.into_iter() {
                let amount = wallet_utils::unit::string_to_f64(&item.amount)?;
                if amount >= 0.0 {
                    let assets_id = AssetsId {
                        address: item.address,
                        chain_code: item.chain_code,
                        token_address: item.contract_address.into(),
                    };

                    let Some(ticket) = refresh.get(&assets_id) else { continue };
                    let Some(_current) = ticket.current().await else { continue };
                    let r = AssetsRepo::update_balance(&pool, &assets_id, &item.amount).await;

                    if let Err(e) = r {
                        tracing::warn!("udpate balance error {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    // 从后端同步余额(根据钱包-账号)
    pub async fn async_balance_from_backend_wallet(
        ctx: &Context,
        wallet_address: String,
        account_id: Option<u32>,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;
        let wallet = WalletRepo::detail(pool.clone(), &wallet_address).await?;

        if let Some(wallet) = wallet {
            let backhand = ctx.get_global_backend_api();

            // 本地的index 进行了 + 1
            let index = account_id.map(|x| x - 1);
            let assets = AssetsRepo::backend_balance_assets(
                &pool,
                None,
                Some(&wallet_address),
                None,
                account_id,
            )
            .await?;
            let refresh = ctx
                .ordinary_balance_order
                .batch(assets.iter().map(AssetsEntity::get_assets_id))
                .await;
            let resp = backhand.wallet_assets_list(wallet.uid, index).await?;

            tracing::warn!("resp = {:#?}", resp);
            for item in resp.list.into_iter() {
                let amount = wallet_utils::unit::string_to_f64(&item.amount)?;
                if amount >= 0.0 {
                    let assets_id = AssetsId {
                        address: item.address,
                        chain_code: item.chain_code,
                        token_address: item.contract_address.into(),
                    };

                    let Some(ticket) = refresh.get(&assets_id) else { continue };
                    let Some(_current) = ticket.current().await else { continue };
                    let r = AssetsRepo::update_balance(&pool, &assets_id, &item.amount).await;

                    if let Err(e) = r {
                        tracing::warn!("udpate balance error {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn do_async_balance(
        ctx: &'static Context,
        pool: CoreDbPool,
        addr: Vec<String>,
        chain_code: Option<String>,
        filter: SyncFilter,
    ) -> Result<(), ServiceError> {
        let mut assets = AssetsRepo::all_assets(&pool, addr, chain_code, None, None).await?;

        let mode = match &filter {
            SyncFilter::Token(_) => "token_key",
            SyncFilter::Symbol(_) => "symbol_filter",
        };
        let (selected_assets, filtered_out) = select_assets_for_sync(assets, &filter);
        if !filtered_out.is_empty() {
            let filter_desc = match &filter {
                SyncFilter::Token(token_address) => format!("token={}", token_address),
                SyncFilter::Symbol(symbol) => format!("symbol={symbol:?}"),
            };
            tracing::debug!(
                "过滤掉 {} 个资产: mode={}, filter={}, filtered_out={:?}",
                filtered_out.len(),
                mode,
                filter_desc,
                filtered_out
            );
        }
        assets = selected_assets;

        let refresh =
            ctx.ordinary_balance_order.batch(assets.iter().map(AssetsEntity::get_assets_id)).await;
        let results = ChainBalance::sync_address_balance(ctx, assets.as_slice()).await?;

        let mut done = 0;
        for (assets_id, balance) in &results {
            let Some(ticket) = refresh.get(assets_id) else { continue };
            let Some(_current) = ticket.current().await else { continue };
            match AssetsRepo::update_balance(&pool, assets_id, balance).await {
                Ok(_) => {
                    tracing::info!("更新余额成功: {:?}", assets_id);
                    done += 1;
                }
                Err(e) => tracing::error!("更新余额出错: {:?}", e),
            }
        }
        if done > 0 {
            if let Err(e) =
                FrontendNotifyEvent::new(NotifyEvent::SyncAssets).send_with_ctx(ctx).await
            {
                tracing::error!("send error: {}", e);
            }
        }

        Ok(())
    }

    pub(crate) async fn init_default_assets(
        ctx: &Context,
        coins: &[CoinEntity],
        address: &str,
        chain_code: &str,
        req: &mut TokenQueryPriceReq,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;
        for coin in coins {
            if chain_code == coin.chain_code {
                let assets_id =
                    AssetsId::new(address, &coin.chain_code, coin.token_address.clone());
                let assets = CreateAssetsVo::new(
                    assets_id,
                    &coin.symbol,
                    coin.decimals,
                    coin.protocol.clone(),
                    0,
                )
                .with_name(&coin.name)
                .with_u256(alloy::primitives::U256::default(), coin.decimals)?;
                if coin.price.is_empty() {
                    req.insert(chain_code, assets.assets_id.token_address.as_db_str());
                }
                AssetsRepo::upsert_assets(&pool, assets).await?;
            }
        }
        Ok(())
    }

    // 根据地址和链初始化多签账号里面的资产
    // address :multisig account address ,
    pub async fn init_default_multisig_assets(
        ctx: &'static Context,
        address: String,
        chain_code: String,
    ) -> Result<(), ServiceError> {
        let pool = ctx.core_pool()?;
        let default_coins =
            CoinRepo::list_v2(&pool, None, Some(chain_code.clone()), Some(1)).await?;
        let mut token_keys = Vec::new();
        for coin in default_coins {
            let assets_id = AssetsId::new(&address, &chain_code, coin.token_address.clone());
            let assets = CreateAssetsVo::new(
                assets_id,
                &coin.symbol,
                coin.decimals,
                coin.protocol.clone(),
                CoinMultisigStatus::IsMultisig.to_i8() as i32,
            )
            .with_name(&coin.name)
            .with_u256(alloy::primitives::U256::default(), coin.decimals)?;

            AssetsRepo::upsert_assets(&pool, assets).await?;
            token_keys.push(coin.token_address);
        }

        // 同步资产余额（内部路径按 token-key 驱动，不依赖 symbol）
        let mut seen = std::collections::HashSet::new();
        for token_key in token_keys.into_iter().filter(|key| seen.insert(key.clone())) {
            AssetsDomain::sync_assets_by_addr_chain_token(
                ctx,
                vec![address.clone()],
                Some(chain_code.clone()),
                token_key,
            )
            .await?;
        }
        Ok(())
    }

    // swap 增加本地不存在的资产
    pub async fn swap_sync_assets(
        ctx: &Context,
        token: SwapTokenInfo,
        recipient: String,
        chain_code: String,
    ) -> Result<(), ServiceError> {
        // notes 不能更新币价
        let pool = ctx.core_pool()?;
        let core_pool = pool.clone();
        // let time = wallet_utils::time::now();
        let coin = CoinRepo::coin_by_chain_token_key(
            &chain_code,
            AssetTokenKey::from_raw(Some(token.token_addr.as_str())),
            &core_pool,
        )
        .await?;
        // let coin_data = CoinData::new(
        //     Some(token.symbol.clone()),
        //     &token.symbol,
        //     &chain_code,
        //     Some(token.token_addr.clone()),
        //     Some("0".to_string()),
        //     None,
        //     token.decimals as u8,
        //     0,
        //     0,
        //     1,
        //     true,
        //     time,
        //     time,
        // );
        // if let Err(e) = CoinRepo::upsert_multi_coin(&pool, vec![coin_data]).await {
        //     tracing::error!("swap insert coin faild : {}", e);
        // };

        // 资产是否存在不存在新增
        let assets_id = AssetsId::new(
            &recipient,
            &chain_code,
            AssetTokenKey::from_raw(Some(token.token_addr.as_str())),
        );
        let assets = CreateAssetsVo::new(assets_id, &token.symbol, token.decimals as u8, None, 0)
            .with_name(&coin.name);

        if let Err(e) = AssetsRepo::upsert_assets(&pool, assets).await {
            tracing::error!("swap insert assets faild : {}", e);
        };

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BalanceTask {
    pub(crate) address: String,
    pub(crate) chain_code: String,
    pub(crate) symbol: String,
    pub(crate) decimals: u8,
    pub(crate) token_address: AssetTokenKey,
}

pub(crate) struct BalanceTasks(pub(crate) Vec<BalanceTask>);
pub(crate) struct ChainBalance;

impl From<&[AssetsEntity]> for BalanceTasks {
    fn from(assets: &[AssetsEntity]) -> Self {
        BalanceTasks(
            assets
                .iter()
                .map(|asset| BalanceTask {
                    address: asset.address.clone(),
                    chain_code: asset.chain_code.clone(),
                    symbol: asset.symbol.clone(),
                    decimals: asset.decimals,
                    token_address: asset.token_key(),
                })
                .collect(),
        )
    }
}

impl From<&[ApiAssetsEntity]> for BalanceTasks {
    fn from(assets: &[ApiAssetsEntity]) -> Self {
        BalanceTasks(
            assets
                .iter()
                .map(|asset| {
                    tracing::info!(
                        source = "api_assets.decimals",
                        address = %asset.address,
                        chain_code = %asset.chain_code,
                        symbol = %asset.symbol,
                        token_address = %asset.token_address,
                        decimals = asset.decimals,
                        balance = %asset.balance,
                        "构建余额同步任务"
                    );

                    BalanceTask {
                        address: asset.address.clone(),
                        chain_code: asset.chain_code.clone(),
                        symbol: asset.symbol.clone(),
                        decimals: asset.decimals,
                        token_address: asset.token_key(),
                    }
                })
                .collect(),
        )
    }
}

impl ChainBalance {
    pub(crate) async fn sync_address_balance(
        ctx: &Context,
        assets: impl Into<BalanceTasks>,
    ) -> Result<Vec<(AssetsId, String)>, crate::error::service::ServiceError> {
        // 限制最大并发数为 10
        let sem = Arc::new(Semaphore::new(10));
        let tasks: BalanceTasks = assets.into();

        // 并发获取余额并格式化
        let results = stream::iter(tasks.0)
            .map(|task| Self::fetch_balance(task, sem.clone(), ctx))
            .buffer_unordered(10)
            .filter_map(|x| async move { x })
            .collect::<Vec<_>>()
            .await;
        tracing::info!("results: {results:#?}");
        Ok(results)
    }

    // 从任务获取余额并返回结果
    async fn fetch_balance(
        task: BalanceTask,
        sem: Arc<Semaphore>,
        ctx: &Context,
    ) -> Option<(AssetsId, String)> {
        // 获取并发许可
        let _permit = sem.acquire().await.ok()?;

        // 获取适配器
        let adapter = ChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, &task.chain_code)
            .await
            .map_err(|e| {
                tracing::error!("获取链详情出错: {}，链代码: {}", e, task.chain_code.clone())
            })
            .ok()?;

        // 获取余额
        let raw = adapter
            .balance(&task.address, task.token_address.clone())
            .await
            .map_err(|e| {
                tracing::error!(
                    "获取余额出错: 地址={}, 链={}, 符号={}, token={:?}, 错误={}",
                    task.address,
                    task.chain_code,
                    task.symbol,
                    task.token_address,
                    e
                )
            })
            .ok()?;

        // 格式化
        let bal_str = wallet_utils::unit::format_to_string(raw, task.decimals)
            .unwrap_or_else(|_| "0".to_string());

        // 构建 ID
        let id = AssetsId {
            address: task.address,
            chain_code: task.chain_code,
            token_address: task.token_address,
        };

        Some((id, bal_str))
    }
}

#[cfg(test)]
mod tests {
    use super::{SyncFilter, filter_assets_for_sync, select_assets_for_sync};
    use wallet_database::entities::{asset_token_key::AssetTokenKey, assets::AssetsEntity};

    fn make_asset(
        symbol: &str,
        address: &str,
        chain_code: &str,
        token_address: &str,
    ) -> AssetsEntity {
        AssetsEntity {
            name: symbol.to_string(),
            symbol: symbol.to_string(),
            decimals: 6,
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            token_address: AssetTokenKey::from(token_address),
            protocol: None,
            status: 1,
            is_multisig: 0,
            balance: "0".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn token_case_normal_wallet_selects_evm_only() {
        let (selected, _) = filter_assets_for_sync(
            vec![make_asset("USDT", "sender", "polygon", "0xaBc")],
            &"0xabc".into(),
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].token_address.as_db_str(), "0xaBc");
        let (selected, _) = filter_assets_for_sync(
            vec![make_asset("USDT", "sender", "tron", "TAbC")],
            &"tabc".into(),
        );
        assert!(selected.is_empty());
    }

    #[test]
    fn normal_assets_sync_filter_matches_by_token_when_symbol_differs() {
        let assets = vec![
            make_asset("USDC", "same-addr", "sol", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            make_asset("USD1", "same-addr", "sol", "other-token"),
        ];

        let (matched, filtered_out) = filter_assets_for_sync(
            assets,
            &AssetTokenKey::from("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        );

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].symbol, "USDC");
        assert_eq!(filtered_out.len(), 1);
    }

    #[test]
    fn normal_assets_sync_filter_matches_native_by_empty_token() {
        let assets = vec![
            make_asset("SOL", "same-addr", "sol", ""),
            make_asset("USDC", "same-addr", "sol", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        ];

        let (matched, filtered_out) = filter_assets_for_sync(assets, &AssetTokenKey::Native);

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].token_key(), AssetTokenKey::Native);
        assert_eq!(filtered_out.len(), 1);
    }

    #[test]
    fn normal_assets_manual_sync_keeps_symbol_filter_when_token_missing() {
        let assets = vec![
            make_asset("USDT", "same-addr", "eth", "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            make_asset("USDC", "same-addr", "eth", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        ];

        let (matched, filtered_out) =
            select_assets_for_sync(assets, &SyncFilter::Symbol(vec![String::from("USDT")]));

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].symbol, "USDT");
        assert_eq!(filtered_out.len(), 1);
    }

    #[test]
    fn normal_assets_manual_sync_keeps_full_sync_when_symbol_empty() {
        let assets = vec![
            make_asset("USDT", "same-addr", "eth", "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            make_asset("USDC", "same-addr", "eth", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
        ];

        let (matched, filtered_out) = select_assets_for_sync(assets, &SyncFilter::Symbol(vec![]));

        assert_eq!(matched.len(), 2);
        assert!(filtered_out.is_empty());
    }
}
