use crate::{
    ApiWalletDbPool, Error, dao::api_app_warning_config::ApiAppWarningConfigDao,
    db::sqlite_retry::with_sqlite_locked_retry,
};

/// Internal snapshots belong to API wallet binding lifetime, never to Core config.
pub struct ApiWarningConfigRepo;

impl ApiWarningConfigRepo {
    pub async fn find(
        pool: &ApiWalletDbPool,
        org: &str,
        app: &str,
    ) -> Result<Option<String>, Error> {
        Ok(Self::find_snapshot(pool, org, app).await?.map(|(_, value)| value))
    }

    pub async fn find_snapshot(
        pool: &ApiWalletDbPool,
        org: &str,
        app: &str,
    ) -> Result<Option<(String, String)>, Error> {
        ApiAppWarningConfigDao::find_snapshot(pool.read_ref(), org, app).await
    }

    /// Reserve the current ownership generation without overwriting a good snapshot.
    pub async fn begin_refresh(
        pool: &ApiWalletDbPool,
        org: &str,
        app: &str,
    ) -> Result<Option<String>, Error> {
        let _guard = pool.lock_write_with_metric("begin_warning_config_refresh").await;
        with_sqlite_locked_retry(|| async {
            let mut tx = pool.write_ref().begin().await.map_err(|e| Error::Database(e.into()))?;
            let generation =
                ApiAppWarningConfigDao::reserve_generation(tx.as_mut(), org, app).await?;
            tx.commit().await.map_err(|e| Error::Database(e.into()))?;
            Ok(generation)
        })
        .await
    }

