use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::{DashMap, DashSet};
use once_cell::sync::Lazy;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tracing::debug;
use wallet_database::repositories::{
    api_wallet::{account::ApiAccountRepo, assets::ApiAssetsRepo},
    exchange_rate::ExchangeRateRepo,
};
use wallet_transport_backend::response_vo::coin::TokenCurrency;
use crate::context::Context;

use crate::{
    domain::app::config::ConfigDomain,
    response_vo::{
        account::BalanceInfo,
        coin::{TokenCurrencies, TokenCurrencyId},
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetKey {
    pub wallet_address: String,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
}

impl AssetKey {
    fn new(wallet_address: &str, address: &str, chain_code: &str, token_address: &str) -> AssetKey {
        AssetKey {
            wallet_address: wallet_address.to_string(),
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_address.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AssetEntry {
    pub wallet_address: String,
    pub address: String,
    pub symbol: String,
    pub chain_code: String,
    pub token_address: String,
    pub balance: Decimal,
    pub decimals: u8,
}

/// key：账户地址，value：钱包地址
static ADDRESS_TO_WALLET: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
/// key：账户地址，value：账户ID
static ADDRESS_TO_ACCOUNT_ID: Lazy<RwLock<HashMap<String, u32>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static ACCOUNT_VALUE_CACHE: Lazy<DashMap<(String, String), Decimal>> = Lazy::new(|| DashMap::new());
// key: (wallet_address, account_address)

static TOKEN_CURRENCIES: Lazy<Arc<RwLock<TokenCurrencies>>> =
    Lazy::new(|| Arc::new(RwLock::new(TokenCurrencies::default())));
// static PRICE_CACHE: Lazy<DashMap<String, PriceEntry>> = Lazy::new(|| DashMap::new());
static DIRTY_PRICE_SET: Lazy<DashSet<TokenCurrencyId>> = Lazy::new(|| DashSet::new());
static ASSET_DIRTY_SET: Lazy<DashSet<AssetKey>> = Lazy::new(|| DashSet::new());
// static ASSET_DIRTY_SET: Lazy<DashSet<TokenCurrencyId>> = Lazy::new(|| DashSet::new());

static ASSET_VALUE_CACHE: Lazy<DashMap<AssetKey, BalanceInfo>> = Lazy::new(|| DashMap::new());
static TOTAL_USDT: Lazy<RwLock<Decimal>> = Lazy::new(|| RwLock::new(Decimal::ZERO));

// 汇率缓存结构
#[derive(Clone, Debug)]
struct ExchangeRateCache {
    rates: HashMap<String, f64>,
    last_updated: Instant,
}

// 全局汇率缓存，设置有效期5分钟
static EXCHANGE_RATE_CACHE: Lazy<Arc<RwLock<ExchangeRateCache>>> = Lazy::new(|| {
    Arc::new(RwLock::new(ExchangeRateCache {
        rates: HashMap::new(),
        last_updated: Instant::now() - Duration::from_secs(60 * 6), // 初始设置为6分钟前，确保首次调用会更新
    }))
});

// 缓存有效期（5分钟）
const EXCHANGE_RATE_CACHE_DURATION: Duration = Duration::from_secs(60 * 5);

// 获取汇率数据的辅助函数，带缓存机制
async fn get_exchange_rates(
    pool: &Arc<SqlitePool>,
) -> Result<HashMap<String, f64>, crate::error::service::ServiceError> {
    let mut cache = EXCHANGE_RATE_CACHE.write().await;

    // 检查缓存是否过期
    if Instant::now() - cache.last_updated < EXCHANGE_RATE_CACHE_DURATION {
        return Ok(cache.rates.clone());
    }

    // 缓存过期，重新从数据库加载
    let exchange_rate_list = ExchangeRateRepo::list(pool).await?;

    // 更新缓存
    let mut rates = HashMap::new();
    for rate in exchange_rate_list {
        rates.insert(rate.target_currency.clone(), rate.rate);
    }

    cache.rates = rates.clone();
    cache.last_updated = Instant::now();

    Ok(rates)
}

pub async fn init_account_cache(
    ctx: &'static Context,
) -> Result<(), crate::error::service::ServiceError> {
    let pool = ctx.get_global_sqlite_pool()?;
    let wallet_map = ApiAccountRepo::account_to_wallet(&pool).await?;
    let account_list = ApiAccountRepo::list(&pool).await?;

    {
        let mut map = ADDRESS_TO_WALLET.write().await;
        map.clear();
        for wallet in wallet_map {
            map.insert(wallet.address.clone(), wallet.wallet_address.clone());
        }
    }

    {
        let mut map = ADDRESS_TO_ACCOUNT_ID.write().await;
        map.clear();
        for row in account_list {
            map.insert(row.address.clone(), row.account_id);
        }
    }

    Ok(())
}

pub async fn add_account_to_cache(address: &str, account_id: u32, wallet: &str) {
    ADDRESS_TO_ACCOUNT_ID.write().await.insert(address.to_string(), account_id);
    ADDRESS_TO_WALLET.write().await.insert(address.to_string(), wallet.to_string());
}

pub async fn update_token_price(
    ctx: &'static Context,
    symbol: &str,
    chain_code: &str,
    token_address: &Option<String>,
    price_real: f64,
) -> Result<(), crate::error::service::ServiceError> {
    let pool = ctx.get_global_sqlite_pool()?;
    let mut token_currencies = TOKEN_CURRENCIES.write().await;
    let id = TokenCurrencyId::new(symbol, chain_code, token_address.clone());
    let currency = ConfigDomain::get_currency(ctx).await?;

    // 修复汇率计算问题，使用缓存的汇率数据
    let (fiat_price, rate) = {
        let exchange_rates = get_exchange_rates(&pool).await?;
        if let Some(rate_value) = exchange_rates.get(&currency) {
            // 使用Decimal的构建函数而不是from_f64
            let price_decimal = Decimal::new((price_real * 100.0) as i64, 2);
            let rate_decimal = Decimal::new((*rate_value * 100.0) as i64, 2);
            let fiat_price_decimal = price_decimal * rate_decimal;

            debug!(
                "update_token_price symbol: {symbol}, chain_code: {chain_code}, token_address: {token_address:?}, price_real: {price_real}, rate: {}",
                rate_value
            );

            // 如果需要返回f64，可以转换回去，但内部计算应保持Decimal
            (fiat_price_decimal.to_f64(), *rate_value)
        } else {
            (None, 1.0)
        }
    };
    // 更新缓存
    token_currencies
        .entry(id.clone())
        .and_modify(|entry| {
            entry.price = Some(price_real);
            entry.currency_price = fiat_price;
            entry.rate = rate;
        })
        .or_insert(TokenCurrency::new(chain_code, symbol, "", Some(price_real), fiat_price, rate));
    // let id = TokenCurrencyId::new("TRX", "tron", None);
    // tracing::info!("update_token_price: token_currencies: {:#?}", token_currencies.get(&id));

    // 标记 dirty，用于触发资产估值刷新
    DIRTY_PRICE_SET.insert(id);

    Ok(())
}

/// Called when a new asset is inserted or its balance changes
pub fn on_asset_update(wallet_address: &str, address: &str, chain_code: &str, token_address: &str) {
    // 添加输入验证
    if wallet_address.is_empty() || address.is_empty() || chain_code.is_empty() {
        tracing::error!(
            "Invalid input for on_asset_update: wallet_address={}, address={}, chain_code={}",
            wallet_address,
            address,
            chain_code
        );
        return;
    }

    let k = AssetKey::new(wallet_address, address, chain_code, token_address);
    ASSET_DIRTY_SET.insert(k);
}

pub async fn init_assets(ctx: &'static Context) -> Result<(), crate::error::service::ServiceError> {
    let pool = ctx.get_global_sqlite_pool()?;
    let list = ApiAssetsRepo::list(&pool, vec![], None).await?;
    let wallet_list = ApiAccountRepo::account_wallet_mapping(&pool, None).await?;
    list.into_iter().for_each(|asset| {
        if let Some(wallet) = wallet_list.iter().find(|wallet| wallet.address == asset.address) {
            on_asset_update(
                &wallet.wallet_address,
                &asset.address,
                &asset.chain_code,
                &asset.token_address,
            );
        }
    });

    Ok(())
}

/// Start the periodic batch recalculation background task.
/// interval_ms: how often to run the batch recalculation (e.g. 500 or 1000)
pub fn start_batch_recalculator(
    ctx: &'static Context,
    interval_ms: u64,
) -> Result<(), crate::error::service::ServiceError> {
    let pool = ctx.get_global_sqlite_pool()?;
    tokio::spawn(async move {
        let interval = Duration::from_millis(interval_ms);
        loop {
            tokio::time::sleep(interval).await;

            // --- collect dirty sets ---
            let price_keys: Vec<TokenCurrencyId> =
                DIRTY_PRICE_SET.iter().map(|k| k.clone()).collect();
            let asset_keys: Vec<AssetKey> = ASSET_DIRTY_SET.iter().map(|id| id.clone()).collect();

            if price_keys.is_empty() && asset_keys.is_empty() {
                continue;
            }

            tracing::debug!(
                "batch recalculation started: price_keys={}, asset_ids={}",
                price_keys.len(),
                asset_keys.len()
            );

            // clear old dirty marks
            for k in &price_keys {
                DIRTY_PRICE_SET.remove(k);
            }
            for id in &asset_keys {
                ASSET_DIRTY_SET.remove(id);
            }

            if !price_keys.is_empty() {
                if let Err(e) = process_price_dirty_assets(ctx, &pool, &price_keys).await {
                    tracing::error!("process_price_dirty_assets error: {:?}", e);
                }
            }

            if !asset_keys.is_empty() {
                if let Err(e) = process_asset_dirty_assets(ctx, &pool, &asset_keys).await {
                    tracing::error!("process_asset_dirty_assets error: {:?}", e);
                }
            }
        }
    });
    Ok(())
}

async fn process_price_dirty_assets(
    ctx: &'static Context,
    pool: &Arc<SqlitePool>,
    keys: &[TokenCurrencyId],
) -> Result<(), Box<dyn std::error::Error>> {
    // process in chunks to avoid huge IN lists
    const CHUNK_KEYS: usize = 200;
    let currency = ConfigDomain::get_currency(ctx).await?;

    for chunk in keys.chunks(CHUNK_KEYS) {
        let mut keys = Vec::new();
        for key in chunk {
            keys.push(key.gen_key());
        }

        let assets_list = ApiAssetsRepo::assets_with_wallet_address_by_token(pool, &keys).await?;

        let mut assets = Vec::new();
        for asset in assets_list.into_iter() {
            let balance = wallet_utils::parse_func::decimal_from_str(&asset.balance)?;
            let asset_entry = AssetEntry {
                wallet_address: asset.wallet_address,
                address: asset.address,
                symbol: asset.symbol,
                chain_code: asset.chain_code,
                token_address: asset.token_address,
                balance,
                decimals: asset.decimals,
            };

            assets.push(asset_entry);
        }

        // 获取最新的token_currencies数据，而不是使用可能过期的快照
        let token_currencies = {
            // 使用读锁获取当前最新的汇率数据
            let guard = TOKEN_CURRENCIES.read().await;
            guard.clone()
        };

        // 使用最新数据进行聚合和通知
        crate::infrastructure::asset_calc::asset_sync::aggregate_and_notify(&assets, token_currencies, currency.clone()).await;
    }

    Ok(())
}

/// Handle asset dirty IDs
async fn process_asset_dirty_assets(
    ctx: &'static Context,
    pool: &Arc<SqlitePool>,
    keys: &[AssetKey],
) -> Result<(), Box<dyn std::error::Error>> {
    const CHUNK_SIZE: usize = 200;
    let currency = ConfigDomain::get_currency(ctx).await?;

    for chunk in keys.chunks(CHUNK_SIZE) {
        let mut keys = Vec::new();
        for key in chunk {
            keys.push(format!("{}:{}:{}", key.address, key.chain_code, key.token_address));
        }
        let assets_list = ApiAssetsRepo::assets_with_wallet_address_by_address(pool, &keys).await?;
        let mut assets = Vec::new();
        for asset in assets_list.into_iter() {
            let balance = wallet_utils::parse_func::decimal_from_str(&asset.balance)?;
            let asset_entry = AssetEntry {
                wallet_address: asset.wallet_address,
                address: asset.address,
                symbol: asset.symbol,
                chain_code: asset.chain_code,
                token_address: asset.token_address,
                balance,
                decimals: asset.decimals,
            };

            assets.push(asset_entry);
        }

        // 获取最新的token_currencies数据，确保使用最新汇率
        let token_currencies = {
            let guard = TOKEN_CURRENCIES.read().await;
            guard.clone()
        };

        // 使用最新数据进行聚合和通知
        crate::infrastructure::asset_calc::asset_sync::aggregate_and_notify(&assets, token_currencies, currency.clone()).await;
        crate::infrastructure::asset_calc::asset_sync::affected_accounts(assets).await;
        // let data_map = ApiWalletSyncAssetsMsgFront::new();
        // assets.par_iter().for_each(|a| {
        //     // let price_key = make_key(&a.symbol, &a.chain_code, &a.token_address);
        //     let asset_key =
        //         AssetKey::new(&a.wallet_address, &a.address, &a.chain_code, &a.token_address);
        //     let balance_info = token_currencies_snapshot
        //         .calculate_sync_to_balance(
        //             &currency,
        //             &a.balance.to_string(),
        //             &a.symbol,
        //             &a.chain_code,
        //             Some(a.token_address.clone()),
        //         )
        //         .unwrap_or(BalanceInfo {
        //             amount: 0.0,
        //             currency: "".to_string(),
        //             unit_price: None,
        //             fiat_value: None,
        //         });

        //     data_map.add_item(
        //         &asset_key.wallet_address,
        //         ApiWalletSyncAccountBalanceMsgFrontItem::new(
        //             &asset_key.address,
        //             &asset_key.chain_code,
        //             &asset_key.token_address,
        //             balance_info.clone(),
        //         ),
        //     );
        //     ASSET_VALUE_CACHE.insert(asset_key, balance_info);
        // });

        // if let Err(e) =
        //     FrontendNotifyEvent::new(NotifyEvent::ApiWalletSyncAssets(data_map)).send().await
        // {
        //     tracing::error!("send error: {}", e);
        // }
    }

    Ok(())
}

/// Get current total snapshot
pub async fn get_total_usdt() -> Decimal {
    *TOTAL_USDT.read().await
}

/// Get current price cache
pub async fn get_price_cache() {
    tracing::debug!("get_price_cache: {:#?}", TOKEN_CURRENCIES);
    // let g = PRICE_CACHE.read().await;
    // g.clone()
}

pub async fn get_wallet_balance_list()
-> Result<HashMap<String, BalanceInfo>, crate::error::service::ServiceError> {
    let account_to_wallet = ADDRESS_TO_WALLET.read().await;
    let mut wallet_totals: HashMap<String, BalanceInfo> = HashMap::new();

    // tracing::info!("get_wallet_balance_list: {:#?}", ASSET_VALUE_CACHE);
    for entry in ASSET_VALUE_CACHE.iter() {
        // if let Some(address) = entry.key().address.split(':').next() {
        // tracing::info!("entry value: {}", address);
        if let Some(wallet_address) = account_to_wallet.get(&entry.key().address) {
            // tracing::info!("get_wallet_balance_list: wallet_address: {:?}", wallet_address);
            let entry_value = entry.value();
            // tracing::info!("get_wallet_balance_list amount: {}", entry_value.amount);
            wallet_totals
                .entry(wallet_address.clone())
                .and_modify(|total| {
                    total.amount_add(entry_value.amount);
                    total.fiat_add(entry_value.fiat_value);
                })
                .or_insert_with(|| entry_value.clone());
            // 👆 用 or_insert_with + clone()，因为 entry_value 是引用
        }
        // }
    }

    Ok(wallet_totals)
}

pub async fn get_account_balance_list_by_wallet(
    wallet_address: &str,
    chain_code: Option<String>,
) -> Result<HashMap<String, BalanceInfo>, crate::error::service::ServiceError> {
    let map = ADDRESS_TO_WALLET.read().await;

    let account_addresses: Vec<String> = map
        .iter()
        .filter_map(
            |(addr, wallet)| {
                if wallet == wallet_address { Some(addr.clone()) } else { None }
            },
        )
        .collect();

    if account_addresses.is_empty() {
        return Ok(HashMap::new());
    }

    let mut account_totals: HashMap<String, BalanceInfo> = HashMap::new();
    for entry in ASSET_VALUE_CACHE.iter() {
        let address = &entry.key().address;
        let asset_chain_code = &entry.key().chain_code;

        let chain_match = match chain_code {
            Some(ref code) => asset_chain_code == code,
            None => true,
        };

        if chain_match && account_addresses.contains(&address.to_string()) {
            let entry_value = entry.value();
            account_totals
                .entry(address.to_string())
                .and_modify(|total| {
                    total.amount_add(entry_value.amount);
                    total.fiat_add(entry_value.fiat_value);
                })
                .or_insert_with(|| entry_value.clone());
        }
    }

    Ok(account_totals)
}

pub async fn get_balance_summary(
    wallet_address: Option<&str>,
    account_id: Option<u32>,
    chain_code: Option<&str>,
) -> Result<BalanceInfo, crate::error::service::ServiceError> {
    let mut total = BalanceInfo::default();
    // 拿到 account -> wallet 映射
    let map = ADDRESS_TO_WALLET.read().await;
    // 根据参数筛选目标地址集合
    let mut target_addresses: Vec<String> = Vec::new();

    match (wallet_address, account_id) {
        (None, None) => {
            // 全部账户
            // for row in &list {
            //     target_addresses.push(row.address.clone());
            // }
            target_addresses = map.keys().cloned().collect();
        }
        (Some(wallet), None) => {
            // 指定钱包下所有账户
            // for row in &list {
            //     if row.wallet_address == wallet {
            //         target_addresses.push(row.address.clone());
            //     }
            // }
            target_addresses = map
                .iter()
                .filter_map(|(addr, w)| if w == wallet { Some(addr.clone()) } else { None })
                .collect();
        }
        (Some(wallet), Some(id)) => {
            // 指定钱包 + 账户
            let map_id = ADDRESS_TO_ACCOUNT_ID.read().await;
            // let list =
            //     ApiAccountRepo::list_by_wallet_address(&pool, wallet, Some(id), chain_code).await?;
            // for account in list {
            //     if account.wallet_address == wallet {
            //         target_addresses.push(account.address);
            //     } else {
            //         tracing::warn!("account {id} not belongs to wallet {wallet}");
            //     }
            // }
            target_addresses = map_id
                .iter()
                .filter_map(|(addr, aid)| {
                    if *aid == id && map.get(addr).map(|w| w == wallet).unwrap_or(false) {
                        Some(addr.clone())
                    } else {
                        None
                    }
                })
                .collect();
        }
        _ => {
            return Ok(total);
        }
    }
    // tracing::info!("target_addresses: {:?}", target_addresses);
    if target_addresses.is_empty() {
        return Ok(total);
    }

    // 遍历缓存，按条件聚合
    for entry in ASSET_VALUE_CACHE.iter() {
        let address = &entry.key().address;
        let asset_chain_code = &entry.key().chain_code;

        // 筛选地址
        if !target_addresses.contains(address) {
            continue;
        }

        // 筛选链
        if let Some(chain_filter) = chain_code {
            if asset_chain_code != chain_filter {
                continue;
            }
        }

        // 累加金额
        let entry_value = entry.value();
        total.amount_add(entry_value.amount);
        total.fiat_add(entry_value.fiat_value);
    }

    Ok(total)
}
