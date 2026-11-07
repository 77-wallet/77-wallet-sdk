use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use crate::{
    context::Context,
    domain::{
        api_wallet::trans::{ApiTransDomain, shared_mechanism},
        chain::adapter::ChainAdapterFactory,
    },
    error::{service::ServiceError, system::SystemError},
    infrastructure::api_trans::{
        resource_amount::parse_resource_delegation_native_trx_units,
        resource_authorization::{
            ResourceDelegationSigner, new_tron_delegate_args, new_tron_undelegate_args,
            resolve_resource_delegation_signer,
        },
        resource_rpc_auth,
    },
};
use tokio::sync::Mutex;
use wallet_chain_interact::{
    BillResourceConsume,
    tron::operations::{
        RawTransactionParams, TronTxOperation, stake::ResourceType as TronStakeResourceType,
    },
};
use wallet_database::{
    ApiResourceDbPool,
    entities::{
        api_resource_delegation::{
            ApiResourceDelegationEntity, ApiResourceDelegationOperationType,
            ApiResourceDelegationSource,
        },
        api_resource_type::ApiResourceType,
    },
    repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo,
};
use wallet_utils::RetryableError as _;

const RESOURCE_BROADCAST_UNCERTAIN_TIMEOUT_SECS: i64 = 300;
const RESOURCE_BROADCAST_UNCERTAIN_AUTO_REBROADCAST_LIMIT: i64 = 1;
const DELEGATION_CAPACITY_INSUFFICIENT_CACHE_TTL: Duration = Duration::from_secs(2);

pub(crate) fn local_delegate_execution_allowed(delegation: &ApiResourceDelegationEntity) -> bool {
    delegation.source == ApiResourceDelegationSource::Local
        && delegation.operation_type == ApiResourceDelegationOperationType::Delegate
        && delegation.finished_at.is_none()
        && delegation.err_code.is_none()
        && delegation.chain_success_at.is_none()
        && delegation.chain_failed_at.is_none()
}

type DelegationCapacityReservations = HashMap<String, i64>;

#[derive(Clone, Copy)]
struct DelegationCapacitySnapshot {
    available_native_amount: i64,
    observed_at: Instant,
}

type DelegationCapacitySnapshots = HashMap<String, DelegationCapacitySnapshot>;
type DelegationCapacityLocks = HashMap<String, Arc<Mutex<()>>>;

static DELEGATION_CAPACITY_RESERVATIONS: OnceLock<Mutex<DelegationCapacityReservations>> =
    OnceLock::new();
static DELEGATION_CAPACITY_SNAPSHOTS: OnceLock<Mutex<DelegationCapacitySnapshots>> =
    OnceLock::new();
static DELEGATION_CAPACITY_LOCKS: OnceLock<Mutex<DelegationCapacityLocks>> = OnceLock::new();

