use crate::{domain::api_wallet::trans::ApiTransDomain, error::service::ServiceError};
use dashmap::{DashMap, DashSet};
use futures::FutureExt;
use once_cell::sync::OnceCell;
use rand;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    time,
};
use tracing::{error, info, warn};

/// Nonce 错误类型分类
#[derive(Debug, PartialEq, Eq)]
pub enum NonceErrorKind {
    /// Nonce 过低
    NonceTooLow,
    /// Nonce 过高
    NonceTooHigh,
    /// Nonce 已使用
    NonceAlreadyUsed,
    /// Node has already seen this transaction; this does not prove inclusion.
    AlreadyKnown,
    /// Replacement fee does not meet the node's bump requirement.
    ReplacementUnderpriced,
    /// Transaction fee is below the node's admission threshold.
    TransactionUnderpriced,
    /// 链上数据不一致
    ChainDataInconsistent,
    /// 其他 Nonce 相关错误
    Other(String),
}

/// Reconcile 触发原因
#[derive(Debug, PartialEq, Eq)]
pub enum ReconcileReason {
    /// 手动触发
    Manual,
    /// Nonce 错误触发
    NonceError,
    /// 交易发送失败触发
    TransactionFailed,
    /// 系统启动触发
    SystemStartup,
    /// 定期检查触发
    PeriodicCheck,
    /// 其他原因
    Other(String),
}

/// 冻结原因
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FreezeReason {
    /// 手动冻结
    Manual,
    /// Nonce 过低错误
    NonceTooLow,
    /// Nonce 过高错误
    NonceTooHigh,
    /// Nonce 已使用错误
    NonceAlreadyUsed,
    /// 链上数据不一致
    ChainDataInconsistent,
    /// 交易发送失败
    TransactionFailed,
    /// 系统维护
    SystemMaintenance,
    /// 其他原因
    Other(String),
}

/// 链错误归一化器，将不同链的错误消息转换为统一的 NonceErrorKind
pub struct ChainErrorNormalizer;

impl ChainErrorNormalizer {
    /// 将链错误消息归一化为 NonceErrorKind
    pub fn normalize(error: &str) -> NonceErrorKind {
        let error_lower = error.to_lowercase();

        if error_lower.contains("replacement transaction underpriced") {
            return NonceErrorKind::ReplacementUnderpriced;
        }
        if error_lower.contains("transaction underpriced") {
            return NonceErrorKind::TransactionUnderpriced;
        }

        // Match complete words so "unknown transaction" is not a duplicate.
        let words: Vec<_> = error_lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        if words
            .windows(2)
            .any(|pair| pair == ["known", "transaction"] || pair == ["already", "known"])
        {
            return NonceErrorKind::AlreadyKnown;
        }

        // 检测 nonce too low 相关错误
        if error_lower.contains("nonce too low")
            || error_lower.contains("nonce is too low")
            || error_lower.contains("invalid nonce: too low")
        {
            return NonceErrorKind::NonceTooLow;
        }

        // 检测 nonce too high 相关错误
        if error_lower.contains("nonce too high")
            || error_lower.contains("nonce is too high")
            || error_lower.contains("invalid nonce: too high")
        {
            return NonceErrorKind::NonceTooHigh;
        }

        // 检测 nonce already used 相关错误
        if error_lower.contains("nonce already used")
            || error_lower.contains("nonce has already been used")
            || error_lower.contains("invalid nonce: already used")
        {
            return NonceErrorKind::NonceAlreadyUsed;
        }

        // 检测链上数据不一致错误
        if error_lower.contains("chain data inconsistent")
            || error_lower.contains("state mismatch")
            || error_lower.contains("blockchain state error")
        {
            return NonceErrorKind::ChainDataInconsistent;
        }

        // 其他错误
        NonceErrorKind::Other(error.to_string())
    }
}

