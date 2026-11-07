//! Test-only entrypoints; fake transport and fixtures live in tests/harness.
use crate::{context::Context, error::service::ServiceError};

pub async fn initialize(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::node::NodeDomain::init_load_default_nodes_with_ctx(ctx).await?;
    crate::domain::api_wallet::chain::ApiChainDomain::init_api_chain_info(ctx).await
}

pub async fn pull_ordinary_coins(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::service::coin::CoinService::new(ctx).pull_hot_coins().await
}

pub async fn pull_coins(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::api_wallet::coin::ApiCoinDomain::pull_api_coins(ctx).await.map(|_| ())
}

pub async fn register_addresses(
    ctx: &'static Context,
    request: wallet_transport_backend::request::api_wallet::address::ApiAddressInitReq,
) -> Result<(), ServiceError> {
    crate::infrastructure::expand_init::executor::do_init_with_ctx(ctx, request).await
}

pub fn core_pool(ctx: &'static Context) -> Result<wallet_database::CoreDbPool, ServiceError> {
    ctx.core_pool()
}

pub async fn sync_chains(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::api_wallet::chain::ApiChainDomain::sync_chains(ctx).await.map(|_| ())
}

/// Exercise the same bounded registration path used by missing-chain reconciliation.
pub async fn register_bounded(
    ctx: &'static Context,
    request: wallet_transport_backend::request::api_wallet::address::ApiAddressInitReq,
) -> Result<(), ServiceError> {
    crate::domain::api_wallet::chain::register_missing_chain_addresses(ctx, request).await
}

/// Install synthetic wallet envelopes/session in this isolated test Context.
pub async fn prepare_sync_wallets(ctx: &'static Context) -> Result<(), ServiceError> {
    use crate::{
        context::{WalletUnlockMaterial, WalletUnlockSession},
        domain::api_wallet::unlock::{SeedEnvelopeCodec, WalletUnlockSessionCodec},
    };
    let mut materials = std::collections::HashMap::new();
    for (address, uid, kind) in [
        (
            "sync-sub-wallet",
            "sync-sub-uid",
            wallet_database::entities::api_wallet::ApiWalletType::SubAccount,
        ),
        (
            "sync-withdraw-wallet",
            "sync-withdraw-uid",
            wallet_database::entities::api_wallet::ApiWalletType::Withdrawal,
        ),
    ] {
        let seed: [u8; 32] = rand::random();
        let encrypted =
            SeedEnvelopeCodec::encrypt_seed_bundle("synthetic-test-password", &seed).await?;
        let envelope =
            SeedEnvelopeCodec::decrypt_seed_envelope("synthetic-test-password", &encrypted).await?;
        let smk =
            WalletUnlockSessionCodec::derive_smk("synthetic-test-password", &envelope.salt).await?;
        wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::upsert(
            &ctx.api_wallet_pool()?,
            uid,
            "synthetic sync wallet",
            address,
            &[],
            &encrypted,
            kind,
            None,
            "test-sn",
            0,
        )
        .await?;
        materials.insert(address.to_owned(), WalletUnlockMaterial::new(smk.to_vec()));
    }
    ctx.set_wallet_unlock_session(WalletUnlockSession::new(
        "synthetic-sync-token".to_owned(),
        std::time::Instant::now() + std::time::Duration::from_secs(600),
        materials,
    ))
    .await
}
pub async fn sync_accounts(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::api_wallet::chain::ApiChainDomain::sync_wallet_chain_data(
        ctx,
        "synthetic-sync-token",
    )
    .await
}

/// Restore an unlock session from the existing synthetic envelopes after reopening.
pub async fn restore_sync_wallet_session(ctx: &'static Context) -> Result<(), ServiceError> {
    use crate::{
        context::{WalletUnlockMaterial, WalletUnlockSession},
        domain::api_wallet::unlock::{SeedEnvelopeCodec, WalletUnlockSessionCodec},
    };
    let mut materials = std::collections::HashMap::new();
    for address in ["sync-sub-wallet", "sync-withdraw-wallet"] {
        let wallet =
            wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::find_by_address(
                &ctx.api_wallet_pool()?,
                address,
            )
            .await?
            .ok_or(crate::error::system::SystemError::SystemNotReady)?;
        let envelope =
            SeedEnvelopeCodec::decrypt_seed_envelope("synthetic-test-password", &wallet.seed)
                .await?;
        let smk =
            WalletUnlockSessionCodec::derive_smk("synthetic-test-password", &envelope.salt).await?;
        materials.insert(address.to_owned(), WalletUnlockMaterial::new(smk.to_vec()));
    }
    ctx.set_wallet_unlock_session(WalletUnlockSession::new(
        "synthetic-sync-token".to_owned(),
        std::time::Instant::now() + std::time::Duration::from_secs(600),
        materials,
    ))
    .await
}

