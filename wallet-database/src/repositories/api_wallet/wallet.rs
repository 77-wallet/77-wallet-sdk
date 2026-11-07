use crate::{
    ApiWalletDbPool,
    dao::api_wallet::ApiWalletDao,
    db::sqlite_retry::with_sqlite_locked_retry,
    entities::api_wallet::{ApiWalletEntity, ApiWalletType},
};

pub struct ApiWalletRepo;

/// Action duration includes pool/SQLite waits and retries, not just SQL execution.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiWalletWriteTiming {
    pub writer_wait_ms: f64,
    pub write_action_ms: f64,
}

mod deletion;
pub use deletion::{ApiWalletDeletionRead, ApiWalletDeletionSource, WalletDeletionSnapshot};

impl ApiWalletRepo {
    pub async fn accounts_for_deletion(
        pool: &ApiWalletDbPool,
        addresses: &[&str],
    ) -> Result<Vec<crate::entities::api_account::ApiAccountEntity>, crate::Error> {
        crate::dao::api_account::ApiAccountDao::list_for_wallet_deletion(pool.read_ref(), addresses)
            .await
    }

    async fn with_write_guard<T, F, Fut>(
        pool: &ApiWalletDbPool,
        op: &'static str,
        action: F,
    ) -> Result<T, crate::Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::Error>>,
    {
        Self::with_write_guard_timed(pool, op, action).await.0
    }

    async fn with_write_guard_timed<T, F, Fut>(
        pool: &ApiWalletDbPool,
        op: &'static str,
        action: F,
    ) -> (Result<T, crate::Error>, ApiWalletWriteTiming)
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::Error>>,
    {
        let wait_start = std::time::Instant::now();
        let _write_guard = pool.lock_write_with_metric(op).await;
        let writer_wait_ms = wait_start.elapsed().as_secs_f64() * 1000.0;
        let tx_start = std::time::Instant::now();
        let result = with_sqlite_locked_retry(action).await;
        let write_action_ms = tx_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(metric="write_tx_duration_ms", db="api_wallet.db", op, value_ms=%write_action_ms, ok=%result.is_ok(), "api wallet write finished");
        (result, ApiWalletWriteTiming { writer_wait_ms, write_action_ms })
    }

    pub async fn upsert(
        pool: &ApiWalletDbPool,
        uid: &str,
        name: &str,
        address: &str,
        phrase: &[u8],
        seed: &[u8],
        wallet_type: ApiWalletType,
        binding_address: Option<&str>,
        sn: &str,
        import_stage: u8,
    ) -> Result<ApiWalletEntity, crate::Error> {
        Self::upsert_with_timing(
            pool,
            uid,
            name,
            address,
            phrase,
            seed,
            wallet_type,
            binding_address,
            sn,
            import_stage,
        )
        .await
        .0
    }

    pub async fn upsert_with_timing(
        pool: &ApiWalletDbPool,
        uid: &str,
        name: &str,
        address: &str,
        phrase: &[u8],
        seed: &[u8],
        wallet_type: ApiWalletType,
        binding_address: Option<&str>,
        sn: &str,
        import_stage: u8,
    ) -> (Result<ApiWalletEntity, crate::Error>, ApiWalletWriteTiming) {
        Self::with_write_guard_timed(pool, "upsert_api_wallet", || async {
            Ok(ApiWalletDao::upsert(
                pool.write_ref(),
                address,
                uid,
                name,
                phrase,
                seed,
                1,
                wallet_type,
                binding_address,
                sn,
                import_stage,
            )
            .await?)
        })
        .await
    }