/// NonceEngine核心引擎
/// Candidate observation never preallocates a nonce.
/// ```compile_fail
/// use wallet_api::infrastructure::nonce::nonce_engine::NonceEngine;
/// let _ = NonceEngine::allocate_nonce;
/// ```
/// ```compile_fail
/// use wallet_api::infrastructure::nonce::nonce_engine::NonceEngine;
/// let _ = NonceEngine::allocate_nonce_synced;
/// ```
pub struct NonceEngine {
    /// 冻结的地址集合，使用多值集合存储不同的冻结原因
    frozen_addresses: DashMap<(String, String), HashSet<FreezeReason>>,
    /// 待处理的reconcile任务
    reconcile_tx: mpsc::UnboundedSender<(String, String, ReconcileReason)>,
    /// 正在处理的reconcile任务
    inflight_reconcile: DashSet<(String, String)>,
    /// 最后一次reconcile的时间，用于节流
    last_reconcile: DashMap<(String, String), std::time::Instant>,
    /// Worker 启动状态，确保只启动一次
    worker_started: AtomicBool,
    ctx: &'static crate::context::Context,
}

/// InflightGuard 确保在任何情况下都能自动移除 inflight 记录
struct InflightGuard {
    engine: Arc<NonceEngine>,
    key: (String, String),
}

impl InflightGuard {
    /// 从已存在的 key 创建 guard，只负责 drop 不负责 insert
    fn from_existing_key(engine: Arc<NonceEngine>, address: &str, chain: &str) -> Self {
        let key = (
            wallet_database::entities::api_nonce::nonce_address_key(address, chain),
            chain.to_string(),
        );
        Self { engine, key }
    }
}

/// FreezeGuard 确保在任何情况下都能自动解冻地址
struct FreezeGuard {
    engine: Arc<NonceEngine>,
    address: String,
    chain: String,
    freeze_reason: FreezeReason,
    freeze_id: u64,
}

impl FreezeGuard {
    fn new(
        engine: Arc<NonceEngine>,
        address: &str,
        chain: &str,
        reason: FreezeReason,
    ) -> Option<Self> {
        let address = wallet_database::entities::api_nonce::nonce_address_key(address, chain);
        let chain = chain.to_string();
        let key = (address.clone(), chain.clone());
        let freeze_id = rand::random();

        // 添加冻结原因到多值集合
        let mut added = false;
        engine
            .frozen_addresses
            .entry(key.clone())
            .and_modify(|reasons| {
                if reasons.insert(reason.clone()) {
                    added = true;
                }
            })
            .or_insert_with(|| {
                let mut reasons = HashSet::new();
                reasons.insert(reason.clone());
                added = true;
                reasons
            });

        // 如果已经存在相同的冻结原因，返回 None
        if !added {
            return None;
        }

        info!(address = %address, chain = %chain, freeze_id = %freeze_id, source = "nonce_engine", reason = ?reason, "Address frozen with reason");
        Some(Self { engine, address, chain, freeze_reason: reason, freeze_id })
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        // 确保在任何情况下都能移除 inflight 记录
        self.engine.inflight_reconcile.remove(&self.key);
    }
}

impl Drop for FreezeGuard {
    fn drop(&mut self) {
        // 确保在任何情况下都能移除对应的冻结原因
        let key = (self.address.clone(), self.chain.clone());
        if let Some(mut entry) = self.engine.frozen_addresses.get_mut(&key) {
            entry.remove(&self.freeze_reason);
            // 如果没有更多冻结原因，移除整个条目
            if entry.is_empty() {
                drop(entry);
                self.engine.frozen_addresses.remove(&key);
            }
        }
        info!(address = %self.address, chain = %self.chain, freeze_id = %self.freeze_id, source = "nonce_engine", reason = ?self.freeze_reason, "Address unfrozen automatically");
    }
}

impl NonceEngine {
    pub fn new(ctx: &'static crate::context::Context) -> Arc<Self> {
        let (reconcile_tx, reconcile_rx) = mpsc::unbounded_channel();

        let engine = Arc::new(Self {
            frozen_addresses: DashMap::new(),
            reconcile_tx,
            inflight_reconcile: DashSet::new(),
            last_reconcile: DashMap::new(),
            worker_started: AtomicBool::new(false),
            ctx,
        });

        // 使用 AtomicBool 确保 worker 只启动一次
        if engine
            .worker_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // 启动reconcile任务处理器，直接传递Arc<Self>引用
            let worker_engine = engine.clone();
            tokio::spawn(async move {
                Self::handle_reconcile_tasks(worker_engine, reconcile_rx).await;
            });

            // 启动TTL清理worker
            let ttl_engine = engine.clone();
            tokio::spawn(async move {
                Self::run_ttl_cleanup(ttl_engine).await;
            });
        }