fn delegation_capacity_reservations() -> &'static Mutex<DelegationCapacityReservations> {
    DELEGATION_CAPACITY_RESERVATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn delegation_capacity_snapshots() -> &'static Mutex<DelegationCapacitySnapshots> {
    DELEGATION_CAPACITY_SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn delegation_capacity_locks() -> &'static Mutex<DelegationCapacityLocks> {
    DELEGATION_CAPACITY_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn delegation_capacity_lock(key: &str) -> Arc<Mutex<()>> {
    let mut locks = delegation_capacity_locks().lock().await;
    locks.entry(key.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

fn delegation_capacity_key(owner_address: &str, resource_type: ApiResourceType) -> String {
    format!("{}:{}", owner_address, resource_type.as_i64())
}

fn cached_delegation_capacity_is_insufficient(
    snapshot: DelegationCapacitySnapshot,
    reserved_native_amount: i64,
    required_native_amount: i64,
    now: Instant,
) -> bool {
    now.duration_since(snapshot.observed_at) <= DELEGATION_CAPACITY_INSUFFICIENT_CACHE_TTL
        && snapshot.available_native_amount.saturating_sub(reserved_native_amount)
            < required_native_amount
}

fn reserve_delegation_capacity(
    reservations: &mut DelegationCapacityReservations,
    key: &str,
    available_native_amount: i64,
    required_native_amount: i64,
) -> bool {
    let reserved_native_amount = reservations.get(key).copied().unwrap_or_default();
    if required_native_amount <= 0
        || available_native_amount.saturating_sub(reserved_native_amount) < required_native_amount
    {
        return false;
    }

    reservations.insert(key.to_string(), reserved_native_amount + required_native_amount);
    true
}

fn release_delegation_capacity(
    reservations: &mut DelegationCapacityReservations,
    key: &str,
    reserved_native_amount: i64,
) {
    let Some(current) = reservations.get_mut(key) else {
        return;
    };
    *current = current.saturating_sub(reserved_native_amount);
    if *current == 0 {
        reservations.remove(key);
    }
}

#[derive(Debug)]
struct DelegationCapacityReservation {
    key: String,
    native_amount: i64,
    released: bool,
}

impl DelegationCapacityReservation {
    async fn release(mut self) {
        let mut reservations = delegation_capacity_reservations().lock().await;
        release_delegation_capacity(&mut reservations, &self.key, self.native_amount);
        self.released = true;
    }
}

impl Drop for DelegationCapacityReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let key = self.key.clone();
        let native_amount = self.native_amount;
        tokio::spawn(async move {
            let mut reservations = delegation_capacity_reservations().lock().await;
            release_delegation_capacity(&mut reservations, &key, native_amount);
        });
    }
}

async fn reserve_delegate_capacity_if_needed(
    ctx: &'static Context,
    delegation: &ApiResourceDelegationEntity,
) -> Result<Option<DelegationCapacityReservation>, ServiceError> {
    if delegation.operation_type != ApiResourceDelegationOperationType::Delegate {
        return Ok(None);
    }

    let required_native_amount =
        parse_resource_delegation_native_trx_units(&delegation.native_amount)?;
    let tron_resource_type = match delegation.resource_type {
        ApiResourceType::Bandwidth => TronStakeResourceType::BANDWIDTH,
        ApiResourceType::Energy => TronStakeResourceType::ENERGY,
    };
    let key = delegation_capacity_key(&delegation.owner_address, delegation.resource_type);
    let capacity_lock = delegation_capacity_lock(&key).await;
    let _capacity_guard = capacity_lock.lock().await;

    let cached_snapshot = delegation_capacity_snapshots().lock().await.get(&key).copied();
    let reserved_native_amount =
        delegation_capacity_reservations().lock().await.get(&key).copied().unwrap_or_default();
    if cached_snapshot.is_some_and(|snapshot| {
        cached_delegation_capacity_is_insufficient(
            snapshot,
            reserved_native_amount,
            required_native_amount,
            Instant::now(),
        )
    }) {
        let available_native_amount =
            cached_snapshot.map(|snapshot| snapshot.available_native_amount).unwrap_or_default();
        tracing::warn!(
            observability_event = "resource_delegation_capacity_insufficient_cached",
            resource_trade_no = %delegation.resource_trade_no,
            source = ?delegation.source,
            owner_address = %delegation.owner_address,
            required_native_amount,
            available_native_amount,
            reserved_native_amount,
            "Resource delegation skipped using recent insufficient capacity snapshot"
        );
        return Err(ServiceError::Parameter(format!(
            "resource delegation capacity insufficient: required={}, available={}, reserved={}",
            required_native_amount, available_native_amount, reserved_native_amount
        )));
    }

    let capacity_check_start = Instant::now();
    let chain = ChainAdapterFactory::get_tron_adapter(ctx).await?;
    let available_native_amount = chain
        .get_provider()
        .can_delegate_resource(&delegation.owner_address, tron_resource_type)
        .await?
        .to_sun();

    let mut reservations = delegation_capacity_reservations().lock().await;
    let reserved_native_amount = reservations.get(&key).copied().unwrap_or_default();
    if !reserve_delegation_capacity(
        &mut reservations,
        &key,
        available_native_amount,
        required_native_amount,
    ) {
        delegation_capacity_snapshots().lock().await.insert(
            key.clone(),
            DelegationCapacitySnapshot { available_native_amount, observed_at: Instant::now() },
        );
        tracing::warn!(
            observability_event = "resource_delegation_capacity_insufficient",
            resource_trade_no = %delegation.resource_trade_no,
            source = ?delegation.source,
            owner_address = %delegation.owner_address,
            required_native_amount,
            available_native_amount,
            reserved_native_amount,
            capacity_elapsed_ms = %capacity_check_start.elapsed().as_millis(),
            "Resource delegation capacity is insufficient before broadcast"
        );
        return Err(ServiceError::Parameter(format!(
            "resource delegation capacity insufficient: required={}, available={}, reserved={}",
            required_native_amount, available_native_amount, reserved_native_amount
        )));
    }

    delegation_capacity_snapshots().lock().await.remove(&key);

    tracing::info!(
        observability_event = "resource_delegation_capacity_reserved",
        resource_trade_no = %delegation.resource_trade_no,
        source = ?delegation.source,
        owner_address = %delegation.owner_address,
        required_native_amount,
        available_native_amount,
        reserved_native_amount_before = reserved_native_amount,
        reserved_native_amount_after = reserved_native_amount + required_native_amount,
        capacity_elapsed_ms = %capacity_check_start.elapsed().as_millis(),
        "Resource delegation capacity reserved"
    );

    Ok(Some(DelegationCapacityReservation {
        key,
        native_amount: required_native_amount,
        released: false,
    }))
}

