use crate::{
    domain::{app::config::ConfigDomain, coin::CoinDomain},
    error::{service::ServiceError, system::SystemError},
    messaging::notify::{
        FrontendNotifyEvent,
        api_wallet::{ApiWalletSyncAccountBalanceMsgFrontItem, ApiWalletSyncAssetsMsgFront},
        event::NotifyEvent,
    },
    response_vo::standard_wallet::{
        account::BalanceInfo,
        coin::{TokenCurrencies, TokenCurrencyId},
    },
};
use dashmap::{DashMap, DashSet};
use rust_decimal::Decimal;
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};
use wallet_database::{
    entities::asset_token_key::AssetTokenKey,
    repositories::{
        api_wallet::{assets::ApiAssetsRepo, chain::ApiChainRepo},
        exchange_rate::ExchangeRateRepo,
    },
};

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

// 批量初始化币价所需的数据结构
#[derive(Clone)]
pub struct CoinInitializationData {
    pub symbol: String,
    pub chain_code: String,
    pub name: String,
    pub token_address: AssetTokenKey,
    pub price_real: f64,
    pub decimals: u8,
}

// 定义消息类型
enum AssetCalcMessage {
    // 批量初始化币价
    BatchInitializePrices {
        coins: Vec<CoinInitializationData>,
        response_tx: mpsc::Sender<Result<(), ServiceError>>,
    },
    // 批量资产更新消息
    BatchAssetUpdate {
        updates: Vec<AssetKey>,
        response_tx: mpsc::Sender<Result<(), ServiceError>>,
    },
    // 价格更新消息
    PriceUpdate {
        symbol: String,
        chain_code: String,
        name: String,
        token_address: AssetTokenKey,
        price_real: f64,
        decimals: u8,
        response_tx: mpsc::Sender<Result<(), ServiceError>>,
    },
    // 获取钱包余额
    GetWalletBalance {
        response_tx: mpsc::Sender<Result<HashMap<String, BalanceInfo>, ServiceError>>,
    },
    // 获取账户余额
    GetAccountBalance {
        wallet_address: String,
        chain_code: Option<String>,
        response_tx: mpsc::Sender<Result<HashMap<String, BalanceInfo>, ServiceError>>,
    },
    // 获取余额摘要
    GetBalanceSummary {
        wallet_address: Option<String>,
        account_id: Option<u32>,
        chain_code: Option<String>,
        response_tx: mpsc::Sender<Result<BalanceInfo, ServiceError>>,
    },
    // 初始化账户缓存
    InitAccountCache {
        response_tx: mpsc::Sender<Result<(), ServiceError>>,
    },
    // 添加账户到缓存
    AddAccountToCache {
        address: String,
        account_id: u32,
        wallet: String,
        response_tx: mpsc::Sender<()>,
    },
    // 获取总价值
    GetTotalUsdt {
        response_tx: mpsc::Sender<Decimal>,
    },
    // 启动批处理任务
    StartBatchRecalculator {
        interval_ms: u64,
        response_tx: mpsc::Sender<Result<(), ServiceError>>,
    },
    SendAffectedAccounts {
        assets: Vec<AssetEntry>,
    },
    AggregateAndNotify {
        assets: Vec<AssetEntry>,
    },
    // 处理批处理更新
    ProcessBatchUpdates,
    //
    RefreshAllCaches,
}

// Actor状态结构
pub struct AssetCalcState {
    // 数据库连接池
    pool: Arc<SqlitePool>,

    // 地址到钱包的映射
    address_to_wallet: HashMap<String, String>,
    // 地址到账户ID的映射
    pub(crate) address_to_account_id: HashMap<String, u32>,

    // 账户价值缓存
    // account_value_cache: DashMap<(String, String), Decimal>,

    // 资产价值缓存
    asset_value_cache: DashMap<AssetKey, BalanceInfo>,

    // 汇率缓存
    exchange_rate_cache: ExchangeRateCache,

    // Token货币信息
    token_currencies: TokenCurrencies,

    // 脏价格集合
    dirty_price_set: DashSet<TokenCurrencyId>,
    // 脏资产集合
    dirty_asset_set: DashSet<AssetKey>,

    // 总价值
    total_usdt: Decimal,

    // 缓存更新时间
    last_cache_update: Instant,
    // 缓存条目过期时间
    cache_ttl: Duration,
}

impl AssetCalcState {
    // 初始化状态
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self {
            pool,
            address_to_wallet: HashMap::new(),
            address_to_account_id: HashMap::new(),
            // account_value_cache: DashMap::new(),
            asset_value_cache: DashMap::new(),
            exchange_rate_cache: ExchangeRateCache {
                rates: HashMap::new(),
                last_updated: Instant::now() - Duration::from_secs(60 * 6), // 初始设置为过期
            },
            token_currencies: TokenCurrencies::default(),
            dirty_price_set: DashSet::new(),
            dirty_asset_set: DashSet::new(),
            total_usdt: Decimal::ZERO,
            last_cache_update: Instant::now(),
            cache_ttl: Duration::from_secs(5 * 60), // 默认5分钟过期
        }
    }

    // 检查缓存是否过期
    pub fn is_cache_expired(&self) -> bool {
        // tracing::debug!("Cache TTL: {:?}", self.cache_ttl);
        let cache_age = Instant::now() - self.last_cache_update;
        // tracing::debug!("Cache age: {:?}", cache_age);
        cache_age > self.cache_ttl
    }

    // 更新缓存时间
    pub fn update_cache_timestamp(&mut self) {
        self.last_cache_update = Instant::now();
    }

    // 清除过期的缓存项
    pub fn cleanup_expired_cache(&mut self) {
        // 这里可以实现基于时间的缓存清理逻辑
        // 目前我们使用简单的全局过期策略
        if self.is_cache_expired() {
            // 清除部分或全部缓存
            // 注意：实际应用中，可能需要更精细的过期策略
            debug!("Cache expired, cleaning up asset_value_cache entries");
            // 为避免过度清理，我们可以选择性地只清理部分过期项
            // 例如，对于很少访问的地址，可以优先清理
        }
    }

    // 批量更新资产值缓存
    pub fn batch_update_asset_cache(&mut self, updates: Vec<(AssetKey, BalanceInfo)>) {
        let mut total_usdt_sum = Decimal::ZERO;

        for (key, value) in updates {
            // 更新缓存
            self.asset_value_cache.insert(key, value);
        }

        // 重新计算总USDT价值
        for entry in self.asset_value_cache.iter() {
            if let Some(fiat_value) = entry.value().fiat_value {
                let price = wallet_types::Decimal::from_f64_retain(fiat_value).unwrap_or_default();
                total_usdt_sum += price;
            }
        }

        // 更新总USDT价值
        self.total_usdt = total_usdt_sum;
        // 更新缓存时间戳
        self.update_cache_timestamp();
    }

    // 标记资产为脏数据
    pub fn mark_asset_dirty(
        &mut self,
        wallet_address: &str,
        address: &str,
        chain_code: &str,
        token_address: &str,
    ) {
        let key = AssetKey::new(wallet_address, address, chain_code, token_address);
        self.dirty_asset_set.insert(key);
    }

    // 标记价格为脏数据
    pub fn mark_price_dirty(
        &mut self,
        symbol: &str,
        chain_code: &str,
        token_address: &AssetTokenKey,
    ) {
        let id = TokenCurrencyId::new(symbol, chain_code, token_address.to_option_string_for_api());
        self.dirty_price_set.insert(id);
    }
}