    /// A last-owner unbind/delete removes this generation, rejecting in-flight replies.
    pub async fn publish(
        pool: &ApiWalletDbPool,
        org: &str,
        app: &str,
        generation: &str,
        value: &str,
    ) -> Result<bool, Error> {
        let _guard = pool.lock_write_with_metric("publish_warning_config").await;
        with_sqlite_locked_retry(|| async {
            ApiAppWarningConfigDao::publish(pool.write_ref(), org, app, generation, value).await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiWarningConfigRepo;
    use crate::{
        ApiWalletDbPool,
        entities::api_wallet::ApiWalletType,
        repositories::{api_wallet::wallet::ApiWalletRepo, test_helper::setup_api_wallet_pool},
    };

    async fn owner(pool: &ApiWalletDbPool, uid: &str, app: &str) {
        ApiWalletRepo::upsert(
            pool,
            uid,
            "fixture",
            uid,
            &[],
            &[],
            ApiWalletType::Withdrawal,
            None,
            "fixture",
            0,
        )
        .await
        .unwrap();
        ApiWalletRepo::update_merchant_id(pool, uid, "org").await.unwrap();
        ApiWalletRepo::update_app_id(pool, uid, Some(app)).await.unwrap();
    }
    async fn count(pool: &ApiWalletDbPool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM api_app_warning_config")
            .fetch_one(pool.read_ref())
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn warning_config_rebind_rejects_inflight_reply_and_preserves_failed_refresh() {
        let pool = setup_api_wallet_pool("warning_config_generation").await;
        assert!(ApiWarningConfigRepo::begin_refresh(&pool, "org", "app").await.unwrap().is_none());
        owner(&pool, "one", "app").await;
        let first =
            ApiWarningConfigRepo::begin_refresh(&pool, "org", "app").await.unwrap().unwrap();
        assert!(ApiWarningConfigRepo::find(&pool, "org", "app").await.unwrap().is_none());
        assert!(ApiWarningConfigRepo::publish(&pool, "org", "app", &first, "good").await.unwrap());
        let retry =
            ApiWarningConfigRepo::begin_refresh(&pool, "org", "app").await.unwrap().unwrap();
        assert_eq!(retry, first);
        assert_eq!(
            ApiWarningConfigRepo::find(&pool, "org", "app").await.unwrap().as_deref(),
            Some("good")
        );
        ApiWalletRepo::unbind_uid(&pool, "one").await.unwrap();
        owner(&pool, "one", "app").await;
        assert!(
            !ApiWarningConfigRepo::publish(&pool, "org", "app", &first, "stale").await.unwrap()
        );
        let second =
            ApiWarningConfigRepo::begin_refresh(&pool, "org", "app").await.unwrap().unwrap();
        assert_ne!(first, second);
        assert!(
            !ApiWarningConfigRepo::publish(&pool, "org", "app", &first, "stale").await.unwrap()
        );
        assert!(ApiWarningConfigRepo::find(&pool, "org", "app").await.unwrap().is_none());
        assert!(
            ApiWarningConfigRepo::publish(&pool, "org", "app", &second, "fresh").await.unwrap()
        );
    }

    #[tokio::test]
    async fn warning_config_cleanup_is_atomic_with_owner_changes_and_does_not_cross_scope() {
        let pool = setup_api_wallet_pool("warning_config_atomic").await;
        owner(&pool, "one", "app").await;
        owner(&pool, "other", "other-app").await;
        for app in ["app", "other-app"] {
            let generation =
                ApiWarningConfigRepo::begin_refresh(&pool, "org", app).await.unwrap().unwrap();
            ApiWarningConfigRepo::publish(&pool, "org", app, &generation, "snapshot")
                .await
                .unwrap();
        }
        let mut tx = pool.write_ref().begin().await.unwrap();
        sqlx::query("UPDATE api_wallet SET app_id = NULL WHERE uid = 'one'")
            .execute(tx.as_mut())
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_app_warning_config")
            .fetch_one(tx.as_mut())
            .await
            .unwrap();
        assert_eq!(n, 1);
        tx.rollback().await.unwrap();
        assert_eq!(count(&pool).await, 2);
        sqlx::query("UPDATE api_wallet SET status = 0 WHERE uid = 'one'")
            .execute(pool.write_ref())
            .await
            .unwrap();
        assert!(ApiWarningConfigRepo::find(&pool, "org", "app").await.unwrap().is_none());
        assert!(ApiWarningConfigRepo::find(&pool, "org", "other-app").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn warning_config_upgrade_discards_only_legacy_cache_and_preserves_other_core_config() {
        let pool = crate::repositories::test_helper::setup_core_pool("warning_config_legacy").await;
        let prefix = "api_balance_notify_config:";
        for key in
            [format!("{prefix}legacy"), "apiXbalance_notify_config:keep".into(), "other".into()]
        {
            crate::repositories::config::ConfigRepo::upsert(&key, "keep", Some(1), &pool)
                .await
                .unwrap();
        }
        // Replay the exact upgrade SQL against a legacy Core fixture.
        sqlx::raw_sql(include_str!(
            "../../../schema/core/migrations/20261027000000_remove_api_warning_cache.sql"
        ))
        .execute(pool.write_ref())
        .await
        .unwrap();
        assert!(
            crate::repositories::config::ConfigRepo::find_by_key(&format!("{prefix}legacy"), &pool)
                .await
                .unwrap()
                .is_none()
        );
        for key in ["other", "apiXbalance_notify_config:keep"] {
            assert!(
                crate::repositories::config::ConfigRepo::find_by_key(key, &pool)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[tokio::test]
    async fn warning_config_lifecycle_keeps_shared_scope_and_cleans_last_owner() {
        let pool = setup_api_wallet_pool("warning_config_lifecycle").await;
        owner(&pool, "one", "app").await;
        owner(&pool, "two", "app").await;
        sqlx::query("INSERT INTO api_app_warning_config (org_id, app_id, value) VALUES ('org','app','snapshot')").execute(pool.write_ref()).await.unwrap();
        ApiWalletRepo::unbind_uid(&pool, "one").await.unwrap();
        assert_eq!(count(&pool).await, 1, "another owner still uses this scope");
        ApiWalletRepo::unbind_uid(&pool, "two").await.unwrap();
        assert_eq!(count(&pool).await, 0, "last owner unbind invalidates cache");
        owner(&pool, "three", "app").await;
        sqlx::query("INSERT INTO api_app_warning_config (org_id, app_id, value) VALUES ('org','app','snapshot')").execute(pool.write_ref()).await.unwrap();
        ApiWalletRepo::physical_delete(&pool, &["three"]).await.unwrap();
        assert_eq!(count(&pool).await, 0);
        owner(&pool, "four", "app").await;
        sqlx::query("INSERT INTO api_app_warning_config (org_id, app_id, value) VALUES ('org','app','snapshot')").execute(pool.write_ref()).await.unwrap();
        ApiWalletRepo::physical_delete_all_wallet(&pool).await.unwrap();
        assert_eq!(count(&pool).await, 0, "reset removes cache");
    }
}