pub(crate) fn resource_delegation_is_built_unbroadcast(
    delegation: &ApiResourceDelegationEntity,
) -> bool {
    delegation.raw_tx.as_deref().is_some_and(|raw_tx| !raw_tx.trim().is_empty())
        && delegation.tx_hash.as_deref().is_some_and(|tx_hash| !tx_hash.trim().is_empty())
        && delegation.last_broadcast_at.is_none()
}

/// Shared bounded backoff for non-delegation recovery tables that still own a
/// dedicated retry schedule, such as TRON account activation.
pub(crate) fn resource_delegation_retry_wait_secs(retry_count: i64) -> i64 {
    let exponent = retry_count.clamp(0, 3) as u32;
    (60_i64 * (1_i64 << exponent)).min(300)
}

pub(crate) fn should_rebroadcast_resource_uncertain(
    delegation: &ApiResourceDelegationEntity,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let Some(since) = delegation.broadcast_uncertain_since_at else {
        return false;
    };
    delegation.broadcast_uncertain_rebroadcast_count
        < RESOURCE_BROADCAST_UNCERTAIN_AUTO_REBROADCAST_LIMIT
        && now.signed_duration_since(since).num_seconds()
            >= RESOURCE_BROADCAST_UNCERTAIN_TIMEOUT_SECS
}