    pub async fn edit_name(
        pool: &ApiWalletDbPool,
        address: &str,
        name: &str,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "edit_api_wallet_name", || async {
            Ok(ApiWalletDao::edit_name(pool.write_ref(), address, name).await?)
        })
        .await
    }

    pub async fn update_merchant_id(
        pool: &ApiWalletDbPool,
        address: &str,
        merchant_id: &str,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "update_merchant_id", || async {
            Ok(ApiWalletDao::update_merchain_id(pool.write_ref(), address, merchant_id).await?)
        })
        .await
    }

    pub async fn update_app_id(
        pool: &ApiWalletDbPool,
        address: &str,
        app_id: Option<&str>,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "update_app_id", || async {
            Ok(ApiWalletDao::update_app_id(pool.write_ref(), address, app_id).await?)
        })
        .await
    }

    pub async fn update_sn(
        pool: &ApiWalletDbPool,
        address: &str,
        sn: &str,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "update_wallet_sn", || async {
            Ok(ApiWalletDao::update_sn(pool.write_ref(), address, sn).await?)
        })
        .await
    }

    pub async fn update_seed_and_phrase(
        pool: &ApiWalletDbPool,
        uid: &str,
        phrase: &[u8],
        seed: &[u8],
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "update_seed_and_phrase", || async {
            Ok(ApiWalletDao::update_seed_and_phrase(pool.write_ref(), uid, phrase, seed).await?)
        })
        .await
    }

    pub async fn unbind_uid(pool: &ApiWalletDbPool, address: &str) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "unbind_wallet_uid", || async {
            Ok(ApiWalletDao::unbind_uid(pool.write_ref(), address).await?)
        })
        .await
    }

    pub async fn clear_binding_address(
        pool: &ApiWalletDbPool,
        address: &str,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "clear_binding_address", || async {
            Ok(ApiWalletDao::clear_binding_address(pool.write_ref(), address).await?)
        })
        .await
    }

    pub async fn clear_binding_relation(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        binding_address: &str,
    ) -> Result<u64, crate::Error> {
        Self::with_write_guard(pool, "clear_binding_relation", || async {
            Ok(ApiWalletDao::clear_binding_relation(
                pool.write_ref(),
                wallet_address,
                binding_address,
            )
            .await?)
        })
        .await
    }

    pub async fn mark_init(pool: &ApiWalletDbPool, uid: &str) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "mark_wallet_init", || async {
            Ok(ApiWalletDao::mark_init(pool.write_ref(), uid).await?)
        })
        .await
    }

    pub async fn update_import_stage(
        pool: &ApiWalletDbPool,
        uid: &str,
        import_stage: u8,
    ) -> Result<bool, crate::Error> {
        Self::with_write_guard(pool, "update_api_wallet_import_stage", || async {
            Ok(ApiWalletDao::update_import_stage(pool.write_ref(), uid, import_stage).await?)
        })
        .await
    }

    pub async fn physical_delete(
        pool: &ApiWalletDbPool,
        wallet_addresses: &[&str],
    ) -> Result<Vec<ApiWalletEntity>, crate::Error> {
        Self::with_write_guard(pool, "delete_api_wallets", || async {
            Ok(ApiWalletDao::physical_delete(pool.write_ref(), wallet_addresses).await?)
        })
        .await
    }

    pub async fn physical_delete_all_wallet(pool: &ApiWalletDbPool) -> Result<u64, crate::Error> {
        Self::with_write_guard(pool, "delete_all_api_wallets", || async {
            Ok(ApiWalletDao::physical_delete_all_wallet(pool.write_ref()).await?)
        })
        .await
    }

    pub async fn list(
        pool: &ApiWalletDbPool,
        api_wallet_type: Option<ApiWalletType>,
    ) -> Result<Vec<ApiWalletEntity>, crate::Error> {
        Ok(ApiWalletDao::list(pool.read_ref(), api_wallet_type).await?)
    }

    pub async fn find_by_address(
        pool: &ApiWalletDbPool,
        address: &str,
    ) -> Result<Option<ApiWalletEntity>, crate::Error> {
        Ok(ApiWalletDao::detail(pool.read_ref(), address).await?)
    }
    pub async fn find_by_uid(
        pool: &ApiWalletDbPool,
        uid: &str,
    ) -> Result<Option<ApiWalletEntity>, crate::Error> {
        Ok(ApiWalletDao::detail_by_uid(pool.read_ref(), uid).await?)
    }

    pub async fn bind_withdraw_and_subaccount_relation(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        binding_address: &str,
    ) -> Result<(), crate::Error> {
        Self::with_write_guard(pool, "bind_withdraw_relation", || async {
            ApiWalletDao::bind_withdraw_and_subaccount_relation(
                pool.write_ref(),
                binding_address,
                wallet_address,
            )
            .await?;
            ApiWalletDao::bind_withdraw_and_subaccount_relation(
                pool.write_ref(),
                wallet_address,
                binding_address,
            )
            .await
        })
        .await
    }

    pub async fn wallet_latest(
        pool: &ApiWalletDbPool,
    ) -> Result<Option<ApiWalletEntity>, crate::Error> {
        Ok(ApiWalletDao::wallet_latest(pool.read_ref()).await?)
    }

    // TODO: 想办法用ApiWalletDbPool替换executor
    pub async fn active_warning_config_bindings(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<(String, String)>, crate::Error> {
        ApiWalletDao::active_warning_config_bindings(pool.read_ref()).await
    }

    pub async fn active_withdrawal_uids(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<(String,)>, crate::Error> {
        ApiWalletDao::active_withdrawal_uids(pool.read_ref()).await
    }

    pub async fn uid_list(pool: &ApiWalletDbPool) -> Result<Vec<(String,)>, crate::Error> {
        Ok(ApiWalletDao::uid_list(pool.read_ref()).await?)
    }
}

#[cfg(test)]
#[path = "wallet/write_timing_tests.rs"]
mod write_timing_tests;

#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "wallet/wallet_tests.rs"]
mod tests;