// 汇率缓存结构
#[derive(Clone, Debug)]
pub struct ExchangeRateCache {
    pub rates: HashMap<String, f64>,
    pub last_updated: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct AssetKey {
    pub wallet_address: String,
    pub address: String,
    pub chain_code: String,
    pub token_address: String,
}

impl AssetKey {
    pub(crate) fn new(
        wallet_address: &str,
        address: &str,
        chain_code: &str,
        token_address: &str,
    ) -> AssetKey {
        AssetKey {
            wallet_address: wallet_address.to_string(),
            address: address.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_address.to_string(),
        }
    }

    // 生成用于数据库查询的键
    pub fn gen_key(&self) -> String {
        format!("{}:{}:{}", self.address, self.chain_code, self.token_address)
    }
}

// AssetCalcActor结构体
struct AssetCalcActor {
    ctx: &'static crate::context::Context,
    state: AssetCalcState,
    sender: mpsc::Sender<AssetCalcMessage>,
    receiver: mpsc::Receiver<AssetCalcMessage>,
}

impl AssetCalcActor {
    // 创建新的Actor
    fn new(
        ctx: &'static crate::context::Context,
        pool: Arc<SqlitePool>,
        sender: mpsc::Sender<AssetCalcMessage>,
        receiver: mpsc::Receiver<AssetCalcMessage>,
    ) -> Self {
        let state = AssetCalcState::new(pool);
        Self { ctx, state: state, sender, receiver }
    }

    /// 获取启用的链列表
    async fn get_enabled_chains(&self) -> Result<HashSet<String>, ServiceError> {
        // 查询状态为1（启用）的链
        let chains = ApiChainRepo::get_chain_list(&self.state.pool).await?;
        // 提取链码到HashSet中以便快速查找
        let enabled_chains: HashSet<String> =
            chains.into_iter().map(|chain| chain.chain_code).collect();
        Ok(enabled_chains)
    }