pub(crate) async fn record_resource_uncertain_and_maybe_rebroadcast(
    pool: &wallet_database::ApiResourceDbPool,
    resource_trade_no: &str,
) -> Result<bool, ServiceError> {
    ApiResourceDelegationRepo::mark_broadcast_uncertain_attempt(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

    let refreshed = ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

    if !should_rebroadcast_resource_uncertain(&refreshed, chrono::Utc::now()) {
        return Ok(false);
    }

    ApiResourceDelegationRepo::mark_broadcast_uncertain_reconciled(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

    let rows = ApiResourceDelegationRepo::invalidate_for_rebroadcast(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;

    if rows > 0 {
        ApiResourceDelegationRepo::mark_broadcast_uncertain_rebroadcast_attempted(
            pool,
            resource_trade_no,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
        return Ok(true);
    }

    Ok(false)
}

/// 资源代理失败 fact 的统一映射。
/// - 网络问题归入 ERR_6005
/// - 其他失败归入 ERR_6008
pub(crate) fn resource_delegation_failure_fact(err: &ServiceError) -> (String, String) {
    let err_code = if err.is_network_error() { "ERR_6005" } else { "ERR_6008" };
    (err_code.to_string(), err.to_string())
}

pub(crate) fn resource_recovery_error_is_uncertain(err: &ServiceError) -> bool {
    err.is_network_error()
        || err.is_delay_retryable()
        || err.is_rpc_auth_unauthorized()
        || err.is_transient_db_pool_timeout()
        || matches!(
            err,
            ServiceError::Timeout
                | ServiceError::TransportBackend(
                    wallet_transport_backend::Error::Timeout
                        | wallet_transport_backend::Error::RateLimited
                        | wallet_transport_backend::Error::Backpressure
                )
        )
        || matches!(
            err,
            ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
                code,
                _
            )) if (500..=599).contains(code)
        )
        || shared_mechanism::is_tron_missing_confirmed_and_pending_error(err)
}

pub(crate) async fn mark_recovered_resource_delegation_failure(
    pool: &ApiResourceDbPool,
    delegation: &ApiResourceDelegationEntity,
    tx_hash: &str,
    err: &ServiceError,
    payload_source: &'static str,
) -> Result<ApiResourceDelegationEntity, ServiceError> {
    let (err_code, err_msg) = resource_delegation_failure_fact(err);
    let payload = format!("{payload_source}:{err_msg}");
    ApiResourceDelegationRepo::mark_recovered_chain_failure(
        pool,
        &delegation.resource_trade_no,
        tx_hash,
        None,
        &err_code,
        &err_msg,
        &payload,
    )
    .await
    .map_err(|e| ServiceError::Database(e.into()))?;

    ApiResourceDelegationRepo::get_by_resource_trade_no(pool, &delegation.resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))
}

/// 使用统一 RPC-auth 策略执行资源代理广播。
///
/// 注意：
/// 1) 这里只做“执行一个 resource delegation 任务本身”，
/// 2) 不包含 collect/withdraw 主链推进逻辑。
/// 3) 是否立即触发下一步，由各自 shadow/stage 决定。
pub(crate) async fn execute_resource_delegation(
    ctx: &'static Context,
    pool: &ApiResourceDbPool,
    delegation: &ApiResourceDelegationEntity,
    rpc_purpose: &'static str,
) -> Result<String, ServiceError> {
    resource_rpc_auth::run_with_rpc_auth_retry(
        &delegation.chain_code,
        rpc_purpose,
        &delegation.resource_trade_no,
        || execute_resource_delegation_once(ctx, pool, &delegation.resource_trade_no),
    )
    .await
}

// 资源任务执行内部实现：支持 Delegate / Undelegate 的 TRON 交易构建、签名与广播。
async fn execute_resource_delegation_once(
    ctx: &'static Context,
    pool: &ApiResourceDbPool,
    resource_trade_no: &str,
) -> Result<String, ServiceError> {
    let mut delegation =
        ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
    if !shared_mechanism::is_tron_chain(&delegation.chain_code) {
        return Err(ServiceError::Parameter(format!(
            "resource delegation only supports tron, got {}",
            delegation.chain_code
        )));
    }

    let _chain_rpc_guard = crate::infrastructure::chain_rpc_guard::acquire_if_guarded_with_ctx(
        ctx,
        &delegation.chain_code,
    )
    .await;
    let capacity_reservation = reserve_delegate_capacity_if_needed(ctx, &delegation).await?;
    if delegation.raw_tx.is_none() && delegation.tx_hash.is_none() {
        let (tx_hash, raw_tx) = build_signed_resource_delegation(ctx, &delegation).await?;
        let raw_tx_json = wallet_utils::serde_func::serde_to_string(&raw_tx)?;
        let rows =
            ApiResourceDelegationRepo::mark_built(pool, resource_trade_no, &tx_hash, &raw_tx_json)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
        if rows == 0 {
            return Err(ServiceError::Database(wallet_database::Error::Database(
                wallet_database::DatabaseError::UpdateFailed(format!(
                    "resource delegation build fact conflict: trade_no={resource_trade_no}"
                )),
            )));
        }
        delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
    }
    let tx_hash = delegation
        .tx_hash
        .as_deref()
        .filter(|hash| !hash.trim().is_empty())
        .ok_or_else(|| {
            ServiceError::Parameter(format!(
                "resource delegation persisted build is missing tx_hash: {resource_trade_no}"
            ))
        })?
        .to_string();
    let raw_tx_json =
        delegation.raw_tx.as_deref().filter(|raw| !raw.trim().is_empty()).ok_or_else(|| {
            ServiceError::Parameter(format!(
                "resource delegation persisted build is missing raw_tx: {resource_trade_no}"
            ))
        })?;
    let raw_tx = wallet_utils::serde_func::serde_from_str(raw_tx_json)?;
    let tx_resp =
        ApiTransDomain::broadcast_transfer(ctx, &delegation.chain_code, raw_tx, Some(&tx_hash))
            .await?;

    let response_tx_hash = tx_resp.as_ref().map(|tx| tx.tx_hash.as_str());
    let resolved_tx_hash = resolve_resource_delegation_broadcast_hash(&tx_hash, response_tx_hash)?;
    let marked =
        ApiResourceDelegationRepo::mark_broadcasted(pool, resource_trade_no, &resolved_tx_hash)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
    if marked == 0 {
        let refreshed =
            ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
                .await
                .map_err(|e| ServiceError::Database(e.into()))?;
        if refreshed.tx_hash.as_deref() != Some(resolved_tx_hash.as_str())
            || refreshed.last_broadcast_at.is_none()
        {
            return Err(ServiceError::Database(wallet_database::Error::Database(
                wallet_database::DatabaseError::UpdateFailed(format!(
                    "resource delegation broadcast fact conflict: trade_no={resource_trade_no}"
                )),
            )));
        }
    }
    if let Some(reservation) = capacity_reservation {
        reservation.release().await;
    }
    if tx_resp.is_none() {
        tracing::info!(
            resource_trade_no = %delegation.resource_trade_no,
            tx_hash = %resolved_tx_hash,
            "Resource delegation broadcast result uncertain; preserving signed hash for recovery"
        );
    }

    Ok(resolved_tx_hash)
}

async fn build_signed_resource_delegation(
    ctx: &'static Context,
    delegation: &ApiResourceDelegationEntity,
) -> Result<(String, crate::domain::api_wallet::adapter::tx::RawTx), ServiceError> {
    let chain = ChainAdapterFactory::get_tron_adapter(ctx).await?;
    let trx_amount = parse_resource_delegation_native_trx_units(&delegation.native_amount)?;
    let resource = match delegation.resource_type {
        ApiResourceType::Bandwidth => "bandwidth",
        _ => "energy",
    };
    let signer = resolve_resource_delegation_signer(ctx, delegation).await?;
    let raw = match delegation.operation_type {
        ApiResourceDelegationOperationType::Delegate => {
            let args = new_tron_delegate_args(
                &delegation.owner_address,
                &delegation.receiver_address,
                trx_amount,
                resource,
                signer.permission_id,
            )?;
            args.build_raw_transaction(chain.get_provider()).await?
        }
        ApiResourceDelegationOperationType::Undelegate => {
            let args = new_tron_undelegate_args(
                &delegation.owner_address,
                &delegation.receiver_address,
                trx_amount,
                resource,
                signer.permission_id,
            )?;
            args.build_raw_transaction(chain.get_provider()).await?
        }
    };
    sign_tron_resource_delegation(ctx, delegation, &signer, raw).await
}

fn resolve_resource_delegation_broadcast_hash(
    signed_tx_hash: &str,
    response_tx_hash: Option<&str>,
) -> Result<String, ServiceError> {
    if response_tx_hash.is_some_and(|response| response != signed_tx_hash) {
        return Err(ServiceError::System(SystemError::Internal(
            "resource delegation tx_hash mismatch between build and broadcast".to_string(),
        )));
    }
    Ok(signed_tx_hash.to_string())
}

async fn sign_tron_resource_delegation(
    ctx: &'static Context,
    delegation: &ApiResourceDelegationEntity,
    signer: &ResourceDelegationSigner,
    mut raw: RawTransactionParams,
) -> Result<(String, crate::domain::api_wallet::adapter::tx::RawTx), ServiceError> {
    let chain = ChainAdapterFactory::get_tron_adapter(ctx).await?;
    let provider = chain.get_provider();
    let consumer =
        provider.transfer_fee(&delegation.owner_address, None, &raw.raw_data_hex, 1).await?;
    let balance = chain.balance(&delegation.owner_address, None).await?;
    if balance.to::<i64>() < consumer.transaction_fee_i64() {
        return Err(ServiceError::Parameter(format!(
            "resource delegation balance is insufficient for tx fee: balance={}, need={}",
            balance,
            consumer.transaction_fee_i64()
        )));
    }

    let handles = ctx.get_handles_arc().await?;
    let private_key_manager = handles.get_global_private_key_manager();
    let private_key =
        private_key_manager.get_private_key(&signer.signer_address, &delegation.chain_code).await?;
    let sign = wallet_utils::sign::sign_tron(&raw.tx_id, &private_key, None)?;
    raw.signature.push(sign);

    let tx_hash = raw.tx_id.clone();
    let raw_tx = crate::domain::api_wallet::adapter::tx::RawTx::Tron(
        raw,
        BillResourceConsume::new_tron(consumer.act_bandwidth() as u64, 0),
        consumer.transaction_fee(),
    );

    Ok((tx_hash, raw_tx))
}

#[cfg(test)]
mod tests {
    use super::{
        DELEGATION_CAPACITY_INSUFFICIENT_CACHE_TTL, DelegationCapacityReservations,
        DelegationCapacitySnapshot, cached_delegation_capacity_is_insufficient,
        delegation_capacity_lock, local_delegate_execution_allowed,
        mark_recovered_resource_delegation_failure, release_delegation_capacity,
        reserve_delegation_capacity, resolve_resource_delegation_broadcast_hash,
        resource_delegation_failure_fact, resource_delegation_retry_wait_secs,
        resource_recovery_error_is_uncertain,
    };

    #[test]
    fn resource_delegation_capacity_allows_only_one_of_ten_when_one_order_fits() {
        let mut reservations = DelegationCapacityReservations::new();
        let key = "TY9:1";

        let admitted = (0..10)
            .filter(|_| reserve_delegation_capacity(&mut reservations, key, 199, 199))
            .count();

        assert_eq!(admitted, 1);
        assert_eq!(reservations.get(key).copied(), Some(199));
    }

    #[test]
    fn resource_delegation_capacity_release_is_immediately_reusable() {
        let mut reservations = DelegationCapacityReservations::new();
        let key = "TY9:1";

        assert!(reserve_delegation_capacity(&mut reservations, key, 199, 199));
        assert!(!reserve_delegation_capacity(&mut reservations, key, 199, 199));

        release_delegation_capacity(&mut reservations, key, 199);

        assert_eq!(reservations.get(key), None);
        assert!(reserve_delegation_capacity(&mut reservations, key, 199, 199));
    }

    #[test]
    fn delegation_capacity_insufficient_cache_short_circuits_zero_snapshot() {
        let now = std::time::Instant::now();
        let snapshot = DelegationCapacitySnapshot { available_native_amount: 0, observed_at: now };

        assert!(cached_delegation_capacity_is_insufficient(snapshot, 0, 199, now));
    }

    #[test]
    fn delegation_capacity_insufficient_cache_expires() {
        let now = std::time::Instant::now();
        let snapshot = DelegationCapacitySnapshot {
            available_native_amount: 0,
            observed_at: now
                - DELEGATION_CAPACITY_INSUFFICIENT_CACHE_TTL
                - std::time::Duration::from_millis(1),
        };

        assert!(!cached_delegation_capacity_is_insufficient(snapshot, 0, 199, now));
    }

    #[tokio::test]
    async fn delegation_capacity_insufficient_cache_coalesces_same_owner_resource() {
        let first = delegation_capacity_lock("owner:energy").await;
        let second = delegation_capacity_lock("owner:energy").await;

        assert!(std::sync::Arc::ptr_eq(&first, &second));
    }
    use crate::error::service::ServiceError;
    use tempfile::tempdir;
    use wallet_database::{
        SqliteContext,
        entities::{
            api_resource_delegation::{
                ApiResourceDelegationResultStatus, ApiResourceDelegationStatus,
                NewApiResourceDelegation,
            },
            api_trade_type::ApiTradeType,
        },
        repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo,
    };

    fn local_delegate_task()
    -> wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity {
        let now = chrono::Utc::now();
        wallet_database::entities::api_resource_delegation::ApiResourceDelegationEntity {
            id: 1,
            uid: "uid".to_string(),
            source: wallet_database::entities::api_resource_delegation::ApiResourceDelegationSource::Local,
            operation_type: wallet_database::entities::api_resource_delegation::ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: Some("C1".to_string()),
            origin_trade_type: Some(ApiTradeType::Collect as i64),
            resource_trade_no: "rsc_local_delegate_C1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: "receiver".to_string(),
            delegation_mode: wallet_database::entities::api_resource_delegation::ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: wallet_database::entities::api_resource_type::ApiResourceType::Energy,
            native_amount: "199".to_string(),
            amount: "14650".to_string(),
            status: wallet_database::entities::api_resource_delegation::ApiResourceDelegationStatus::Pending,
            task_ack_sent_at: None,
            task_ack_send_count: 0,
            building_at: None,
            raw_tx: None,
            last_broadcast_at: None,
            tx_hash: None,
            chain_success_at: None,
            chain_failed_at: None,
            transaction_time: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: None,
            result_status: None,
            tx_res_received_at: None,
            finished_at: None,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            created_at: now,
            updated_at: None,
        }
    }

    #[test]
    fn local_delegate_execution_boundary_allows_only_active_local_delegate() {
        let active = local_delegate_task();
        assert!(local_delegate_execution_allowed(&active));

        let mut platform = active.clone();
        platform.source = wallet_database::entities::api_resource_delegation::ApiResourceDelegationSource::Platform;
        assert!(!local_delegate_execution_allowed(&platform));

        let mut undelegate = active.clone();
        undelegate.operation_type = wallet_database::entities::api_resource_delegation::ApiResourceDelegationOperationType::Undelegate;
        assert!(!local_delegate_execution_allowed(&undelegate));

        let mut terminal = active;
        terminal.finished_at = Some(chrono::Utc::now());
        assert!(!local_delegate_execution_allowed(&terminal));
    }

    #[test]
    fn retry_wait_secs_follows_exp_backoff_with_cap() {
        assert_eq!(resource_delegation_retry_wait_secs(0), 60);
        assert_eq!(resource_delegation_retry_wait_secs(1), 120);
        assert_eq!(resource_delegation_retry_wait_secs(2), 240);
        assert_eq!(resource_delegation_retry_wait_secs(3), 300);
        assert_eq!(resource_delegation_retry_wait_secs(5), 300);
        assert_eq!(resource_delegation_retry_wait_secs(6), 300);
        assert_eq!(resource_delegation_retry_wait_secs(10), 300);
    }

    #[test]
    fn failure_fact_non_network_defaults_to_err_6008() {
        let err = ServiceError::Parameter("test-business-error".to_string());
        let (code, msg) = resource_delegation_failure_fact(&err);
        assert_eq!(code, "ERR_6008");
        assert_eq!(msg, "parameter error: test-business-error");
    }

    #[test]
    fn recovery_error_classifier_keeps_transient_observation_errors_uncertain() {
        let visibility_gap =
            ServiceError::Parameter("tron tx missing from confirmed and pending pools".to_string());
        let timeout = ServiceError::Timeout;
        let backend_timeout =
            ServiceError::TransportBackend(wallet_transport_backend::Error::Timeout);
        let rate_limited =
            ServiceError::TransportBackend(wallet_transport_backend::Error::RateLimited);
        let backend_unavailable =
            ServiceError::TransportBackend(wallet_transport_backend::Error::ApiBackend(
                503,
                Some("Service Unavailable".to_string()),
            ));
        let unauthorized = ServiceError::TransportBackend(
            wallet_transport_backend::Error::ApiBackend(401, Some("Unauthorized".to_string())),
        );
        let confirmed_failure = ServiceError::Parameter("chain execution failed".to_string());

        assert!(resource_recovery_error_is_uncertain(&visibility_gap));
        assert!(resource_recovery_error_is_uncertain(&timeout));
        assert!(resource_recovery_error_is_uncertain(&backend_timeout));
        assert!(resource_recovery_error_is_uncertain(&rate_limited));
        assert!(resource_recovery_error_is_uncertain(&backend_unavailable));
        assert!(resource_recovery_error_is_uncertain(&unauthorized));
        assert!(!resource_recovery_error_is_uncertain(&confirmed_failure));
    }

    #[test]
    fn uncertain_broadcast_preserves_signed_hash_for_recovery() {
        let hash = resolve_resource_delegation_broadcast_hash("signed-hash", None)
            .expect("uncertain broadcast keeps deterministic hash");

        assert_eq!(hash, "signed-hash");
    }

    #[test]
    fn explicit_broadcast_hash_mismatch_fails() {
        let error =
            resolve_resource_delegation_broadcast_hash("signed-hash", Some("different-hash"))
                .expect_err("hash mismatch must fail");

        assert!(error.to_string().contains("tx_hash mismatch"));
    }

    #[tokio::test]
    async fn recovered_failure_helper_commits_complete_failure_fact() {
        let dir = tempdir().expect("tempdir");
        let sqlite = SqliteContext::new(&dir.path().to_string_lossy(), Some("api_resource.db"))
            .await
            .expect("init resource db");
        let pool = sqlite.into_api_resource_db_pool().expect("resource pool");
        let resource_trade_no = "rsc_api_recovered_failure";
        let tx_hash = "rsc_api_recovered_failure_hash";

        ApiResourceDelegationRepo::upsert(
            &pool,
            NewApiResourceDelegation::platform_delegate(
                "uid",
                resource_trade_no,
                "W_api_recovered_failure",
                ApiTradeType::Withdraw as i64,
                "owner",
                "receiver",
                "100",
            ),
        )
        .await
        .expect("insert delegation");
        ApiResourceDelegationRepo::mark_task_ack_sent(&pool, resource_trade_no)
            .await
            .expect("mark task ack");
        assert_eq!(
            ApiResourceDelegationRepo::claim_build_slot(&pool, resource_trade_no)
                .await
                .expect("claim build"),
            1
        );
        assert_eq!(
            ApiResourceDelegationRepo::mark_built(&pool, resource_trade_no, tx_hash, "signed_raw",)
                .await
                .expect("mark built"),
            1
        );
        assert_eq!(
            ApiResourceDelegationRepo::mark_broadcasted(&pool, resource_trade_no, tx_hash)
                .await
                .expect("mark broadcast"),
            1
        );
        let delegation =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&pool, resource_trade_no)
                .await
                .expect("load delegation");

        let failed = mark_recovered_resource_delegation_failure(
            &pool,
            &delegation,
            tx_hash,
            &ServiceError::Parameter("confirmed failure".to_string()),
            "api_recovered_failure",
        )
        .await
        .expect("commit recovered failure");

        assert_eq!(failed.tx_hash.as_deref(), Some(tx_hash));
        assert_eq!(failed.status, ApiResourceDelegationStatus::Fail);
        assert_eq!(failed.result_status, Some(ApiResourceDelegationResultStatus::Fail));
        assert_eq!(failed.err_code.as_deref(), Some("ERR_6008"));
        assert!(failed.chain_failed_at.is_some());
        assert!(failed.finished_at.is_some());
        assert!(
            failed
                .result_payload
                .as_deref()
                .is_some_and(|payload| payload.starts_with("api_recovered_failure:"))
        );
    }
}