/// Invoke the production unlock entry that assembles the session and starts recovery.
pub async fn unlock_and_start_recovery(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::api_wallet::wallet::ApiWalletDomain::new(ctx)
        .initialize_wallet_unlock_session("synthetic-test-password")
        .await
}
pub async fn lock_sync_wallet(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::domain::api_wallet::wallet::ApiWalletDomain::new(ctx).clear_wallet_unlock_session().await
}

/// Explicit test steps use the same context-local consumer/reservations as periodic wake.
pub async fn drain_saved_recovery(ctx: &'static Context) -> Result<(), ServiceError> {
    use wallet_database::repositories::api_wallet::chain_recovery::{
        ChainRecoveryRepo, DUE_PAGE_SIZE,
    };
    crate::infrastructure::system_ready::mark_system_ready();
    for _ in 0..100 {
        let rows = ChainRecoveryRepo::due(
            &ctx.task_pool()?,
            chrono::Utc::now().timestamp_millis(),
            DUE_PAGE_SIZE,
        )
        .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            ctx.api_chain_recovery.run_saved_once_for_test(ctx, &row.id).await?;
        }
    }
    Ok(())
}

pub async fn recovery_task_ids(ctx: &'static Context) -> Result<Vec<String>, ServiceError> {
    Ok(wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(&ctx.task_pool()?)
        .await?
        .into_iter()
        .filter(|row| row.id.starts_with("api_chain_reconcile:"))
        .map(|row| row.id)
        .collect())
}

pub async fn run_saved_job(ctx: &'static Context, id: &str) -> Result<(), ServiceError> {
    ctx.api_chain_recovery.run_saved_once_for_test(ctx, id).await
}

/// Real periodic chain coordinator plus a default dispatcher for ownership isolation.
pub async fn start_recovery_coordinator(
    ctx: &'static Context,
) -> Result<RecoveryCoordinatorGuard, ServiceError> {
    crate::infrastructure::system_ready::mark_system_ready();
    let default_consumer = crate::infrastructure::task_queue::task_manager::TaskManager::new(
        ctx,
        std::sync::Arc::new(tokio::sync::Notify::new()),
    );
    ctx.api_chain_recovery.start(ctx);
    ctx.api_chain_recovery.start(ctx);
    Ok(RecoveryCoordinatorGuard { ctx, default_consumer })
}
pub struct RecoveryCoordinatorGuard {
    ctx: &'static Context,
    default_consumer: crate::infrastructure::task_queue::task_manager::TaskManager,
}
impl RecoveryCoordinatorGuard {
    pub async fn deliver_owned_to_default_consumer(&self) -> Result<(), ServiceError> {
        use crate::infrastructure::task_queue::task_manager::dispatcher::PriorityTask;
        use wallet_database::repositories::task_queue::TaskQueueRepo;
        let pool = self.ctx.task_pool()?;
        let mut tasks = Vec::new();
        for row in TaskQueueRepo::all_tasks_queue(&pool).await? {
            if TaskQueueRepo::externally_owned(&pool, row.remark.as_deref()).await? {
                tasks.push(row);
            }
        }
        self.default_consumer
            .get_task_sender()
            .send(PriorityTask { priority: 2, tasks })
            .map_err(|e| crate::error::system::SystemError::Service(e.to_string()))?;
        Ok(())
    }
}
impl Drop for RecoveryCoordinatorGuard {
    fn drop(&mut self) {
        self.ctx.api_chain_recovery.stop_for_test();
    }
}

pub async fn deliver_saved_owned_through_default_entry(
    ctx: &'static Context,
) -> Result<(), ServiceError> {
    // Deliberately do not pre-classify metadata: the actual default entry must filter it.
    let records = wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
        &ctx.task_pool()?,
    )
    .await?
    .into_iter()
    .filter(|r| r.id.starts_with("external-"))
    .collect();
    crate::infrastructure::task_queue::task::dispatch_task_entities_with_ctx(ctx, records).await
}