    // 启动Actor处理循环
    async fn run(mut self) {
        debug!("AssetCalcActor started");

        // 初始化账户缓存
        if let Err(e) = self.handle_init_account_cache().await {
            error!("Failed to initialize account cache: {:?}", e);
        }

        // 初始刷新缓存
        let sender = self.sender.clone();

        // if let Err(e) = self.refresh_all_caches().await {
        //     error!("Failed to perform initial cache refresh: {:?}", e);
        // }

        while let Some(msg) = self.receiver.recv().await {
            match msg {
                AssetCalcMessage::BatchAssetUpdate { updates, response_tx } => {
                    debug!("Received BatchAssetUpdate message with {} updates", updates.len());
                    let mut all_succeeded = true;

                    // 批量处理所有资产更新
                    for asset_key in updates {
                        if let Err(e) = self.handle_asset_update(&asset_key).await {
                            error!(
                                "Failed to update asset in batch: wallet={}, address={}, chain={}, token={}, error={:?}",
                                asset_key.wallet_address,
                                asset_key.address,
                                asset_key.chain_code,
                                asset_key.token_address,
                                e
                            );
                            all_succeeded = false;
                        }
                    }

                    // 只在所有更新完成后发送一次批处理更新消息
                    if all_succeeded {
                        let sender_c = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await
                            {
                                error!(
                                    "Failed to send ProcessBatchUpdates message after batch asset change: {:?}",
                                    e
                                );
                            }
                        });
                    }

                    let result = if all_succeeded {
                        Ok(())
                    } else {
                        Err(ServiceError::System(SystemError::Service(
                            "Some asset updates failed".to_string(),
                        )))
                    };

                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send batch asset update result: {:?}", err);
                    }
                }
                AssetCalcMessage::PriceUpdate {
                    symbol,
                    chain_code,
                    name,
                    token_address,
                    price_real,
                    decimals,
                    response_tx,
                } => {
                    debug!(
                        "Received PriceUpdate message: symbol={}, chain={}, token={:?}, price={}",
                        symbol, chain_code, token_address, price_real
                    );
                    let result = self
                        .handle_price_update(
                            &symbol,
                            &chain_code,
                            &name,
                            &token_address,
                            price_real,
                            decimals,
                        )
                        .await;

                    // 如果更新成功，发送批处理更新消息
                    if result.is_ok() {
                        let sender_c = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await
                            {
                                error!(
                                    "Failed to send ProcessBatchUpdates message after asset change: {:?}",
                                    e
                                );
                            }
                        });
                    }

                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send asset price update result: {:?}", err);
                    }
                }
                AssetCalcMessage::BatchInitializePrices { coins, response_tx } => {
                    debug!("Received BatchInitializePrices message: {} coins", coins.len());
                    let result = self.handle_batch_initialize_prices(coins).await;

                    // 如果初始化成功，发送批处理更新消息
                    if result.is_ok() {
                        let sender_c = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await
                            {
                                error!(
                                    "Failed to send ProcessBatchUpdates message after batch price initialization: {:?}",
                                    e
                                );
                            }
                        });
                    }

                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send batch price initialization result: {:?}", err);
                    }
                }
                AssetCalcMessage::GetWalletBalance { response_tx } => {
                    debug!("Received GetWalletBalance message");
                    // 检查缓存是否过期，如果过期则刷新
                    self.ensure_cache_fresh().await;

                    let result = self.handle_get_wallet_balance().await;
                    debug!(
                        "Wallet balance calculated, result entries: {}",
                        result.as_ref().map_or(0, |m| m.len())
                    );
                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send wallet balance result: {:?}", err);
                    }
                }
                AssetCalcMessage::GetAccountBalance { wallet_address, chain_code, response_tx } => {
                    // 检查缓存是否过期，如果过期则刷新
                    self.ensure_cache_fresh().await;

                    let result =
                        self.handle_get_account_balance(&wallet_address, chain_code.clone()).await;
                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send account balance result: {:?}", err);
                    }
                }
                AssetCalcMessage::GetBalanceSummary {
                    wallet_address,
                    account_id,
                    chain_code,
                    response_tx,
                } => {
                    // 检查缓存是否过期，如果过期则刷新
                    self.ensure_cache_fresh().await;

                    let result = self
                        .handle_get_balance_summary(wallet_address.clone(), account_id, chain_code)
                        .await;
                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send balance summary result: {:?}", err);
                    }
                }
                AssetCalcMessage::InitAccountCache { response_tx } => {
                    let result = self.handle_init_account_cache().await;

                    // 如果初始化成功，触发缓存刷新
                    if result.is_ok() {
                        let sender_c = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) =
                                sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await
                            {
                                error!(
                                    "Failed to send ProcessBatchUpdates message after asset change: {:?}",
                                    e
                                );
                            }
                        });
                    }

                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send init account cache result: {:?}", err);
                    }
                }
                AssetCalcMessage::AddAccountToCache {
                    address,
                    account_id,
                    wallet,
                    response_tx,
                } => {
                    debug!(
                        "Received AddAccountToCache message: address={}, account_id={}, wallet={}",
                        address, account_id, wallet
                    );
                    self.handle_add_account_to_cache(&address, account_id, &wallet).await;

                    // 如果添加成功，发送批处理更新消息
                    let sender_c = sender.clone();
                    tokio::spawn(async move {
                        if let Err(e) = sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await {
                            error!(
                                "Failed to send ProcessBatchUpdates message after adding account: {:?}",
                                e
                            );
                        }
                    });

                    // 异步刷新该账户的资产
                    let address_clone = address.clone();
                    let address_for_log = address_clone.clone();
                    let pool = self.state.pool.clone();
                    let sender_c = sender.clone();
                    tokio::spawn(async move {
                        // 获取该账户的所有资产
                        match ApiAssetsRepo::get_api_assets_by_address(
                            &pool,
                            vec![address_clone],
                            None,
                        )
                        .await
                        {
                            Ok(_assets) => {
                                // 发送ProcessBatchUpdates消息以更新新添加账户的资产
                                if let Err(e) =
                                    sender_c.send(AssetCalcMessage::ProcessBatchUpdates).await
                                {
                                    error!(
                                        "Failed to send ProcessBatchUpdates for new account: {:?}",
                                        e
                                    );
                                } else {
                                    debug!(
                                        "Sent ProcessBatchUpdates for new account: {}",
                                        address_for_log
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Failed to get assets for new account: {:?}", e);
                            }
                        }
                    });

                    if let Err(err) = response_tx.send(()).await {
                        error!("Failed to send add account to cache ack: {:?}", err);
                    }
                }
                AssetCalcMessage::GetTotalUsdt { response_tx } => {
                    // 检查缓存是否过期，如果过期则刷新
                    self.ensure_cache_fresh().await;

                    let result = self.handle_get_total_usdt().await;
                    if let Err(err) = response_tx.send(result).await {
                        error!("Failed to send add account cache result: {:?}", err);
                    }
                }
                AssetCalcMessage::StartBatchRecalculator { interval_ms, response_tx } => {
                    let result = self.handle_start_batch_recalculator(interval_ms).await;
                    let _ = response_tx.send(result).await;
                }
                AssetCalcMessage::SendAffectedAccounts { assets } => {
                    self.handle_send_affected_accounts(assets).await;
                }
                AssetCalcMessage::AggregateAndNotify { assets } => {
                    if let Err(e) = self.handle_aggregate_and_notify(&assets).await {
                        error!("Failed to aggregate and notify: {:?}", e);
                    }
                }
                AssetCalcMessage::ProcessBatchUpdates => {
                    debug!("Received ProcessBatchUpdates message");
                    // 处理批处理更新
                    if let Err(e) = self.process_batch_updates().await {
                        error!("Failed to process batch updates: {:?}", e);
                    } else {
                        debug!("Batch updates processed successfully")
                    }

                    let balance = self.handle_get_wallet_balance().await.unwrap();
                    let res = wallet_utils::serde_func::toml_to_string(&balance).unwrap();
                    debug!("ProcessBatchUpdates wallet balance: {}", res);
                }
                AssetCalcMessage::RefreshAllCaches => {
                    if let Err(e) = self.refresh_all_caches().await {
                        error!("Failed to process refresh all caches: {:?}", e);
                    }
                }
            }
        }

        debug!("AssetCalcActor stopped");
    }

    // 确保缓存新鲜
    async fn ensure_cache_fresh(&self) {
        // 检查缓存是否过期
        if self.state.is_cache_expired() {
            // 异步刷新缓存

            if let Err(e) = self.sender.send(AssetCalcMessage::RefreshAllCaches).await {
                error!("Failed to send RefreshAllCaches message after ensure_cache_fresh: {:?}", e);
            }
        }
    }

    async fn handle_send_affected_accounts(&self, assets: Vec<AssetEntry>) {
        // 按账户分组处理，为每个账户只创建一个消息项
        let changed_accounts = ApiWalletSyncAssetsMsgFront::new();
        let map = self.state.address_to_account_id.clone();

        // 使用HashMap来跟踪已经处理过的账户，避免重复处理
        let mut processed_accounts = std::collections::HashMap::new();

        tracing::debug!("handle_send_affected_accounts assets: {assets:?}");

        for asset in assets {
            if let Some(account_id) = map.get(&asset.address) {
                let wallet_address = asset.wallet_address.clone();
                let account_id = *account_id;

                // 检查该账户是否已经处理过
                let key = (wallet_address.clone(), account_id);
                if processed_accounts.contains_key(&key) {
                    continue;
                }

                // 标记该账户为已处理
                processed_accounts.insert(key, ());

                // 获取该账户的总余额信息（不指定chain_code，获取所有网络的总余额）
                let balance_debug = self
                    .handle_get_balance_summary(
                        Some(wallet_address.clone()),
                        Some(account_id),
                        None, // 不指定chain_code，获取账户总余额
                    )
                    .await
                    .unwrap();

                // 创建消息项，chain_code设为空字符串，token_address设为None
                let item = ApiWalletSyncAccountBalanceMsgFrontItem::new(account_id, balance_debug);

                // 添加到变化的账户列表中
                changed_accounts.add_item(&wallet_address, item);
            }
        }

        // 只有有变化才推送
        if let Err(e) = FrontendNotifyEvent::new(NotifyEvent::ApiWalletSyncAssets(changed_accounts))
            .send()
            .await
        {
            tracing::error!("send error: {}", e);
        }
    }

    async fn handle_aggregate_and_notify(
        &mut self,
        assets: &[AssetEntry],
    ) -> Result<(), ServiceError> {
        let ctx = self.ctx;
        let currency = ConfigDomain::get_currency(ctx).await?;

        // 使用标准迭代器替代并行迭代，确保线程安全
        for a in assets {
            let asset_key =
                AssetKey::new(&a.wallet_address, &a.address, &a.chain_code, &a.token_address);

            // 检查价格是否为0或None，如果是则重新查询后端
            if let Err(e) = self
                .check_and_update_price(
                    &a.symbol,
                    &a.chain_code,
                    AssetTokenKey::from_raw(Some(a.token_address.as_str())),
                )
                .await
            {
                tracing::error!(
                    "Failed to check and update price for: symbol={}, chain_code={}, error: {:?}",
                    a.symbol,
                    a.chain_code,
                    e
                );
            }

            // 重新获取最新的价格数据
            let updated_token_currencies = self.state.token_currencies.clone();

            // 改进错误处理，避免使用unwrap_or掩盖错误
            let balance_debug = match updated_token_currencies.calculate_to_balance(
                &currency,
                &a.balance.to_string(),
                &a.symbol,
                &a.chain_code,
                Some(a.token_address.clone()),
            ) {
                Ok(balance_debug) => balance_debug,
                Err(e) => {
                    tracing::error!(
                        "Failed to calculate balance for asset: address={}, symbol={}, error: {:?}",
                        a.address,
                        a.symbol,
                        e
                    );
                    // 使用合理的默认值
                    BalanceInfo {
                        amount: 0.0,
                        currency: currency.clone(),
                        unit_price: None,
                        fiat_value: Some(0.0),
                    }
                }
            };

            // 先检查是否存在旧值，用于后续正确更新TOTAL_USDT
            // let old_fiat_value =
            //     self.state.asset_value_cache.get(&asset_key).and_then(|old| old.fiat_value).map(
            //         |v| {
            //             // 直接使用v作为浮点数进行计算
            //             Decimal::new((v * 100.0) as i64, 2)
            //         },
            //     );

            // 更新资产缓存
            self.state.asset_value_cache.insert(asset_key.clone(), balance_debug.clone());

            // 正确更新TOTAL_USDT：先减去旧值，再加上新值
            // self.update_total_usdt(old_fiat_value, balance_debug.fiat_value).await;
        }
        Ok(())
    }

    // 处理资产更新
    async fn handle_asset_update(&mut self, asset_key: &AssetKey) -> Result<(), ServiceError> {
        if asset_key.wallet_address.is_empty()
            || asset_key.address.is_empty()
            || asset_key.chain_code.is_empty()
        {
            error!(
                "Invalid input for asset update: wallet_address={}, address={}, chain_code={}",
                asset_key.wallet_address, asset_key.address, asset_key.chain_code
            );
            return Err(ServiceError::Parameter("Invalid input parameters".to_string()));
        }

        // 确保地址到钱包的映射存在
        if !self.state.address_to_wallet.contains_key(&asset_key.address) {
            self.state
                .address_to_wallet
                .insert(asset_key.address.to_string(), asset_key.wallet_address.to_string());
            debug!(
                "Added address-to-wallet mapping: {} -> {}",
                asset_key.address, asset_key.wallet_address
            );
        }

        // 标记资产为脏数据
        self.state.mark_asset_dirty(
            &asset_key.wallet_address,
            &asset_key.address,
            &asset_key.chain_code,
            &asset_key.token_address,
        );

        Ok(())
    }

    // 处理批量币价初始化
    async fn handle_batch_initialize_prices(
        &mut self,
        coins: Vec<CoinInitializationData>,
    ) -> Result<(), ServiceError> {
        if coins.is_empty() {
            return Ok(());
        }

        let ctx = self.ctx;
        let currency = ConfigDomain::get_currency(ctx).await?;
        let exchange_rates = self.get_exchange_rates().await?;
        let rate_value = exchange_rates.get(&currency).copied().unwrap_or_default();

        // 批量处理所有币价
        for coin in coins {
            // 计算法币价格
            let fiat_price = coin.price_real * rate_value;

            // 标记价格为脏
            self.state.mark_price_dirty(&coin.symbol, &coin.chain_code, &coin.token_address);

            // 更新token价格
            let id = TokenCurrencyId::new(
                &coin.symbol,
                &coin.chain_code,
                coin.token_address.to_option_string_for_api(),
            );
            self.state
                .token_currencies
                .entry(id)
                .and_modify(|entry| {
                    entry.price = Some(coin.price_real);
                    entry.currency_price = Some(fiat_price);
                    entry.rate = rate_value;
                })
                .or_insert_with(|| {
                    use wallet_transport_backend::response_vo::coin::TokenCurrency;
                    TokenCurrency::new(
                        &coin.chain_code,
                        &coin.symbol,
                        &coin.name,
                        Some(coin.price_real),
                        Some(fiat_price),
                        rate_value,
                        coin.decimals,
                    )
                });
        }

        Ok(())
    }

    // 处理价格更新
    async fn handle_price_update(
        &mut self,
        symbol: &str,
        chain_code: &str,
        name: &str,
        token_address: &AssetTokenKey,
        price_real: f64,
        decimals: u8,
    ) -> Result<(), ServiceError> {
        let ctx = self.ctx;
        let currency = ConfigDomain::get_currency(ctx).await?;

        // 更新token价格
        let (fiat_price, rate) = {
            // 获取汇率
            let exchange_rates = self.get_exchange_rates().await?;

            if let Some(rate_value) = exchange_rates.get(&currency) {
                // 使用Decimal进行精确计算
                let price_f64 = price_real * rate_value;

                // let price_decimal = Decimal::new((price_real * 100.0) as i64, 2);
                // let rate_decimal = Decimal::new((*rate_value * 100.0) as i64, 2);
                // let fiat_price_decimal = price_decimal * rate_decimal;

                // (fiat_price_decimal.to_f64(), *rate_value)
                (price_f64, *rate_value)
            } else {
                (f64::default(), f64::default())
            }
        };

        // 标记价格为脏并更新价格信息
        self.state.mark_price_dirty(symbol, chain_code, token_address);

        // 更新token价格
        let id = TokenCurrencyId::new(
            symbol,
            chain_code,
            token_address.to_option_string_for_api(),
        );
        self.state
            .token_currencies
            .entry(id)
            .and_modify(|entry| {
                entry.price = Some(price_real);
                entry.currency_price = Some(fiat_price);
                entry.rate = rate;
            })
            .or_insert_with(|| {
                use wallet_transport_backend::response_vo::coin::TokenCurrency;
                TokenCurrency::new(
                    chain_code,
                    symbol,
                    name,
                    Some(price_real),
                    Some(fiat_price),
                    rate,
                    decimals,
                )
            });

        debug!(
            "update_token_price symbol: {}, chain_code: {}, token_address: {}, price_real: {}, rate: {}",
            symbol, chain_code, token_address.as_db_str(), price_real, rate
        );

        Ok(())
    }

    // 检查价格是否为0或None，如果是则重新查询后端
    async fn check_and_update_price(
        &mut self,
        symbol: &str,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<(), ServiceError> {
        // 创建TokenCurrencyId
        let token_currency_id = TokenCurrencyId::new(
            symbol,
            chain_code,
            token_key.to_option_string_for_api(),
        );

        // 检查当前价格是否为None（区分真正的0价格和默认值0）
        let should_update =
            if let Some(token_currency) = self.state.token_currencies.get(&token_currency_id) {
                // 对于USDT，检查price字段是否为None
                // 对于其他代币，检查currency_price字段是否为None
                if symbol.eq_ignore_ascii_case("usdt") {
                    token_currency.price.is_none()
                } else {
                    token_currency.currency_price.is_none()
                }
            } else {
                true
            };

        // 如果需要更新价格
        if should_update {
            tracing::debug!(
                "Token currency not found or price is None, querying backend for: symbol={}, chain_code={}, token_address={:?}",
                symbol,
                chain_code,
                token_key.as_db_str()
            );

            // 构建查询请求
            let mut req = wallet_transport_backend::request::TokenQueryPriceReq::default();
            req.insert(chain_code, token_key.as_db_str());

            // 查询后端价格
            if let Err(e) = CoinDomain::query_token_price_with_ctx(self.ctx, &req).await {
                tracing::error!(
                    "Failed to query backend price for: symbol={}, chain_code={}, error: {:?}",
                    symbol,
                    chain_code,
                    e
                );
            }
        }

        Ok(())
    }

    // 获取汇率（带缓存）
    async fn get_exchange_rates(&mut self) -> Result<HashMap<String, f64>, ServiceError> {
        const CACHE_DURATION: Duration = Duration::from_secs(60 * 5);

        // 检查缓存是否过期
        if Instant::now() - self.state.exchange_rate_cache.last_updated < CACHE_DURATION {
            return Ok(self.state.exchange_rate_cache.rates.clone());
        }

        // 缓存过期，重新从数据库加载
        let exchange_rate_list = ExchangeRateRepo::list(self.state.pool.clone()).await?;

        // 更新缓存
        let mut rates = HashMap::new();
        for rate in exchange_rate_list {
            rates.insert(rate.target_currency.clone(), rate.rate);
        }

        self.state.exchange_rate_cache.rates = rates.clone();
        self.state.exchange_rate_cache.last_updated = Instant::now();

        Ok(rates)
    }

    // 处理获取钱包余额
    async fn handle_get_wallet_balance(
        &mut self,
    ) -> Result<HashMap<String, BalanceInfo>, ServiceError> {
        let mut wallet_totals: HashMap<String, BalanceInfo> = HashMap::new();

        // tracing::debug!(
        //     "handle_get_wallet_balance asset_value_cache: {:?}",
        //     self.state.asset_value_cache
        // );
        // tracing::debug!(
        //     "handle_get_wallet_balance address_to_wallet: {:?}",
        //     self.state.address_to_wallet
        // );

        for entry in self.state.asset_value_cache.iter() {
            if let Some(wallet_address) = self.state.address_to_wallet.get(&entry.key().address) {
                let entry_value = entry.value();
                wallet_totals
                    .entry(wallet_address.clone())
                    .and_modify(|total| {
                        total.amount_add(entry_value.amount);
                        total.fiat_add(entry_value.fiat_value);
                    })
                    .or_insert_with(|| entry_value.clone());
            }
        }

        Ok(wallet_totals)
    }

    // 处理获取账户余额
    async fn handle_get_account_balance(
        &self,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> Result<HashMap<String, BalanceInfo>, ServiceError> {
        let account_addresses: Vec<String> = self
            .state
            .address_to_wallet
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
        for entry in self.state.asset_value_cache.iter() {
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

    // 处理获取余额摘要
    async fn handle_get_balance_summary(
        &self,
        wallet_address: Option<String>,
        account_id: Option<u32>,
        chain_code: Option<String>,
    ) -> Result<BalanceInfo, ServiceError> {
        let mut total = BalanceInfo::default();

        // 根据参数筛选目标地址集合
        let mut target_addresses: Vec<String> = Vec::new();

        match (wallet_address, account_id) {
            (None, None) => {
                // 全部账户
                target_addresses = self.state.address_to_wallet.keys().cloned().collect();
            }
            (Some(wallet), None) => {
                // 指定钱包下所有账户
                target_addresses = self
                    .state
                    .address_to_wallet
                    .iter()
                    .filter_map(|(addr, w)| if w == &wallet { Some(addr.clone()) } else { None })
                    .collect();
            }
            (Some(wallet), Some(id)) => {
                // 指定钱包 + 账户
                target_addresses = self
                    .state
                    .address_to_account_id
                    .iter()
                    .filter_map(|(addr, aid)| {
                        if *aid == id
                            && self
                                .state
                                .address_to_wallet
                                .get(addr)
                                .map(|w| w == &wallet)
                                .unwrap_or(false)
                        {
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

        if target_addresses.is_empty() {
            return Ok(total);
        }

        let chain_codes = self.get_enabled_chains().await?;
        // 遍历缓存，按条件聚合
        // tracing::debug!("asset_value_cache: {:?}", self.state.asset_value_cache);
        for entry in self.state.asset_value_cache.iter() {
            // 需要过滤掉未启用的链
            // tracing::debug!("get_balance_summary ---------------- contains  ");
            if !chain_codes.contains(&entry.key().chain_code) {
                continue;
            }

            let address = &entry.key().address;
            let asset_chain_code = &entry.key().chain_code;

            // 筛选地址
            if !target_addresses.contains(address) {
                continue;
            }

            // 筛选链
            if let Some(chain_filter) = chain_code.as_ref() {
                if asset_chain_code != chain_filter {
                    continue;
                }
            }

            // tracing::debug!("累加了: {:?}, 价值: {:?}", entry.key(), entry.value());
            // 累加金额
            let entry_value = entry.value();
            total.amount_add(entry_value.amount);
            total.fiat_add(entry_value.fiat_value);
        }

        Ok(total)
    }

    // 处理初始化账户缓存
    async fn handle_init_account_cache(&mut self) -> Result<(), ServiceError> {
        let pool = self.ctx.get_global_sqlite_pool()?;
        let wallet_map =
            wallet_database::repositories::api_wallet::account::ApiAccountRepo::account_to_wallet(
                pool.clone(),
            )
            .await?;
        let account_list =
            wallet_database::repositories::api_wallet::account::ApiAccountRepo::list(pool.clone())
                .await?;

        // 更新地址到钱包的映射
        self.state.address_to_wallet.clear();
        for wallet in wallet_map {
            self.state
                .address_to_wallet
                .insert(wallet.address.clone(), wallet.wallet_address.clone());
        }

        // 更新地址到账户ID的映射
        self.state.address_to_account_id.clear();
        for row in account_list {
            self.state.address_to_account_id.insert(row.address.clone(), row.account_id);
        }

        Ok(())
    }

    // 处理添加账户到缓存
    async fn handle_add_account_to_cache(&mut self, address: &str, account_id: u32, wallet: &str) {
        debug!(
            "Adding account to cache: address={}, account_id={}, wallet={}",
            address, account_id, wallet
        );
        self.state.address_to_account_id.insert(address.to_string(), account_id);
        self.state.address_to_wallet.insert(address.to_string(), wallet.to_string());
        // 标记该账户为需要更新
        debug!("Account added to cache, marking for update")
    }

    // 处理获取总价值
    async fn handle_get_total_usdt(&self) -> Decimal {
        self.state.total_usdt
    }

    // 处理启动批处理任务
    async fn handle_start_batch_recalculator(&self, interval_ms: u64) -> Result<(), ServiceError> {
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let interval = Duration::from_millis(interval_ms);
            loop {
                tokio::time::sleep(interval).await;

                // if let Err(e) = Self::process_batch_updates(&state, &pool).await {
                //     error!("Batch update error: {:?}", e);
                // }
                if let Err(e) = sender.send(AssetCalcMessage::ProcessBatchUpdates).await {
                    error!(
                        "Failed to send ProcessBatchUpdates message after {interval_ms} secs: {:?}",
                        e
                    );
                }
            }
        });

        Ok(())
    }

    // 处理批处理更新
    async fn process_batch_updates(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 检查缓存是否过期，过期则需要刷新整个缓存
        let need_full_refresh = self.state.is_cache_expired();

        // 收集脏集合
        let price_keys: Vec<TokenCurrencyId> =
            self.state.dirty_price_set.iter().map(|k| k.clone()).collect();
        let asset_keys: Vec<AssetKey> =
            self.state.dirty_asset_set.iter().map(|id| id.clone()).collect();

        if !need_full_refresh && price_keys.is_empty() && asset_keys.is_empty() {
            debug!("No updates needed, batch process skipped");
            return Ok(());
        }

        debug!(
            "batch recalculation started: need_full_refresh={}, price_keys={}, asset_ids={}",
            need_full_refresh,
            price_keys.len(),
            asset_keys.len()
        );

        // 清除旧的脏标记
        self.state.dirty_price_set.clear();
        self.state.dirty_asset_set.clear();

        // 更新缓存时间戳
        self.state.update_cache_timestamp();
        debug!("Cache timestamp updated");

        if need_full_refresh {
            // 执行全量刷新
            debug!("Performing full cache refresh due to expiration");
            if let Err(e) = self.refresh_all_caches().await {
                error!("Full cache refresh failed: {:?}", e);
            } else {
                debug!("Full cache refresh completed successfully");
            }
        } else {
            // 处理价格变更
            if !price_keys.is_empty() {
                // debug!("Processing {} dirty price entries", price_keys.len());
                if let Err(e) = self.process_price_dirty_assets(&price_keys).await {
                    error!("process_price_dirty_assets error: {:?}", e);
                } else {
                    debug!("Dirty price entries processed successfully");
                }
            }

            // 处理资产变更
            if !asset_keys.is_empty() {
                debug!("Processing {} dirty asset entries", asset_keys.len());
                if let Err(e) = self.process_asset_dirty_assets(&asset_keys).await {
                    error!("process_asset_dirty_assets error: {:?}", e);
                } else {
                    debug!("Dirty asset entries processed successfully");
                }
            }

            // 清理过期缓存条目
            self.state.cleanup_expired_cache();
        }

        Ok(())
    }

    // 全量刷新缓存
    async fn refresh_all_caches(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 分块拉取地址资产，避免大钱包/大账号集合一次性查询后占用过多内存。
        const ADDRESS_CHUNK: usize = 200;
        let ctx = self.ctx;
        let currency = ConfigDomain::get_currency(ctx).await?;
        debug!("Starting full cache refresh");

        // 从状态中获取所有地址和pool
        let addresses: Vec<String> = self.state.address_to_wallet.keys().cloned().collect();
        debug!("Preparing to refresh cache for {} addresses", addresses.len());

        // 创建临时缓存，避免刷新期间旧缓存被清空
        let new_asset_value_cache: DashMap<AssetKey, BalanceInfo> = DashMap::new();
        let mut new_total_usdt = Decimal::ZERO;

        for chunk in addresses.chunks(ADDRESS_CHUNK) {
            // repo 接口当前接收 Vec<String>，这里按块复制以换取稳定的内存峰值。
            let chunk_addresses: Vec<String> = chunk.to_vec();
            let assets_list = ApiAssetsRepo::assets_with_wallet_address_by_address(
                &self.state.pool,
                &chunk_addresses,
            )
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

            debug!(
                "Retrieved {} assets for refresh chunk (addresses={})",
                assets_list.len(),
                chunk_addresses.len()
            );

            // 分批计算并填充临时缓存，避免一次性加载超大数据
            for asset in assets_list {
                let key = AssetKey::new(
                    &asset.wallet_address,
                    &asset.address,
                    &asset.chain_code,
                    &asset.token_address,
                );

                let token_id = TokenCurrencyId::new(
                    &asset.symbol,
                    &asset.chain_code,
                    Some(asset.token_address.clone()),
                );

                if let Err(e) = self
                    .check_and_update_price(
                        &asset.symbol,
                        &asset.chain_code,
                        AssetTokenKey::from_raw(Some(asset.token_address.as_str())),
                    )
                    .await
                {
                    tracing::error!(
                        "Failed to check and update price for: symbol={}, chain_code={}, error: {:?}",
                        asset.symbol,
                        asset.chain_code,
                        e
                    );
                }

                let updated_token_currencies = self.state.token_currencies.clone();
                let asset_value = updated_token_currencies.calculate_to_balance(
                    &currency,
                    &asset.balance,
                    &token_id.symbol,
                    &token_id.chain_code,
                    token_id.token_address,
                )?;

                new_asset_value_cache.insert(key, asset_value.clone());

                if let Some(fiat_value) = asset_value.fiat_value {
                    let price =
                        wallet_types::Decimal::from_f64_retain(fiat_value).unwrap_or_default();
                    new_total_usdt += price;
                }
            }
        }

        // 所有计算完成后，一次性替换旧缓存
        self.state.asset_value_cache = new_asset_value_cache;
        self.state.total_usdt = new_total_usdt;
        debug!("Full cache refresh completed: total_value={}", new_total_usdt);

        Ok(())
    }

    // 处理价格变更的资产
    async fn process_price_dirty_assets(
        &mut self,
        keys: &[TokenCurrencyId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        // process in chunks to avoid huge IN lists
        const CHUNK_KEYS: usize = 200;

        for chunk in keys.chunks(CHUNK_KEYS) {
            let mut keys_vec = Vec::new();
            for key in chunk {
                // 生成用于查询的键格式
                keys_vec.push(key.gen_key());
            }

            // 使用正确的Repository方法获取资产列表
            let assets_list = match ApiAssetsRepo::assets_with_wallet_address_by_token(
                &self.state.pool,
                &keys_vec,
            )
            .await
            {
                Ok(list) => list,
                Err(e) => {
                    warn!("Failed to get assets by token: {:?}", e);
                    Vec::new()
                }
            };

            let mut assets = Vec::new();
            for asset in assets_list {
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

            // debug!("Processing price update for {} assets", assets.len());

            // let actor_manager = crate::context::CONTEXT
            //     .get()
            //     .unwrap()
            //     .get_global_asset_calc_actor_manager()
            //     .await?;

            // 发送AggregateAndNotify消息
            // if let Err(e) = actor_manager
            //     .sender
            //     .send(AssetCalcMessage::AggregateAndNotify { assets: assets.clone() })
            //     .await
            // {
            //     warn!("Failed to send AggregateAndNotify message: {:?}", e);
            // }

            // 币价变动不需要发送SendAffectedAccounts消息
            // if let Err(e) =
            //     actor_manager.sender.send(AssetCalcMessage::SendAffectedAccounts { assets }).await
            // {
            //     warn!("Failed to send SendAffectedAccounts message: {:?}", e);
            // }
        }

        Ok(())
    }

    // 处理资产变更
    async fn process_asset_dirty_assets(
        &mut self,
        keys: &[AssetKey],
    ) -> Result<(), Box<dyn std::error::Error>> {
        const CHUNK_SIZE: usize = 200;

        for chunk in keys.chunks(CHUNK_SIZE) {
            let mut keys_vec = Vec::new();
            for key in chunk {
                keys_vec.push(format!("{}:{}:{}", key.address, key.chain_code, key.token_address));
            }

            // 尝试获取受影响的资产
            let assets_list = match ApiAssetsRepo::assets_with_wallet_address_by_address(
                &self.state.pool,
                &keys_vec,
            )
            .await
            {
                Ok(list) => list,
                Err(e) => {
                    warn!("Failed to get assets by address: {:?}", e);
                    Vec::new()
                }
            };

            let mut assets = Vec::new();
            for asset in assets_list {
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

            debug!("Processing asset update for {} assets", assets.len());

            // 使用AssetCalcActorManager的sender发送消息
            // let actor_manager = crate::context::CONTEXT
            //     .get()
            //     .unwrap()
            //     .get_global_asset_calc_actor_manager()
            //     .await?;

            // // 发送AggregateAndNotify消息
            // if let Err(e) = actor_manager
            //     .sender
            //     .send(AssetCalcMessage::AggregateAndNotify { assets: assets.clone() })
            //     .await
            // {
            //     warn!("Failed to send AggregateAndNotify message: {:?}", e);
            // }

            // // 发送SendAffectedAccounts消息
            // if let Err(e) =
            //     actor_manager.sender.send(AssetCalcMessage::SendAffectedAccounts { assets }).await
            // {
            //     warn!("Failed to send SendAffectedAccounts message: {:?}", e);
            // }
        }

        Ok(())
    }
}

// AssetCalcActor的管理器
#[derive(Debug)]
pub struct AssetCalcActorManager {
    sender: mpsc::Sender<AssetCalcMessage>,
}

impl AssetCalcActorManager {
    // 创建并启动新的Actor
    pub fn start(ctx: &'static crate::context::Context, pool: Arc<SqlitePool>) -> Self {
        let (sender, receiver) = mpsc::channel(100);

        // 启动Actor
        let actor = AssetCalcActor::new(ctx, pool, sender.clone(), receiver);
        tokio::spawn(actor.run());

        Self { sender }
    }

    /// 批量更新资产，优化发送频率
    pub async fn update_assets(
        &self,
        updates: &[AssetKey], // (wallet_address, address, chain_code, token_address)
    ) -> Result<(), ServiceError> {
        if updates.is_empty() {
            return Ok(());
        }

        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::BatchAssetUpdate { updates: updates.to_vec(), response_tx };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：价格更新
    pub async fn update_price(
        &self,
        symbol: &str,
        chain_code: &str,
        name: &str,
        token_key: AssetTokenKey,
        price_real: f64,
        decimals: u8,
    ) -> Result<(), ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::PriceUpdate {
            symbol: symbol.to_string(),
            name: name.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_key,
            price_real,
            decimals,
            response_tx,
        };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：批量初始化币价
    pub async fn batch_initialize_prices(
        &self,
        coins: Vec<CoinInitializationData>,
    ) -> Result<(), ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::BatchInitializePrices { coins, response_tx };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：获取钱包余额
    pub async fn get_wallet_balance(&self) -> Result<HashMap<String, BalanceInfo>, ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::GetWalletBalance { response_tx };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：获取账户余额
    pub async fn get_account_balance(
        &self,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> Result<HashMap<String, BalanceInfo>, ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::GetAccountBalance {
            wallet_address: wallet_address.to_string(),
            chain_code,
            response_tx,
        };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：获取余额摘要
    pub async fn get_balance_summary(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<BalanceInfo, ServiceError> {
        // tracing::debug!("get_balance_summary ---------------- 1   -------  Getting balance summary");
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::GetBalanceSummary {
            wallet_address: wallet_address.map(|s| s.to_string()),
            account_id,
            chain_code: chain_code.map(|s| s.to_string()),
            response_tx,
        };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：初始化账户缓存
    pub async fn init_account_cache(&self) -> Result<(), ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::InitAccountCache { response_tx };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }

    // 异步接口：添加账户到缓存
    pub async fn add_account_to_cache(&self, address: &str, account_id: u32, wallet: &str) {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::AddAccountToCache {
            address: address.to_string(),
            account_id,
            wallet: wallet.to_string(),
            response_tx,
        };

        if let Err(e) = self.sender.send(msg).await {
            error!("Failed to send message: {}", e);
        }

        // 忽略响应
                    if response_rx.recv().await.is_none() {
                        error!("No response received for add_account_to_cache");
                    }
    }

    // 异步接口：获取总价值
    pub async fn get_total_usdt(&self) -> Decimal {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::GetTotalUsdt { response_tx };

        if let Err(e) = self.sender.send(msg).await {
            error!("Failed to send message: {}", e);
            return Decimal::ZERO;
        }

        response_rx.recv().await.unwrap_or(Decimal::ZERO)
    }

    // 异步接口：启动批处理任务
    pub async fn start_batch_recalculator(&self, interval_ms: u64) -> Result<(), ServiceError> {
        let (response_tx, mut response_rx) = mpsc::channel(1);

        let msg = AssetCalcMessage::StartBatchRecalculator { interval_ms, response_tx };

        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(SystemError::Service(format!("Failed to send message: {}", e)))
        })?;

        response_rx.recv().await.unwrap_or(Err(ServiceError::System(SystemError::Service(
            "No response received".to_string(),
        ))))
    }
}
