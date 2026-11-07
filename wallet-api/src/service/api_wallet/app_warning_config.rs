use std::{collections::HashSet, future::Future, time::Duration};

use rust_decimal::Decimal;
use wallet_database::{
    entities::{
        api_business_notification::ApiBusinessNotification, asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{wallet::ApiWalletRepo, warning_config::ApiWarningConfigRepo},
};
use wallet_transport_backend::{
    request::api_wallet::wallet::AppConfigReq, response_vo::api_wallet::wallet::AppConfigResp,
};

use crate::{
    context::Context, error::service::ServiceError,
    messaging::mqtt::topics::api_wallet::address_insufficient::AwmAddressInsufficientBody,
};

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredConfig {
    config: AppConfigResp,
    /// Start of the successful snapshot request, milliseconds since Unix epoch.
    synced_at: i64,
}

pub(crate) enum BalanceConfigPolicy {
    Legacy,
    Configured { org_id: String, app_id: String, generation: String, threshold: Option<Decimal> },
}

impl BalanceConfigPolicy {
    pub(crate) fn disabled(&self) -> bool {
        matches!(self, Self::Configured { threshold: None, .. })
    }
}

pub(crate) struct AppWarningConfigService {
    ctx: &'static Context,
}

impl AppWarningConfigService {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    fn validate(config: &AppConfigResp) -> Result<(), ServiceError> {
        let mut seen = HashSet::new();
        for item in &config.configs.balance_notify_cfg_list {
            let chain = item.chain_code.trim().to_ascii_lowercase();
            if chain.is_empty() || item.amount < Decimal::ZERO {
                return Err(ServiceError::Parameter("invalid balance notification rule".into()));
            }
            let token =
                AssetTokenKey::from_raw(Some(&item.contract_address)).normalized_for_chain(&chain);
            if !seen.insert((chain.to_string(), token)) {
                return Err(ServiceError::Parameter("duplicate balance notification rule".into()));
            }
        }
        Ok(())
    }

    /// Called under address_balance_check, shared by config publication and evaluation.
    pub(crate) async fn policy_for(
        &self,
        event: &ApiBusinessNotification,
        body: &AwmAddressInsufficientBody,
    ) -> Result<BalanceConfigPolicy, ServiceError> {
        let Some(wallet) =
            ApiWalletRepo::find_by_uid(&self.ctx.api_wallet_pool()?, &event.uid).await?
        else {
            return Ok(BalanceConfigPolicy::Legacy);
        };
        let (Some(org_id), Some(app_id)) = (wallet.merchant_id, wallet.app_id) else {
            return Ok(BalanceConfigPolicy::Legacy);
        };
        let Some((generation, value)) =
            ApiWarningConfigRepo::find_snapshot(&self.ctx.api_wallet_pool()?, &org_id, &app_id)
                .await?
        else {
            return Ok(BalanceConfigPolicy::Legacy);
        };
        let stored: StoredConfig = serde_json::from_str(&value)
            .map_err(|_| ServiceError::Parameter("invalid stored warning configuration".into()))?;
        Self::validate(&stored.config)?;
        // A newly arrived warning may describe a later configuration than our cache.
        if stored.synced_at <= event.created_at.timestamp_millis() {
            return Ok(BalanceConfigPolicy::Legacy);
        }
        let token = AssetTokenKey::from_raw(Some(&body.token));
        let threshold = stored
            .config
            .configs
            .balance_notify_cfg_list
            .iter()
            .find(|item| {
                item.chain_code.trim().eq_ignore_ascii_case(&body.chain_code)
                    && AssetTokenKey::from_raw(Some(&item.contract_address))
                        .matches_for_chain(&token, &body.chain_code)
            })
            .filter(|item| item.status)
            .map(|item| item.amount);
        Ok(BalanceConfigPolicy::Configured { org_id, app_id, generation, threshold })
    }

    pub(crate) async fn sync(&self) -> Result<(), ServiceError> {
        let backend = self.ctx.get_global_backend_api();
        self.sync_using(|request| {
            let backend = backend.clone();
            async move { Ok(backend.query_app_config(&request).await?) }
        })
        .await
    }

    pub(crate) async fn sync_for_wallet_scope(
        &self,
        uid: &str,
        org: &str,
        app: &str,
    ) -> Result<(), ServiceError> {
        let backend = self.ctx.get_global_backend_api();
        self.sync_scoped_using(Some((uid, org, app)), |request| {
            let backend = backend.clone();
            async move { Ok(backend.query_app_config(&request).await?) }
        })
        .await
    }

    pub(crate) async fn sync_using<F, Fut>(&self, fetch: F) -> Result<(), ServiceError>
    where
        F: Fn(AppConfigReq) -> Fut,
        Fut: Future<Output = Result<AppConfigResp, ServiceError>>,
    {
        self.sync_scoped_using(None, fetch).await
    }

    async fn sync_scoped_using<F, Fut>(
        &self,
        scope: Option<(&str, &str, &str)>,
        fetch: F,
    ) -> Result<(), ServiceError>
    where
        F: Fn(AppConfigReq) -> Fut,
        Fut: Future<Output = Result<AppConfigResp, ServiceError>>,
    {
        let _sync = self.ctx.app_warning_config_sync.lock().await;
        if self.ctx.is_recovery_paused() {
            // A durable MQTT invalidation must remain retryable until published.
            return if scope.is_some() { Err(ServiceError::Timeout) } else { Ok(()) };
        }
        let pool = self.ctx.api_wallet_pool()?;
        let mut bindings = ApiWalletRepo::active_warning_config_bindings(&pool).await?;
        if let Some((uid, org, app)) = scope {
            let Some(wallet) = ApiWalletRepo::find_by_uid(&pool, uid).await? else {
                return Ok(());
            };
            if wallet.status != 1
                || wallet.api_wallet_type
                    != wallet_database::entities::api_wallet::ApiWalletType::Withdrawal
                || wallet.merchant_id.as_deref() != Some(org)
                || wallet.app_id.as_deref() != Some(app)
            {
                return Ok(());
            }
            bindings.retain(|(o, a)| o == org && a == app);
        }
        // Capture before network I/O, never discover newly arrived events after a fetch.
        let events = super::business_notification::ApiBusinessNotificationService::new(self.ctx)
            .unresolved_address_balance_notifications()
            .await?;
        let mut first_error = None;
        for (org_id, app_id) in bindings {
            let result = async {
                let Some(generation) =
                    ApiWarningConfigRepo::begin_refresh(&pool, &org_id, &app_id).await?
                else {
                    return Ok(());
                };
                let observed_at = chrono::Utc::now().timestamp_millis();
                let config = tokio::time::timeout(
                    Duration::from_secs(3),
                    fetch(AppConfigReq { app_id: app_id.clone(), org_id: org_id.clone() }),
                )
                .await
                .map_err(|_| ServiceError::Timeout)??;
                Self::validate(&config)?;
                if self.ctx.is_recovery_paused() {
                    return if scope.is_some() { Err(ServiceError::Timeout) } else { Ok(()) };
                }
                // Publish atomically in the API wallet ownership generation. A cancelled or failed
                // fetch never replaces the previous snapshot, including with an empty list.
                let stored = StoredConfig { config, synced_at: observed_at };
                let value = serde_json::to_string(&stored).map_err(|_| {
                    ServiceError::Parameter("warning configuration serialization failed".into())
                })?;
                {
                    let _checks = self.ctx.address_balance_check.lock().await;
                    if !ApiWarningConfigRepo::publish(&pool, &org_id, &app_id, &generation, &value)
                        .await?
                    {
                        // Last-owner removal invalidates even a same-scope rebind.
                        return Err(ServiceError::Timeout);
                    }
                }
                let episode = format!("config-{}", uuid::Uuid::new_v4());
                for event in &events {
                    let Some(wallet) = ApiWalletRepo::find_by_uid(&pool, &event.uid).await? else {
                        continue;
                    };
                    if wallet.merchant_id.as_deref() == Some(org_id.as_str())
                        && wallet.app_id.as_deref() == Some(app_id.as_str())
                    {
                        crate::application::api_wallet_balance_check::enqueue(
                            self.ctx,
                            event,
                            Some(&episode),
                        )
                        .await?;
                    }
                }
                Ok::<(), ServiceError>(())
            }
            .await;
            if let Err(error) = result {
                // Do not log a whole config/response: it may contain unrelated credentials.
                tracing::warn!("warning configuration refresh failed; retain previous snapshot");
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wallet_database::{entities::api_wallet::ApiWalletType, repositories::config::ConfigRepo};

    async fn owner(ctx: &'static Context, uid: &str, org: &str, app: &str) {
        let pool = ctx.api_wallet_pool().unwrap();
        ApiWalletRepo::upsert(
            &pool,
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
        ApiWalletRepo::update_merchant_id(&pool, uid, org).await.unwrap();
        ApiWalletRepo::update_app_id(&pool, uid, Some(app)).await.unwrap();
    }
    fn empty() -> AppConfigResp {
        serde_json::from_value(serde_json::json!({"configs":{"balanceNotifyCfgList":[]}})).unwrap()
    }
    async fn value(ctx: &'static Context, org: &str, app: &str) -> Option<String> {
        ApiWarningConfigRepo::find(&ctx.api_wallet_pool().unwrap(), org, app).await.unwrap()
    }

    #[tokio::test]
    async fn cache_lives_in_api_database_and_stale_rebind_reply_cannot_recreate_it() {
        let (_dir, ctx) = crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "org", "app").await;
        let service = AppWarningConfigService::new(ctx);
        service.sync_using(|_| std::future::ready(Ok(empty()))).await.unwrap();
        assert!(
            wallet_database::repositories::api_wallet::warning_config::ApiWarningConfigRepo::find(
                &ctx.api_wallet_pool().unwrap(),
                "org",
                "app"
            )
            .await
            .unwrap()
            .is_some()
        );
        assert!(
            ConfigRepo::find_by_key(
                "api_balance_notify_config:[\"org\",\"app\"]",
                &ctx.core_pool().unwrap()
            )
            .await
            .unwrap()
            .is_none()
        );
        let result = service
            .sync_using(|_| async move {
                ApiWalletRepo::unbind_uid(&ctx.api_wallet_pool().unwrap(), "one").await.unwrap();
                owner(ctx, "one", "org", "app").await;
                Ok(empty())
            })
            .await;
        assert!(result.is_err(), "reply from removed ownership generation must not publish");
        assert!(
            wallet_database::repositories::api_wallet::warning_config::ApiWarningConfigRepo::find(
                &ctx.api_wallet_pool().unwrap(),
                "org",
                "app"
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn mqtt_scope_paused_refresh_is_retryable_and_recovers_after_resume() {
        let (_dir, ctx) = crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "org", "app").await;
        let service = AppWarningConfigService::new(ctx);
        ctx.set_recovery_paused(true);
        assert!(
            service
                .sync_scoped_using(Some(("one", "org", "app")), |_| {
                    panic!("paused refresh must not query");
                    #[allow(unreachable_code)]
                    std::future::ready(Ok(empty()))
                })
                .await
                .is_err()
        );
        ctx.set_recovery_paused(false);
        assert!(
            service
                .sync_scoped_using(Some(("one", "org", "app")), |_| {
                    ctx.set_recovery_paused(true);
                    std::future::ready(Ok(empty()))
                })
                .await
                .is_err()
        );
        assert!(value(ctx, "org", "app").await.is_none());
        ctx.set_recovery_paused(false);
        service
            .sync_scoped_using(Some(("one", "org", "app")), |_| std::future::ready(Ok(empty())))
            .await
            .unwrap();
        assert!(value(ctx, "org", "app").await.is_some());
    }

    #[tokio::test]
    async fn mqtt_scope_fetches_only_current_bound_application_and_rejects_stale_owner() {
        let (_dir, ctx) = crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "org", "app").await;
        owner(ctx, "two", "other", "other-app").await;
        let calls = AtomicUsize::new(0);
        let service = AppWarningConfigService::new(ctx);
        service
            .sync_scoped_using(Some(("one", "org", "app")), |req| {
                assert_eq!((req.org_id.as_str(), req.app_id.as_str()), ("org", "app"));
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(empty()))
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(value(ctx, "other", "other-app").await.is_none());
        for scope in [("missing", "org", "app"), ("one", "other", "other-app")] {
            service
                .sync_scoped_using(Some(scope), |_| {
                    panic!("stale or unrelated wallet must not fetch configuration");
                    #[allow(unreachable_code)]
                    std::future::ready(Ok(empty()))
                })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn sync_deduplicates_bound_wallets_and_persists_snapshot_for_next_service_instance() {
        let (_dir,ctx)=crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "merchant", "app").await;
        owner(ctx, "two", "merchant", "app").await;
        let pool = ctx.api_wallet_pool().unwrap();
        ApiWalletRepo::upsert(
            &pool,
            "sub",
            "fixture",
            "sub",
            &[],
            &[],
            ApiWalletType::SubAccount,
            None,
            "fixture",
            0,
        )
        .await
        .unwrap();
        ApiWalletRepo::update_merchant_id(&pool, "sub", "excluded").await.unwrap();
        ApiWalletRepo::update_app_id(&pool, "sub", Some("excluded")).await.unwrap();
        owner(ctx, "disabled", "excluded", "excluded").await;
        sqlx::query("UPDATE api_wallet SET status = 0 WHERE address = 'disabled'")
            .execute(pool.write_ref())
            .await
            .unwrap();
        let calls = AtomicUsize::new(0);
        AppWarningConfigService::new(ctx)
            .sync_using(|request| {
                assert_eq!((request.org_id.as_str(), request.app_id.as_str()), ("merchant", "app"));
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(empty()))
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let persisted = value(ctx, "merchant", "app").await.unwrap();
        let decoded: StoredConfig = serde_json::from_str(&persisted).unwrap();
        assert!(decoded.config.configs.balance_notify_cfg_list.is_empty());
        assert!(decoded.synced_at > 0);
        assert!(
            ConfigRepo::list_v2(&ctx.core_pool().unwrap())
                .await
                .unwrap()
                .iter()
                .all(|row| !row.key.starts_with("api_balance_notify_config:")),
            "internal merchant snapshot must not appear in frontend generic configuration"
        );
        assert_eq!(value(ctx, "merchant", "app").await.unwrap(), persisted);
    }

    #[tokio::test]
    async fn failed_invalid_and_cancelled_refresh_keep_last_successful_snapshot() {
        let (_dir,ctx)=crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "merchant", "app").await;
        let service = AppWarningConfigService::new(ctx);
        service.sync_using(|_| std::future::ready(Ok(empty()))).await.unwrap();
        let original = value(ctx, "merchant", "app").await.unwrap();
        assert!(
            service.sync_using(|_| std::future::ready(Err(ServiceError::Timeout))).await.is_err()
        );
        let invalid:AppConfigResp=serde_json::from_value(serde_json::json!({"configs":{"balanceNotifyCfgList":[{"amount":-1,"chainCode":"polygon","contractAddress":"","status":true,"tokenCode":"pol"}]}})).unwrap();
        assert!(service.sync_using(|_| std::future::ready(Ok(invalid.clone()))).await.is_err());
        assert_eq!(value(ctx, "merchant", "app").await.unwrap(), original);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(5),
                service
                    .sync_using(|_| std::future::pending::<Result<AppConfigResp, ServiceError>>())
            )
            .await
            .is_err()
        );
        assert_eq!(value(ctx, "merchant", "app").await.unwrap(), original);
        service.sync_using(|_| std::future::ready(Ok(empty()))).await.unwrap();
    }

    #[tokio::test]
    async fn overlapping_refreshes_are_serialized_and_one_failed_merchant_does_not_block_another() {
        let (_dir,ctx)=crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        owner(ctx, "one", "a", "app").await;
        owner(ctx, "two", "b", "app").await;
        let service = AppWarningConfigService::new(ctx);
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let fetch = |request: AppConfigReq| {
            let active = &active;
            let peak = &peak;
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(3)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                if request.org_id == "a" { Err(ServiceError::Timeout) } else { Ok(empty()) }
            }
        };
        let (first, second) = tokio::join!(service.sync_using(&fetch), service.sync_using(&fetch));
        assert!(first.is_err() && second.is_err());
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert!(value(ctx, "a", "app").await.is_none());
        assert!(value(ctx, "b", "app").await.is_some());
    }
}

#[cfg(test)]
mod arrival_timing_tests {
    use super::*;
    use wallet_database::{
        entities::{
            api_business_notification::NewApiBusinessNotification, api_wallet::ApiWalletType,
        },
        repositories::api_business_notification::ApiBusinessNotificationRepo,
    };
    #[tokio::test]
    async fn notification_arriving_during_fetch_does_not_use_older_snapshot_response() {
        let (_dir,ctx)=crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        let pool = ctx.api_wallet_pool().unwrap();
        ApiWalletRepo::upsert(
            &pool,
            "owner",
            "fixture",
            "wallet",
            &[],
            &[],
            ApiWalletType::Withdrawal,
            None,
            "fixture",
            0,
        )
        .await
        .unwrap();
        ApiWalletRepo::update_merchant_id(&pool, "wallet", "merchant").await.unwrap();
        ApiWalletRepo::update_app_id(&pool, "wallet", Some("app")).await.unwrap();
        AppWarningConfigService::new(ctx).sync_using(|_| async {
            // The backend snapshot request is already in flight when this event arrives.
            tokio::time::sleep(Duration::from_millis(3)).await;
            let payload=serde_json::json!({"remaining":"1","thresholdValue":"20","token":"","symbol":"POL","merchantName":"fixture","waddressType":"1","chainCode":"polygon","targetAddress":"address","notifiedAt":chrono::Utc::now()}).to_string();
            ApiBusinessNotificationRepo::upsert(&pool,NewApiBusinessNotification{id:"arrived",uid:"owner",kind:"AWM_ADDRESS_INSUFFICIENT",business_key:"arrived",payload:&payload}).await?;
            Ok(serde_json::from_value(serde_json::json!({"configs":{"balanceNotifyCfgList":[]}})).unwrap())
        }).await.unwrap();
        let row =
            ApiBusinessNotificationRepo::find(&pool, "owner", "arrived").await.unwrap().unwrap();
        let body = serde_json::from_str(&row.payload).unwrap();
        assert!(matches!(
            AppWarningConfigService::new(ctx).policy_for(&row, &body).await.unwrap(),
            BalanceConfigPolicy::Legacy
        ));
        assert!(row.resolved_at.is_none());
    }
}