        engine
    }

    /// Observe a candidate without reserving it. The signed intent repository
    /// rechecks and reserves atomically before the caller may broadcast.
    pub(crate) async fn observe_ordinary_transfer_nonce(
        &self,
        address: &str,
        chain: &str,
    ) -> Result<i32, ServiceError> {
        if self.is_frozen(address, chain) {
            return Err(ServiceError::Parameter(format!(
                "Address {address} on chain {chain} is frozen"
            )));
        }
        let next = ApiTransDomain::nonce(self.ctx, address, chain).await?;
        let next = i32::try_from(next).map_err(|_| {
            ServiceError::Parameter("chain nonce exceeds supported allocation range".into())
        })?;
        let pool = self.ctx.api_transaction_pool()?;
        let last =
            wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::get_api_nonce_optional(
                &pool, address, chain,
            )
            .await
            .map_err(ServiceError::Database)?
            .unwrap_or(-1);
        info!(
            address,
            chain,
            chain_next = next,
            local_last = last,
            "Ordinary transfer nonce candidate observed"
        );
        let decision =
            wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::observe_signed_nonce(
                &pool,
                address,
                chain,
                i64::from(next),
            )
            .await?;
        if last >= i64::from(next)
            || decision
                != wallet_database::entities::api_nonce_claim::WithdrawNonceDecision::Ready(
                    i64::from(next),
                )
        {
            return Err(ServiceError::Parameter(format!(
                "nonce_gap_requires_recovery: address={address} chain={chain} chain_next={next} local_last={last}"
            )));
        }
        Ok(next)
    }

    /// Shared collect/fee candidate, retaining the observed chain frontier.
    pub(crate) async fn observe_signed_nonce_candidate(
        &self,
        address: &str,
        chain: &str,
    ) -> Result<(u64, i64), ServiceError> {
        if self.is_frozen(address, chain) {
            return Err(ServiceError::Parameter(format!(
                "Address {address} on chain {chain} is frozen"
            )));
        }
        let chain_next = ApiTransDomain::nonce(self.ctx, address, chain).await?;
        let chain_next = i64::try_from(chain_next).map_err(|_| {
            ServiceError::Parameter("chain nonce exceeds supported allocation range".into())
        })?;
        let decision =
            wallet_database::repositories::api_wallet::nonce::ApiNonceRepo::observe_signed_nonce(
                &self.ctx.api_transaction_pool()?,
                address,
                chain,
                chain_next,
            )
            .await?;
        match decision {
            wallet_database::entities::api_nonce_claim::WithdrawNonceDecision::Ready(nonce) => {
                let nonce = i32::try_from(nonce).map_err(|_| {
                    ServiceError::Parameter("chain nonce exceeds supported allocation range".into())
                })?;
                info!(
                    address,
                    chain,
                    chain_next,
                    nonce,
                    "Signed nonce candidate observed from durable ownership"
                );
                Ok((nonce as u64, chain_next))
            }
            _ => Err(ServiceError::Parameter(format!(
                "nonce_gap_requires_recovery: address={address} chain={chain} chain_next={chain_next}"
            ))),
        }
    }

    /// 获取地址级 transfer gate，确保同地址转账串行执行
    pub async fn acquire_transfer_gate(&self, address: &str, chain: &str) -> OwnedSemaphorePermit {
        self.ctx.transfer_gates.acquire(address, chain).await
    }

    /// 触发reconcile
    pub fn trigger_reconcile(&self, address: &str, chain: &str) {
        self.trigger_reconcile_with_reason(address, chain, ReconcileReason::Manual, false)
    }

    /// 触发reconcile（支持强制绕过节流）
    pub fn trigger_reconcile_with_force(&self, address: &str, chain: &str, force: bool) {
        self.trigger_reconcile_with_reason(address, chain, ReconcileReason::Manual, force)
    }

    /// 触发reconcile（支持指定原因和强制绕过节流）
    pub fn trigger_reconcile_with_reason(
        &self,
        address: &str,
        chain: &str,
        reason: ReconcileReason,
        force: bool,
    ) {
        let key = (
            wallet_database::entities::api_nonce::nonce_address_key(address, chain),
            chain.to_string(),
        );

        // 检查是否已经在处理中，避免 reconcile storm
        if !self.inflight_reconcile.insert(key.clone()) {
            info!(address = %address, chain = %chain, reason = ?reason, source = "nonce_engine", "Reconcile already in flight, skipping");
            return;
        }

        // 检查是否在节流期内，避免短时间内重复触发（force=true 时绕开）
        let throttle_duration = std::time::Duration::from_secs(30); // 30秒节流
        if !force {
            if let Some(timestamp_ref) = self.last_reconcile.get(&key) {
                let timestamp = timestamp_ref.value();
                if std::time::Instant::now().duration_since(*timestamp) < throttle_duration {
                    info!(address = %address, chain = %chain, reason = ?reason, source = "nonce_engine", "Reconcile throttled, skipping");
                    self.inflight_reconcile.remove(&key);
                    return;
                }
            }
        } else {
            info!(address = %address, chain = %chain, reason = ?reason, source = "nonce_engine", "Forced reconcile, bypassing throttle");
        }

        if let Err(e) = self.reconcile_tx.send((address.to_string(), chain.to_string(), reason)) {
            // 如果发送失败，需要移除 inflight 记录
            self.inflight_reconcile.remove(&key);
            warn!(error = %e, source = "nonce_engine", "Failed to send reconcile task");
        }
    }

    /// 处理reconcile任务
    async fn handle_reconcile_tasks(
        engine: Arc<Self>,
        mut rx: mpsc::UnboundedReceiver<(String, String, ReconcileReason)>,
    ) {
        while let Some((address, chain, reason)) = rx.recv().await {
            // 创建 inflight guard（key 已经在 trigger 时插入）
            let guard = InflightGuard::from_existing_key(engine.clone(), &address, &chain);

            info!(address = %address, chain = %chain, reason = ?reason, source = "nonce_engine", "Starting reconcile task");

            // 使用catch_unwind保护，防止panic导致业务逻辑异常
            let ctx = engine.ctx;
            let result = std::panic::AssertUnwindSafe(async {
                Self::reconcile_address(engine.clone(), &address, &chain).await
            })
            .catch_unwind()
            .await;

            // guard 会在 drop 时自动移除 inflight 记录
            drop(guard);

            if let Err(e) = result {
                error!(address = %address, chain = %chain, reason = ?reason, error = ?e, source = "nonce_engine", "Reconcile panicked");
            } else if let Err(e) = result.unwrap() {
                error!(address = %address, chain = %chain, reason = ?reason, error = %e, source = "nonce_engine", "Reconcile failed");
            } else {
                info!(address = %address, chain = %chain, reason = ?reason, source = "nonce_engine", "Reconcile completed successfully");
            }
        }
    }

    /// Reconcile the allocation floor without releasing existing reservations.
    ///
    /// The legacy name is retained for callers. A pending count cannot prove
    /// that signed or in-flight transactions are safe to discard. Gaps require
    /// transaction-aware recovery; never repair them by rewinding this counter.
    pub async fn force_align_to_chain_next_nonce(
        &self,
        address: &str,
        chain: &str,
        reason: ReconcileReason,
    ) -> Result<u64, ServiceError> {
        if self.is_frozen(address, chain) {
            return Err(ServiceError::Parameter(format!(
                "Address {} on chain {} is frozen",
                address, chain
            )));
        }

        // 从链上获取 next nonce（pending 语义）
        let chain_next = ApiTransDomain::nonce(self.ctx, address, chain).await?;
        let chain_next_i64 = i64::try_from(chain_next).map_err(|_| {
            ServiceError::Parameter(format!("chain_next out of range: {}", chain_next))
        })?;
        let target_last = chain_next_i64.saturating_sub(1);

        use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
        let pool = self.ctx.api_transaction_pool()?;
        // Atomic MAX is essential: a read followed by an exact write could
        // erase a reservation made while the RPC query was in flight.
        let local_last = ApiNonceRepo::set_nonce_floor(&pool, address, chain, target_last)
            .await
            .map_err(ServiceError::Database)?;
        if local_last > target_last {
            warn!(
                observability_event = "nonce_gap_requires_recovery",
                address,
                chain,
                chain_next,
                local_last,
                source = "nonce_engine",
                "Preserve local nonce reservations; transaction-aware gap recovery required"
            );
            return Err(ServiceError::Parameter("nonce_gap_requires_recovery".into()));
        }

        let key = (
            wallet_database::entities::api_nonce::nonce_address_key(address, chain),
            chain.to_string(),
        );
        self.last_reconcile.insert(key, std::time::Instant::now());

        info!(
            address = %address,
            chain = %chain,
            chain_next = %chain_next,
            target_last = %target_last,
            local_last,
            reason = ?reason,
            source = "nonce_engine",
            "NonceEngine alignment advanced allocation floor without releasing reservations"
        );

        Ok(chain_next)
    }

    /// reconcile单个地址
    async fn reconcile_address(
        engine: Arc<NonceEngine>,
        address: &str,
        chain: &str,
    ) -> Result<(), ServiceError> {
        // ⚠️ 冻结期间仍然允许 reconcile：
        // freeze 用于阻止业务继续 allocate/发送交易；
        // reconcile 用于纠正 DB nonce 与链上 nonce 漂移，必须允许执行。
        if engine.is_frozen(address, chain) {
            info!(address = %address, chain = %chain, source = "nonce_engine", "Address is frozen, but continuing reconcile");
        }

        // 从链上获取 next nonce（pending 语义）
        let chain_next = ApiTransDomain::nonce(engine.ctx, address, chain).await?;
        info!(address = %address, chain = %chain, chain_next = %chain_next, source = "nonce_engine", "Got chain nonce for reconcile");

        // 获取数据库中的 nonce
        use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
        let pool = engine.ctx.api_transaction_pool()?;
        let db_nonce = ApiNonceRepo::get_api_nonce_optional(&pool, address, chain)
            .await
            .map_err(ServiceError::Database)?;
        info!(address = %address, chain = %chain, db_nonce = ?db_nonce, source = "nonce_engine", "Got db nonce for reconcile");

        // DB 存的是 last_used，链上返回的是 next_to_use：需要追平到 (chain_next - 1)
        let chain_next_i64 = i64::try_from(chain_next).map_err(|_| {
            ServiceError::Parameter(format!("chain_next out of range: {}", chain_next))
        })?;
        let target_last = chain_next_i64.saturating_sub(1);

        // 只有当 DB 落后链上时才追平：chain_next > (db_last + 1)
        let db_last = db_nonce.unwrap_or(i64::MIN);
        if chain_next_i64 > db_last.saturating_add(1) {
            info!(
                address = %address,
                chain = %chain,
                db_last = %db_last,
                chain_next = %chain_next_i64,
                target_last = %target_last,
                source = "nonce_engine",
                "Nonce drift detected (db behind chain), syncing floor"
            );
            let _ = ApiNonceRepo::set_nonce_floor(&pool, address, chain, target_last)
                .await
                .map_err(ServiceError::Database)?;
        }

        // 更新最后一次reconcile的时间
        let key = (
            wallet_database::entities::api_nonce::nonce_address_key(address, chain),
            chain.to_string(),
        );
        engine.last_reconcile.insert(key, std::time::Instant::now());

        Ok(())
    }

    /// 冻结地址
    pub fn freeze(&self, address: &str, chain: &str, reason: FreezeReason) {
        // 空字符串检查
        if address.trim().is_empty() || chain.trim().is_empty() {
            error!(address = %address, chain = %chain, source = "nonce_engine", "Empty address or chain provided for freeze");
            return;
        }

        let address = wallet_database::entities::api_nonce::nonce_address_key(address, chain);
        let chain = chain.to_string();
        let key = (address.clone(), chain.clone());

        // 添加冻结原因到多值集合
        let added = self
            .frozen_addresses
            .entry(key)
            .and_modify(|reasons| {
                reasons.insert(reason.clone());
            })
            .or_insert_with(|| {
                let mut reasons = HashSet::new();
                reasons.insert(reason.clone());
                reasons
            });

        info!(address = %address, chain = %chain, source = "nonce_engine", reason = ?reason, "Address frozen with reason");
    }

    /// 解冻地址
    pub fn unfreeze(&self, address: &str, chain: &str, reason: FreezeReason) {
        // 空字符串检查
        if address.trim().is_empty() || chain.trim().is_empty() {
            error!(address = %address, chain = %chain, source = "nonce_engine", "Empty address or chain provided for unfreeze");
            return;
        }

        let address = wallet_database::entities::api_nonce::nonce_address_key(address, chain);
        let chain = chain.to_string();
        let key = (address.clone(), chain.clone());

        // 移除特定的冻结原因
        if let Some(mut entry) = self.frozen_addresses.get_mut(&key) {
            entry.remove(&reason);
            // 如果没有更多冻结原因，移除整个条目
            if entry.is_empty() {
                drop(entry);
                self.frozen_addresses.remove(&key);
            }
        }

        info!(address = %address, chain = %chain, source = "nonce_engine", reason = ?reason, "Address unfrozen for specific reason");
    }

    /// 检查地址是否被冻结
    pub fn is_frozen(&self, address: &str, chain: &str) -> bool {
        let address = wallet_database::entities::api_nonce::nonce_address_key(address, chain);
        let chain = chain.to_string();
        let key = (address, chain);

        self.frozen_addresses.contains_key(&key)
    }

    /// 运行TTL清理worker，定期清理过期的last_reconcile记录
    async fn run_ttl_cleanup(engine: Arc<NonceEngine>) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60 * 5));

        loop {
            interval.tick().await;

            let now = std::time::Instant::now();
            let ttl = std::time::Duration::from_secs(60 * 60 * 24); // 24小时TTL

            // 清理过期的记录
            engine.last_reconcile.retain(|_, timestamp| now.duration_since(*timestamp) < ttl);

            info!(
                source = "nonce_engine",
                "TTL cleanup completed, removed expired reconcile records"
            );
        }
    }

    /// 稳定分页扫描所有 nonce 记录，用于系统级 reconcile
    pub async fn stable_paginate_scan(&self, page_size: i32) -> Result<(), ServiceError> {
        use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;

        let pool = self.ctx.api_transaction_pool()?;
        let mut cursor: Option<(String, String)> = None;
        let mut processed = 0;

        loop {
            // 获取当前页的数据
            let cursor_ref = cursor.as_ref().map(|(addr, chain)| (addr.as_str(), chain.as_str()));
            let records =
                ApiNonceRepo::get_all_api_nonce_paginated(&pool, cursor_ref, page_size).await?;

            if records.is_empty() {
                // 扫描完成
                break;
            }

            // 处理当前页的记录
            for (addr, chain, _nonce) in records.iter() {
                // 触发 reconcile
                self.trigger_reconcile_with_reason(
                    addr,
                    chain,
                    ReconcileReason::PeriodicCheck,
                    false,
                );
                processed += 1;
            }

            // 更新 cursor 为当前页的最后一条记录
            let last_record = records.last().unwrap();
            cursor = Some((last_record.0.clone(), last_record.1.clone()));

            info!(
                source = "nonce_engine",
                processed = processed,
                page_size = page_size,
                "Stable pagination scan: processed page"
            );

            // 添加 page sleep 避免数据库风暴
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!(source = "nonce_engine", processed = processed, "Stable pagination scan completed");
        Ok(())
    }

    /// 处理nonce错误
    pub async fn handle_nonce_error(
        self: &Arc<Self>,
        address: &str,
        chain: &str,
        error: &str,
    ) -> Result<(), ServiceError> {
        let error_kind = ChainErrorNormalizer::normalize(error);

        match error_kind {
            NonceErrorKind::NonceTooLow => {
                // 处理nonce too low错误
                self.handle_nonce_too_low(address, chain).await
            }
            NonceErrorKind::NonceTooHigh => {
                // 处理nonce too high错误
                self.handle_nonce_too_high(address, chain).await
            }
            NonceErrorKind::NonceAlreadyUsed => {
                // 处理nonce already used错误，与nonce too low类似
                self.handle_nonce_too_low(address, chain).await
            }
            NonceErrorKind::ChainDataInconsistent => {
                // 处理链上数据不一致错误，触发reconcile
                self.trigger_reconcile_with_reason(
                    address,
                    chain,
                    ReconcileReason::NonceError,
                    false,
                );
                Ok(())
            }
            // Visibility is not nonce consumption; fee rejection cannot repair a nonce.
            NonceErrorKind::AlreadyKnown => Ok(()),
            NonceErrorKind::ReplacementUnderpriced
            | NonceErrorKind::TransactionUnderpriced
            | NonceErrorKind::Other(_) => Err(ServiceError::Parameter(error.to_string())),
        }
    }

    /// 处理nonce too low错误
    async fn handle_nonce_too_low(
        self: &Arc<Self>,
        address: &str,
        chain: &str,
    ) -> Result<(), ServiceError> {
        // 空字符串检查
        if address.trim().is_empty() || chain.trim().is_empty() {
            error!(address = %address, chain = %chain, source = "nonce_engine", "Empty address or chain provided for freeze");
            return Ok(());
        }

        // 使用 FreezeGuard 确保在任何情况下都能解冻
        let engine = Arc::clone(self);
        let _guard = match FreezeGuard::new(
            engine.clone(),
            address,
            chain,
            FreezeReason::NonceTooLow,
        ) {
            Some(guard) => guard,
            None => {
                info!(address = %address, chain = %chain, source = "nonce_engine", "Address already frozen with NonceTooLow reason, skipping");
                return Ok(());
            }
        };

        let result = async {
            let pool = self.ctx.api_transaction_pool()?;
            // 从链上获取 next nonce（pending 语义）
            let chain_next = ApiTransDomain::nonce(self.ctx, address, chain).await?;
            info!(address = %address, chain = %chain, chain_next = %chain_next, source = "nonce_engine", "Got chain nonce for repair");

            // DB 存的是 last_used：追平到 (chain_next - 1)
            use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
            let chain_next_i64 = i64::try_from(chain_next)
                .map_err(|_| ServiceError::Parameter(format!("chain_next out of range: {}", chain_next)))?;
            let target_last = chain_next_i64.saturating_sub(1);
            let nonce = ApiNonceRepo::set_nonce_floor(&pool, address, chain, target_last)
                .await
                .map_err(ServiceError::Database)?;
            info!(address = %address, chain = %chain, nonce = %nonce, target_last = %target_last, source = "nonce_engine", "Nonce floor synced successfully");

            // 触发reconcile作为兜底
            engine.trigger_reconcile_with_reason(address, chain, ReconcileReason::NonceError, false);

            Ok::<_, ServiceError>(())
        }.await;

        result
    }

    /// 处理nonce too high错误
    async fn handle_nonce_too_high(
        self: &Arc<Self>,
        address: &str,
        chain: &str,
    ) -> Result<(), ServiceError> {
        // 空字符串检查
        if address.trim().is_empty() || chain.trim().is_empty() {
            error!(address = %address, chain = %chain, source = "nonce_engine", "Empty address or chain provided for freeze");
            return Ok(());
        }

        // 使用 FreezeGuard 确保在任何情况下都能解冻
        let engine = Arc::clone(self);
        let _guard = match FreezeGuard::new(
            engine.clone(),
            address,
            chain,
            FreezeReason::NonceTooHigh,
        ) {
            Some(guard) => guard,
            None => {
                info!(address = %address, chain = %chain, source = "nonce_engine", "Address already frozen with NonceTooHigh reason, skipping");
                return Ok(());
            }
        };

        let result = async {
            let pool = self.ctx.api_transaction_pool()?;
            // 等待一段时间让链上交易确认
            time::sleep(Duration::from_secs(5)).await;

            // 从链上获取 next nonce（pending 语义）
            let chain_next = ApiTransDomain::nonce(self.ctx, address, chain).await?;
            info!(address = %address, chain = %chain, chain_next = %chain_next, source = "nonce_engine", "Got chain nonce for repair");

            // DB 不允许回滚：只在落后时追平到 (chain_next - 1)
            use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
            let chain_next_i64 = i64::try_from(chain_next)
                .map_err(|_| ServiceError::Parameter(format!("chain_next out of range: {}", chain_next)))?;
            let target_last = chain_next_i64.saturating_sub(1);
            let nonce = ApiNonceRepo::set_nonce_floor(&pool, address, chain, target_last)
                .await
                .map_err(ServiceError::Database)?;
            info!(address = %address, chain = %chain, nonce = %nonce, target_last = %target_last, source = "nonce_engine", "Nonce floor synced successfully");

            // 触发reconcile作为兜底
            engine.trigger_reconcile_with_reason(address, chain, ReconcileReason::NonceError, false);

            Ok::<_, ServiceError>(())
        }.await;

        result
    }
}

static NONCE_ENGINE: OnceCell<Arc<NonceEngine>> = OnceCell::new();

pub fn get_nonce_engine_with_ctx(
    ctx: &'static crate::context::Context,
) -> Result<Arc<NonceEngine>, ServiceError> {
    ctx.api_transaction_pool()?;
    NONCE_ENGINE.get_or_try_init(|| Ok::<_, ServiceError>(NonceEngine::new(ctx))).cloned()
}

#[cfg(test)]
#[path = "nonce_error_tests.rs"]
mod tests;
