use crate::{context::Context, error::service::ServiceError};

use rust_decimal::Decimal;
use std::{
    collections::HashMap,
    str::FromStr,
    time::{Duration, Instant},
};
use wallet_database::{
    ApiWalletDbPool, entities::api_business_notification::ApiBusinessNotification,
    pagination::Pagination, repositories::api_business_notification::ApiBusinessNotificationRepo,
};

use super::asset::ApiAssetsService;
use crate::messaging::mqtt::{
    message::BizType, topics::api_wallet::address_insufficient::AwmAddressInsufficientBody,
};

const BALANCE_CHECK_BUFFER: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct AddressBalanceCheckCache {
    checked: HashMap<(String, String), BalanceCheckSnapshot>,
}

struct BalanceCheckSnapshot {
    completed_at: Instant,
    payload: String,
    balance: Result<Decimal, String>,
    observed_events: Vec<ApiBusinessNotification>,
}

pub struct ApiBusinessNotificationService {
    ctx: &'static Context,
}

impl ApiBusinessNotificationService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    /// Read active withdrawal-wallet history without marking read or refreshing balances.
    pub async fn detail_for_withdrawal_wallets(
        &self,
        id: &str,
    ) -> Result<
        Option<
            crate::response_vo::api_wallet::business_notification::ApiBusinessNotificationDetail,
        >,
        ServiceError,
    > {
        if id.trim().is_empty() {
            return Err(ServiceError::Parameter("notification id is empty".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        ApiBusinessNotificationRepo::find_for_withdrawal_wallets(&pool, id).await?
            .map(crate::response_vo::api_wallet::business_notification::ApiBusinessNotificationDetail::from_record)
            .transpose()
    }

    /// Pure local banner read, one latest unresolved event per business object.
    pub async fn unresolved_address_balance_notifications(
        &self,
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        Ok(Self::latest_unresolved_events(self.address_balance_notification_events().await?))
    }

    /// Read all history so a resolved latest event can suppress older unresolved rows.
    pub(crate) async fn address_balance_notification_events(
        &self,
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        use wallet_database::repositories::api_wallet::wallet::ApiWalletRepo;
        let pool = self.ctx.api_wallet_pool()?;
        let mut items = Vec::new();
        for (uid,) in ApiWalletRepo::active_withdrawal_uids(&pool).await? {
            items.extend(
                self.all_events(&uid)
                    .await?
                    .into_iter()
                    .filter(|item| BizType::from_business_notification_kind(&item.kind).is_some()),
            );
        }
        Ok(items)
    }

    fn balance_object_key(
        row: &ApiBusinessNotification,
    ) -> Option<(String, String, String, String, String)> {
        let detail = crate::response_vo::api_wallet::business_notification::ApiBusinessNotificationDetail::from_record(row.clone()).ok()?;
        Some((
            row.uid.clone(),
            row.kind.clone(),
            detail.chain_code,
            detail.target_address,
            detail.token,
        ))
    }

    fn latest_unresolved_events(
        events: Vec<ApiBusinessNotification>,
    ) -> Vec<ApiBusinessNotification> {
        Self::event_groups(events)
            .into_iter()
            .filter_map(|group| {
                let latest = group.into_iter().next()?;
                latest.resolved_at.is_none().then_some(latest)
            })
            .collect()
    }

    // Newest first, including resolved events: never fall back to an older threshold.
    fn event_groups(events: Vec<ApiBusinessNotification>) -> Vec<Vec<ApiBusinessNotification>> {
        let mut groups: HashMap<_, Vec<ApiBusinessNotification>> = HashMap::new();
        for row in events {
            if let Some(key) = Self::balance_object_key(&row) {
                groups.entry(key).or_default().push(row);
            }
        }
        let mut groups: Vec<_> = groups.into_values().collect();
        for group in &mut groups {
            group.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
        }
        groups.sort_by(|a, b| {
            b[0].created_at.cmp(&a[0].created_at).then_with(|| b[0].id.cmp(&a[0].id))
        });
        groups
    }

    #[cfg(test)]
    async fn unresolved_balance_notifications_for_uid_using<C, F, Fut>(
        &self,
        uid: &str,
        clock: C,
        balance_for: F,
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError>
    where
        C: Fn() -> Instant,
        F: Fn(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        if uid.trim().is_empty() {
            return Err(ServiceError::Parameter("uid is empty".into()));
        }
        self.refresh_balance_notifications_using(uid, clock, balance_for).await?;
        Ok(self
            .all_unresolved(uid)
            .await?
            .into_iter()
            .filter(|item| BizType::from_business_notification_kind(&item.kind).is_some())
            .collect())
    }

    async fn all_events(&self, uid: &str) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        self.all_events_filtered(uid, false).await
    }

    #[cfg(test)]
    async fn all_unresolved(
        &self,
        uid: &str,
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        self.all_events_filtered(uid, true).await
    }

    async fn all_events_filtered(
        &self,
        uid: &str,
        unresolved_only: bool,
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        let mut items = Vec::new();
        let mut page = 0;
        loop {
            let batch = self.list(uid, unresolved_only, page, 100).await?;
            let count = batch.data.len();
            items.extend(batch.data);
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(items)
    }

    pub(crate) async fn check_address_balance(
        &self,
        check: &crate::application::api_wallet_balance_check::ApiAddressBalanceCheck,
    ) -> Result<(), ServiceError> {
        self.check_address_balance_using(check, Instant::now, |address, chain, token| async move {
            let value =
                ApiAssetsService::new(self.ctx).chain_balance(&address, &chain, &token).await?;
            Decimal::from_str(&value.balance).map_err(|e| ServiceError::Parameter(e.to_string()))
        })
        .await
    }

    pub(crate) async fn check_address_balance_using<C, F, Fut>(
        &self,
        check: &crate::application::api_wallet_balance_check::ApiAddressBalanceCheck,
        clock: C,
        balance_for: F,
    ) -> Result<(), ServiceError>
    where
        C: Fn() -> Instant,
        F: Fn(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        use wallet_database::repositories::api_wallet::wallet::ApiWalletRepo;
        let pool = self.ctx.api_wallet_pool()?;
        if !ApiWalletRepo::active_withdrawal_uids(&pool)
            .await?
            .iter()
            .any(|(uid,)| uid == &check.uid)
        {
            return Ok(());
        }
        let Some(row) =
            ApiBusinessNotificationRepo::find(&pool, &check.uid, &check.notification_id).await?
        else {
            return Ok(());
        };
        self.check_record_using(&row, &check.payload, clock, balance_for).await
    }

    async fn check_record_using<C, F, Fut>(
        &self,
        row: &ApiBusinessNotification,
        expected_payload: &str,
        clock: C,
        balance_for: F,
    ) -> Result<(), ServiceError>
    where
        C: Fn() -> Instant,
        F: Fn(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        let mut cache = self.ctx.address_balance_check.lock().await;
        let pool = self.ctx.api_wallet_pool()?;
        // Reload after waiting: a newer MQTT payload may have replaced this task.
        let Some(current) = ApiBusinessNotificationRepo::find(&pool, &row.uid, &row.id).await?
        else {
            return Ok(());
        };
        if current.resolved_at.is_some()
            || BizType::from_business_notification_kind(&current.kind).is_none()
            || current.payload != expected_payload
        {
            return Ok(());
        }
        let body: AwmAddressInsufficientBody = serde_json::from_str(&current.payload)
            .map_err(|e| ServiceError::Parameter(format!("invalid balance alert: {e}")))?;
        let Some(object_key) = Self::balance_object_key(&current) else {
            return Ok(());
        };
        let Some(candidates) = Self::event_groups(self.all_events(&current.uid).await?)
            .into_iter()
            .find(|group| Self::balance_object_key(&group[0]).as_ref() == Some(&object_key))
        else {
            return Ok(());
        };
        if candidates[0].id != current.id || candidates[0].resolved_at.is_some() {
            return Ok(());
        }
        let policy = super::app_warning_config::AppWarningConfigService::new(self.ctx)
            .policy_for(&current, &body)
            .await?;
        if policy.disabled() {
            let changed = Self::apply_policy_balance_result(
                &pool,
                &candidates,
                &body,
                Err(ServiceError::Timeout),
                &policy,
            )
            .await?;
            self.publish_resolution_changed(changed).await;
            return Ok(());
        }
        let key = (current.uid.clone(), current.id.clone());
        let cached = cache
            .checked
            .get(&key)
            .filter(|s| {
                s.payload == current.payload
                    && clock().saturating_duration_since(s.completed_at) < BALANCE_CHECK_BUFFER
            })
            .map(|s| (s.balance.clone(), s.completed_at, s.observed_events.clone()));
        let cached_completed_at = cached.as_ref().map(|(_, completed_at, _)| *completed_at);
        let observed_events =
            cached.as_ref().map(|(_, _, events)| events.clone()).unwrap_or(candidates);
        let balance = if let Some((balance, _, _)) = cached {
            balance.map_err(|error| {
                ServiceError::Parameter(format!("recent balance check failed: {error}"))
            })?
        } else {
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                balance_for(body.address.clone(), body.chain_code.clone(), body.token.clone()),
            )
            .await
            .unwrap_or(Err(ServiceError::Timeout));
            cache.checked.insert(
                key.clone(),
                BalanceCheckSnapshot {
                    completed_at: clock(),
                    payload: current.payload.clone(),
                    balance: result.as_ref().map(|value| *value).map_err(ToString::to_string),
                    observed_events: observed_events.clone(),
                },
            );
            result?
        };
        // Cached compensation reuses the original group snapshot, never new siblings.
        let observed_at = cached_completed_at.unwrap_or_else(&clock);
        cache.checked.insert(
            key,
            BalanceCheckSnapshot {
                completed_at: observed_at,
                payload: current.payload.clone(),
                balance: Ok(balance),
                observed_events: observed_events.clone(),
            },
        );
        let changed =
            Self::apply_policy_balance_result(&pool, &observed_events, &body, Ok(balance), &policy)
                .await?;
        self.publish_resolution_changed(changed).await;
        Ok(())
    }

    #[cfg(test)]
    async fn refresh_balance_notifications_using<C, F, Fut>(
        &self,
        uid: &str,
        clock: C,
        balance_for: F,
    ) -> Result<(), ServiceError>
    where
        C: Fn() -> Instant,
        F: Fn(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        for row in self.all_unresolved(uid).await? {
            if let Err(error) =
                self.check_record_using(&row, &row.payload, &clock, &balance_for).await
            {
                if matches!(error, ServiceError::Database(_)) {
                    return Err(error);
                }
                tracing::warn!(%error, "balance check failed; reminder remains active");
            }
        }
        Ok(())
    }

    /// Capture event identities before asset RPC. Reconciliation must not discover
    /// new events after the observation has already started.
    pub(crate) async fn snapshot_balance_events(
        &self,
        assets: &[wallet_database::entities::assets::AssetsId],
    ) -> Result<Vec<ApiBusinessNotification>, ServiceError> {
        use wallet_database::entities::asset_token_key::AssetTokenKey;
        let assets: std::collections::HashSet<_> = assets
            .iter()
            .map(|id| {
                (
                    id.address.clone(),
                    id.chain_code.clone(),
                    id.token_address.normalized_for_chain(&id.chain_code),
                )
            })
            .collect();
        Ok(self
            .address_balance_notification_events()
            .await?
            .into_iter()
            .filter(|event| {
                Self::balance_object_key(event).is_some_and(|(_, _, chain, address, token)| {
                    let token = AssetTokenKey::from_raw(Some(&token)).normalized_for_chain(&chain);
                    assets.contains(&(address, chain, token))
                })
            })
            .collect())
    }

    /// Reconcile only observations whose asset writes succeeded. This shares the
    /// balance-check lock so a buffered/in-flight query cannot hide the resolution.
    /// The buffer stores checked payloads, not items: resolved rows disappear from
    /// the next query without another RPC, even inside the five-second window.
    /// Return only assets whose notification work has no durable compensation.
    pub(crate) async fn resolve_from_persisted_balances(
        &self,
        uid: &str,
        balances: &[(wallet_database::entities::assets::AssetsId, String)],
        observed_events: &[ApiBusinessNotification],
    ) -> Result<Vec<wallet_database::entities::assets::AssetsId>, ServiceError> {
        use wallet_database::entities::asset_token_key::AssetTokenKey;
        if uid.trim().is_empty() || balances.is_empty() {
            return Ok(Vec::new());
        }
        let mut cache = self.ctx.address_balance_check.lock().await;
        let pool = self.ctx.api_wallet_pool()?;
        let observed: HashMap<_, _> = balances
            .iter()
            .map(|(id, balance)| {
                (
                    (
                        id.address.as_str(),
                        id.chain_code.as_str(),
                        id.token_address.normalized_for_chain(&id.chain_code),
                    ),
                    (id, balance.as_str()),
                )
            })
            .collect();
        let mut retry_assets = Vec::new();
        let mut changed = false;
        let episode =
            format!("sync-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
        for group in Self::event_groups(
            observed_events.iter().filter(|item| item.uid == uid).cloned().collect(),
        ) {
            let item = &group[0];
            if item.resolved_at.is_some()
                || BizType::from_business_notification_kind(&item.kind).is_none()
            {
                continue;
            }
            let Ok(body) = serde_json::from_str::<AwmAddressInsufficientBody>(&item.payload) else {
                continue;
            };
            let token =
                AssetTokenKey::from_raw(Some(&body.token)).normalized_for_chain(&body.chain_code);
            let observed_balance =
                observed.get(&(body.address.as_str(), body.chain_code.as_str(), token));
            let result = async {
                let policy = super::app_warning_config::AppWarningConfigService::new(self.ctx)
                    .policy_for(item, &body)
                    .await?;
                if policy.disabled() {
                    return Self::apply_policy_balance_result(
                        &pool, &group, &body, Err(ServiceError::Timeout), &policy,
                    ).await;
                }
                let Some((_, balance)) = observed_balance else {
                    return Ok(false);
                };
                let Ok(balance) = Decimal::from_str(balance) else {
                    tracing::warn!(id = %item.id, "invalid persisted balance; reminder remains active");
                    return Ok(false);
                };
                cache.checked.insert(
                    (item.uid.clone(), item.id.clone()),
                    BalanceCheckSnapshot {
                        completed_at: Instant::now(),
                        payload: item.payload.clone(),
                        balance: Ok(balance),
                        observed_events: group.clone(),
                    },
                );
                Self::apply_policy_balance_result(&pool, &group, &body, Ok(balance), &policy).await
            }.await;
            match result {
                Ok(resolved) => changed |= resolved,
                Err(error) => {
                    tracing::warn!(%error, id = %item.id, "balance reminder reconciliation failed");
                    // Task DB is separate. Only persisted compensation permits this
                    // asset's sync state to complete; dispatch alone is not authority.
                    if let Err(enqueue_error) =
                        crate::application::api_wallet_balance_check::enqueue(
                            self.ctx,
                            item,
                            Some(&episode),
                        )
                        .await
                    {
                        tracing::warn!(%enqueue_error, "reminder compensation storage failed; retain dirty asset");
                        if let Some((asset_id, _)) = observed_balance {
                            retry_assets.push((*asset_id).clone());
                        }
                    }
                }
            }
        }
        drop(cache);
        // Publish even when another group failed: its committed transitions survive.
        self.publish_resolution_changed(changed).await;
        Ok(retry_assets)
    }

    pub async fn list_for_withdrawal_wallets(
        &self,
        unresolved_only: bool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError> {
        if page < 0 || page_size <= 0 || page.checked_mul(page_size).is_none() {
            return Err(ServiceError::Parameter("page or page_size is invalid".into()));
        }
        ApiBusinessNotificationRepo::list_for_withdrawal_wallets(
            &self.ctx.api_wallet_pool()?,
            unresolved_only,
            page,
            page_size,
        )
        .await
        .map_err(ServiceError::from)
    }

    pub async fn unread_count_for_withdrawal_wallets(&self) -> Result<i64, ServiceError> {
        ApiBusinessNotificationRepo::unread_count_for_withdrawal_wallets(
            &self.ctx.api_wallet_pool()?,
        )
        .await
        .map_err(ServiceError::from)
    }

    pub async fn mark_read_for_withdrawal_wallets(&self, id: &str) -> Result<(), ServiceError> {
        if id.trim().is_empty() {
            return Err(ServiceError::Parameter("notification id is empty".into()));
        }
        ApiBusinessNotificationRepo::mark_read_for_withdrawal_wallets(
            &self.ctx.api_wallet_pool()?,
            id,
        )
        .await
        .map_err(ServiceError::from)
    }

    pub async fn mark_all_read_for_withdrawal_wallets(&self) -> Result<(), ServiceError> {
        ApiBusinessNotificationRepo::mark_all_read_for_withdrawal_wallets(
            &self.ctx.api_wallet_pool()?,
        )
        .await
        .map_err(ServiceError::from)
    }

    pub async fn list_unresolved_for_withdrawal_wallets_with_refresh(
        &self,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError> {
        self.list_unresolved_scoped_with_refresh_using(
            None,
            page,
            page_size,
            |address, chain, token| async move {
                let value =
                    ApiAssetsService::new(self.ctx).chain_balance(&address, &chain, &token).await?;
                Decimal::from_str(&value.balance).map_err(|error| {
                    ServiceError::Parameter(format!("invalid current balance: {error}"))
                })
            },
        )
        .await
    }

    async fn list_in_scope(
        &self,
        uid: Option<&str>,
        unresolved_only: bool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError> {
        match uid {
            Some(uid) => self.list(uid, unresolved_only, page, page_size).await,
            None => self.list_for_withdrawal_wallets(unresolved_only, page, page_size).await,
        }
    }

    pub async fn list(
        &self,
        uid: &str,
        unresolved_only: bool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError> {
        if uid.is_empty() || page < 0 || page_size <= 0 {
            return Err(ServiceError::Parameter("uid, page, or page_size is invalid".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        ApiBusinessNotificationRepo::list(&pool, uid, unresolved_only, page, page_size)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn unread_count(&self, uid: &str) -> Result<i64, ServiceError> {
        if uid.trim().is_empty() {
            return Err(ServiceError::Parameter("uid is empty".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        ApiBusinessNotificationRepo::unread_count(&pool, uid).await.map_err(ServiceError::from)
    }

    pub async fn mark_all_read(&self, uid: &str) -> Result<(), ServiceError> {
        if uid.trim().is_empty() {
            return Err(ServiceError::Parameter("uid is empty".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        ApiBusinessNotificationRepo::mark_all_read(&pool, uid).await.map_err(ServiceError::from)
    }

    pub async fn mark_read(&self, uid: &str, id: &str) -> Result<(), ServiceError> {
        if uid.is_empty() || id.is_empty() {
            return Err(ServiceError::Parameter("uid or notification id is empty".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        ApiBusinessNotificationRepo::mark_read(&pool, uid, id).await.map_err(ServiceError::from)
    }

    pub async fn list_unresolved_with_refresh(
        &self,
        uid: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError> {
        self.list_unresolved_with_refresh_using(
            uid,
            page,
            page_size,
            |address, chain, token| async move {
                let value =
                    ApiAssetsService::new(self.ctx).chain_balance(&address, &chain, &token).await?;
                Decimal::from_str(&value.balance).map_err(|error| {
                    ServiceError::Parameter(format!("invalid current balance: {error}"))
                })
            },
        )
        .await
    }

    async fn list_unresolved_with_refresh_using<F, Fut>(
        &self,
        uid: &str,
        page: i64,
        page_size: i64,
        balance_for: F,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError>
    where
        F: FnMut(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        self.list_unresolved_scoped_with_refresh_using(Some(uid), page, page_size, balance_for)
            .await
    }

    async fn list_unresolved_scoped_with_refresh_using<F, Fut>(
        &self,
        uid: Option<&str>,
        page: i64,
        page_size: i64,
        mut balance_for: F,
    ) -> Result<Pagination<ApiBusinessNotification>, ServiceError>
    where
        F: FnMut(String, String, String) -> Fut,
        Fut: std::future::Future<Output = Result<Decimal, ServiceError>>,
    {
        // Validate before IO; aggregate the whole eligible scope before slicing pages.
        if page < 0
            || page_size <= 0
            || page.checked_mul(page_size).is_none()
            || uid.is_some_and(str::is_empty)
        {
            return Err(ServiceError::Parameter("page or page_size is invalid".into()));
        }
        let pool = self.ctx.api_wallet_pool()?;
        let mut changed = false;
        let result = async {
        // Snapshot candidates before resolution changes their pagination offsets.
        let mut candidates = Vec::new();
        let mut scan_page = 0;
        loop {
            let batch = self.list_in_scope(uid, false, scan_page, 100).await?;
            let count = batch.data.len();
            candidates.extend(batch.data);
            if count < 100 {
                break;
            }
            scan_page += 1;
        }
        for group in Self::event_groups(candidates) {
            let notification = &group[0];
            if notification.resolved_at.is_some() {
                continue;
            }
            let body: AwmAddressInsufficientBody = serde_json::from_str(&notification.payload)
                .map_err(|e| ServiceError::Parameter(e.to_string()))?;
            let _checks = self.ctx.address_balance_check.lock().await;
            let policy = super::app_warning_config::AppWarningConfigService::new(self.ctx)
                .policy_for(notification, &body)
                .await?;
            if policy.disabled() {
                changed |= Self::apply_policy_balance_result(
                    &pool,
                    &group,
                    &body,
                    Err(ServiceError::Timeout),
                    &policy,
                )
                .await?;
                continue;
            }
            match balance_for(body.address.clone(), body.chain_code.clone(), body.token.clone())
                .await
            {
                Ok(balance) => {
                    changed |= Self::apply_policy_balance_result(&pool, &group, &body, Ok(balance), &policy)
                        .await?
                }
                Err(error) => {
                    tracing::warn!(id = %notification.id, %error, "balance query failed; reminder remains active")
                }
            }
        }
        // Re-read after resolution; do not let pagination discard objects before grouping.
        let mut remaining = Vec::new();
        let mut scan_page = 0;
        loop {
            let batch = self.list_in_scope(uid, false, scan_page, 100).await?;
            let count = batch.data.len();
            remaining.extend(batch.data);
            if count < 100 {
                break;
            }
            scan_page += 1;
        }
        let rows = Self::latest_unresolved_events(remaining);
        let total_count = rows.len() as i64;
        let offset = usize::try_from(page * page_size).unwrap_or(usize::MAX);
        let limit = usize::try_from(page_size).unwrap_or(usize::MAX);
        Ok(Pagination {
            page,
            page_size,
            total_count,
            data: rows.into_iter().skip(offset).take(limit).collect(),
        })
        }.await;
        self.publish_resolution_changed(changed).await;
        result
    }

    async fn apply_policy_balance_result(
        pool: &ApiWalletDbPool,
        observed_events: &[ApiBusinessNotification],
        body: &AwmAddressInsufficientBody,
        balance: Result<Decimal, ServiceError>,
        policy: &super::app_warning_config::BalanceConfigPolicy,
    ) -> Result<bool, ServiceError> {
        use super::app_warning_config::BalanceConfigPolicy;
        match policy {
            BalanceConfigPolicy::Legacy => {
                Self::apply_balance_result(pool, observed_events, body, balance).await
            }
            BalanceConfigPolicy::Configured { org_id, app_id, generation, threshold } => {
                let clear = match threshold {
                    None => true,
                    Some(threshold) => match balance {
                        Ok(balance) => balance >= *threshold,
                        Err(_) => false,
                    },
                };
                if clear {
                    ApiBusinessNotificationRepo::resolve_observed_group_for_config(
                        pool,
                        observed_events,
                        org_id,
                        app_id,
                        generation,
                    )
                    .await
                    .map_err(Into::into)
                } else {
                    Ok(false)
                }
            }
        }
    }

    async fn apply_balance_result(
        pool: &ApiWalletDbPool,
        observed_events: &[ApiBusinessNotification],
        body: &AwmAddressInsufficientBody,
        balance: Result<Decimal, ServiceError>,
    ) -> Result<bool, ServiceError> {
        let Some(notification) = observed_events.first() else {
            return Ok(false);
        };
        match balance {
            Ok(balance) if balance >= body.threshold_value => {
                return Ok(ApiBusinessNotificationRepo::resolve_observed_group(
                    pool,
                    observed_events,
                )
                .await?);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(id = %notification.id, %error, "balance query failed; reminder remains active");
            }
        }
        Ok(false)
    }

    async fn publish_resolution_changed(&self, changed: bool) {
        if changed {
            use crate::messaging::notify::{FrontendNotifyEvent, event::NotifyEvent};
            if let Err(error) =
                FrontendNotifyEvent::new(NotifyEvent::ApiWalletNotificationChanged {})
                    .send_with_ctx(self.ctx)
                    .await
            {
                tracing::warn!(%error, "notification resolution committed; frontend refresh unavailable");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use std::sync::atomic::{AtomicU64, Ordering};

    use wallet_database::{
        entities::api_business_notification::NewApiBusinessNotification,
        repositories::api_business_notification::ApiBusinessNotificationRepo,
    };

    use super::{ApiBusinessNotificationService, AwmAddressInsufficientBody};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn business_queries_support_plugin_thread_pool() {
        fn assert_send<T: Send>(_: T) {}
        fn check(ctx: &'static crate::context::Context) {
            let service = ApiBusinessNotificationService::new(ctx);
            assert_send(service.unresolved_address_balance_notifications());
            let manager = crate::manager::WalletManager { ctx, handles: None };
            assert_send(manager.page_api_business_notifications(0, 10));
            assert_send(manager.page_unresolved_api_business_notifications(0, 10));
            assert_send(manager.count_unread_api_wallet_notifications());
            assert_send(manager.mark_api_business_notification_read("record"));
            assert_send(manager.mark_all_api_business_notifications_read());
            assert_send(super::super::withdraw::WithdrawService::new(ctx).pending_withdrawals());
        }
        // Type-check the public future without constructing a global SDK or using RPC.
        let _ = check as fn(&'static crate::context::Context);
    }

    #[tokio::test]
    async fn balance_checks_coalesce_but_expire_and_invalidate_changed_alerts() {
        use super::BALANCE_CHECK_BUFFER;
        use std::{sync::atomic::AtomicUsize, time::Instant};

        // Arrange: one active alert per UID, with an injected monotonic clock.
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        let uid = format!("buffer-owner-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let other_uid = format!("{uid}-other");
        let payload = r#"{"remaining":"10","thresholdValue":"20","token":"","symbol":"TRX","merchantName":"merchant","waddressType":"1","chainCode":"tron","targetAddress":"TAddress"}"#;
        for owner in [&uid, &other_uid] {
            let key = format!("{owner}-alert");
            ApiBusinessNotificationRepo::upsert(
                &pool,
                NewApiBusinessNotification {
                    id: &key,
                    uid: owner,
                    kind: "AWM_ADDRESS_INSUFFICIENT",
                    business_key: &key,
                    payload,
                },
            )
            .await
            .unwrap();
        }
        let service = ApiBusinessNotificationService::new(ctx);
        let calls = AtomicUsize::new(0);
        let now = Instant::now();

        // Act: overlapping callers share one completed balance check.
        let balance_for = |_, _, _| {
            calls.fetch_add(1, Ordering::Relaxed);
            async {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                Ok(Decimal::new(10, 0))
            }
        };
        let first =
            service.unresolved_balance_notifications_for_uid_using(&uid, || now, &balance_for);
        let overlapping =
            service.unresolved_balance_notifications_for_uid_using(&uid, || now, &balance_for);
        let (first, overlapping) = tokio::join!(first, overlapping);
        assert_eq!(first.unwrap().len(), 1);
        assert_eq!(overlapping.unwrap().len(), 1);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // Assert: read changes reuse the buffer, but each UID has its own check.
        ApiBusinessNotificationRepo::mark_all_read(&pool, &uid).await.unwrap();
        assert_eq!(
            service
                .unresolved_balance_notifications_for_uid_using(
                    &uid,
                    || now,
                    |_, _, _| async { panic!("buffered read") }
                )
                .await
                .unwrap()
                .len(),
            1
        );
        service
            .unresolved_balance_notifications_for_uid_using(
                &other_uid,
                || now,
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Decimal::new(10, 0)) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);

        // Expiry permits a full check without waiting in real time.
        service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + BALANCE_CHECK_BUFFER,
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Decimal::new(10, 0)) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 3);

        // A new threshold bypasses the buffer immediately.
        let key = format!("{uid}-alert");
        let updated_payload =
            payload.replace("\"thresholdValue\":\"20\"", "\"thresholdValue\":\"100\"");
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: &key,
                uid: &uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: &key,
                payload: &updated_payload,
            },
        )
        .await
        .unwrap();
        let updated = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + BALANCE_CHECK_BUFFER,
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Decimal::new(50, 0)) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 4);
        assert_eq!(updated.len(), 1);

        // A new reminder checks itself without invalidating another record observation.
        let new_key = format!("{uid}-new-alert");
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: &new_key,
                uid: &uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: &new_key,
                payload,
            },
        )
        .await
        .unwrap();
        let new = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + BALANCE_CHECK_BUFFER,
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Decimal::new(10, 0)) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 5);
        assert_eq!(new.len(), 2);
    }

    #[tokio::test]
    async fn balance_checks_all_records_and_preserves_read_state() {
        use std::sync::atomic::AtomicUsize;
        use wallet_database::{
            entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus},
            repositories::api_wallet::withdraw::ApiWithdrawRepo,
        };

        // Arrange: twelve unresolved reminders and orders from two different UIDs.
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        let uid = format!("balance-owner-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        for index in 0..12 {
            let key = format!("{uid}-{index}");
            let payload = serde_json::json!({"remaining":"10", "thresholdValue":"20", "token":"", "symbol":"TRX", "merchantName":"merchant", "waddressType":"1", "chainCode":"tron", "targetAddress":key, "notifiedAt":"2026-10-01T00:00:00Z"}).to_string();
            ApiBusinessNotificationRepo::upsert(
                &pool,
                NewApiBusinessNotification {
                    id: &key,
                    uid: &uid,
                    kind: "AWM_ADDRESS_INSUFFICIENT",
                    business_key: &key,
                    payload: &payload,
                },
            )
            .await
            .unwrap();
        }
        let tx_pool = ctx.api_transaction_pool().unwrap();
        for (owner, key, status) in [
            (&*uid, "pending", ApiWithdrawStatus::Init),
            (&*uid, "finished", ApiWithdrawStatus::Success),
            (&*uid, "signed", ApiWithdrawStatus::Init),
            (&*uid, "rejected", ApiWithdrawStatus::Init),
            ("other", "foreign", ApiWithdrawStatus::Init),
        ] {
            ApiWithdrawRepo::upsert_api_withdraw(
                &tx_pool,
                owner,
                "account",
                "from",
                "to",
                "10",
                "validate",
                "tron",
                None,
                "TRX",
                &format!("{uid}-{key}"),
                None,
                None,
                None,
                ApiTradeType::Withdraw,
                0,
                None,
                ApiWithdrawStatus::Init,
                status,
                "",
                "0",
                None,
                None,
            )
            .await
            .unwrap();
        }
        ApiWithdrawRepo::set_audit_passed(&tx_pool, &format!("{uid}-signed")).await.unwrap();
        ApiWithdrawRepo::set_audit_rejected(&tx_pool, &format!("{uid}-rejected"), "fixture")
            .await
            .unwrap();
        let pending = super::super::withdraw::WithdrawService::new(ctx)
            .pending_withdrawals_for_uid(&uid)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].trade_no, format!("{uid}-pending"));
        let service = ApiBusinessNotificationService::new(ctx);
        let calls = AtomicUsize::new(0);
        let now = std::time::Instant::now();

        // Act: every poll checks all twelve reminders, including those beyond ten.
        let first = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now,
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Err(crate::error::service::ServiceError::Parameter("offline".into())) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 12);
        assert_eq!(first.len(), 12);
        ApiBusinessNotificationRepo::mark_all_read(&pool, &uid).await.unwrap();
        let second = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + std::time::Duration::from_secs(30),
                |_, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async { Ok(Decimal::new(10, 0)) }
                },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 24);
        assert_eq!(second.len(), 12);
        let third = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + std::time::Duration::from_secs(60),
                |address, _, _| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    async move {
                        let index: usize = address.rsplit('-').next().unwrap().parse().unwrap();
                        Ok(Decimal::new(if index < 5 { 20 } else { 10 }, 0))
                    }
                },
            )
            .await
            .unwrap();

        // Assert: directed checks cover every unresolved row and preserve read state.
        assert_eq!(calls.load(Ordering::Relaxed), 36);
        assert_eq!(third.len(), 7);
        let fourth = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + std::time::Duration::from_secs(90),
                |_, _, _| async { Ok(Decimal::new(20, 0)) },
            )
            .await
            .unwrap();
        assert_eq!(fourth.len(), 0);
        let fifth = service
            .unresolved_balance_notifications_for_uid_using(
                &uid,
                || now + std::time::Duration::from_secs(90),
                |_, _, _| async { Ok(Decimal::new(20, 0)) },
            )
            .await
            .unwrap();
        assert_eq!(fifth.len(), 0);
        let guard = ctx.address_balance_check.lock().await;
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                service.unresolved_balance_notifications_for_uid_using(
                    &uid,
                    || now,
                    |_, _, _| async { panic!("overlapping refresh") }
                )
            )
            .await
            .unwrap()
            .unwrap()
            .is_empty()
        );
        drop(guard);
        assert!(
            service
                .unresolved_balance_notifications_for_uid_using(
                    " ",
                    || now,
                    |_, _, _| async { panic!("invalid uid") }
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn balance_reminder_list_and_read_are_uid_scoped() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let uid = format!("balance-owner-{id}");
        let other_uid = format!("balance-other-{id}");
        let notification_id = format!("balance-message-{id}");
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: &notification_id,
                uid: &uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: "tron:address:trx",
                payload: r#"{"remaining":"10","thresholdValue":"20"}"#,
            },
        )
        .await
        .unwrap();
        let service = ApiBusinessNotificationService::new(ctx);

        assert_eq!(service.list(&other_uid, false, 0, 20).await.unwrap().total_count, 0);
        assert_eq!(service.list(&uid, true, 0, 20).await.unwrap().total_count, 1);
        service.mark_read(&other_uid, &notification_id).await.unwrap();
        assert!(!service.list(&uid, false, 0, 20).await.unwrap().data[0].is_read);
        service.mark_read(&uid, &notification_id).await.unwrap();
        assert!(service.list(&uid, false, 0, 20).await.unwrap().data[0].is_read);
        assert_eq!(service.list(&uid, true, 0, 20).await.unwrap().total_count, 1);
    }

    #[tokio::test]
    async fn business_reminder_queries_require_uid_and_valid_page() {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let service = ApiBusinessNotificationService::new(ctx);
        assert!(service.list("", false, 0, 20).await.is_err());
        assert!(service.list("uid", false, -1, 20).await.is_err());
        assert!(service.list("uid", true, 0, 0).await.is_err());
        assert!(service.mark_read("", "message").await.is_err());
        for uid in ["", " "] {
            assert!(service.unread_count(uid).await.is_err());
            assert!(service.mark_all_read(uid).await.is_err());
        }
    }

    #[tokio::test]
    async fn business_all_read_covers_all_pages_without_resolving_or_crossing_uid() {
        // Arrange: twelve reminders and an independent owner.
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let uid = format!("all-read-owner-{id}");
        let other_uid = format!("all-read-other-{id}");
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        for index in 0..13 {
            let key = format!("all-read-{id}-{index}");
            ApiBusinessNotificationRepo::upsert(
                &pool,
                NewApiBusinessNotification {
                    id: &key,
                    uid: if index == 12 { &other_uid } else { &uid },
                    kind: "AWM_ADDRESS_INSUFFICIENT",
                    business_key: &key,
                    payload: r#"{"remaining":"10","thresholdValue":"20"}"#,
                },
            )
            .await
            .unwrap();
        }
        ApiBusinessNotificationRepo::resolve_if_payload_matches(
            &pool,
            &uid,
            &format!("all-read-{id}-0"),
            r#"{"remaining":"10","thresholdValue":"20"}"#,
        )
        .await
        .unwrap();
        let service = ApiBusinessNotificationService::new(ctx);
        assert_eq!(service.unread_count(&uid).await.unwrap(), 12);
        let before = service.list(&uid, true, 0, 20).await.unwrap();

        // Act: all-read is idempotent and applies beyond the visible page.
        service.mark_all_read(&uid).await.unwrap();
        service.mark_all_read(&uid).await.unwrap();

        // Assert: active reminders and payloads survive; another UID stays unread.
        assert_eq!(service.unread_count(&uid).await.unwrap(), 0);
        assert_eq!(service.unread_count(&other_uid).await.unwrap(), 1);
        let after = service.list(&uid, true, 0, 20).await.unwrap();
        assert_eq!(after.total_count, 11);
        for (before, after) in before.data.iter().zip(&after.data) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.payload, after.payload);
            assert!(after.is_read);
            assert!(after.resolved_at.is_none());
        }
        assert_eq!(service.list(&uid, false, 1, 10).await.unwrap().data.len(), 2);
        let history = service.list(&uid, false, 0, 20).await.unwrap();
        assert!(history.data.iter().all(|item| item.is_read));
        assert_eq!(history.data.iter().filter(|item| item.resolved_at.is_some()).count(), 1);
        for invalid in ["", " "] {
            assert!(service.mark_all_read(invalid).await.is_err());
        }
        assert_eq!(service.unread_count(&other_uid).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn unresolved_page_checks_items_that_shift_in_after_resolution() {
        // Arrange: the newest item resolves, while the next two remain active.
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let uid = format!("pagination-owner-{id}");
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        for index in 0..3 {
            let key = format!("pagination-{id}-{index}");
            let body = AwmAddressInsufficientBody {
                remaining: Decimal::new(10, 0),
                token: String::new(),
                symbol: "TRX".into(),
                merchant_name: "merchant".into(),
                w_address_type: "1".into(),
                threshold_value: Decimal::new(20, 0),
                chain_code: "tron".into(),
                address: key.clone(),
            };
            let payload = serde_json::to_string(&body).unwrap();
            ApiBusinessNotificationRepo::upsert(
                &pool,
                NewApiBusinessNotification {
                    id: &key,
                    uid: &uid,
                    kind: "AWM_ADDRESS_INSUFFICIENT",
                    business_key: &key,
                    payload: &payload,
                },
            )
            .await
            .unwrap();
        }
        let service = ApiBusinessNotificationService::new(ctx);

        // Act: ask for one item; the other two must be checked too.
        let page = service
            .list_unresolved_with_refresh_using(&uid, 0, 1, |address, _, _| async move {
                if address.ends_with("-2") {
                    Ok(Decimal::new(20, 0))
                } else {
                    Ok(Decimal::new(10, 0))
                }
            })
            .await
            .unwrap();

        // Assert: pagination uses the fully refreshed set.
        assert_eq!(page.total_count, 2);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].id, format!("pagination-{id}-1"));
        assert_eq!(service.list(&uid, true, 0, 10).await.unwrap().total_count, 2);
    }

    #[tokio::test]
    async fn current_balance_resolves_only_after_successful_threshold_check() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let uid = format!("balance-refresh-owner-{id}");
        let notification_id = format!("balance-refresh-{id}");
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let pool = ctx.api_wallet_pool().unwrap();
        let body = AwmAddressInsufficientBody {
            remaining: Decimal::new(10, 0),
            token: String::new(),
            symbol: "TRX".into(),
            merchant_name: "merchant".into(),
            w_address_type: "1".into(),
            threshold_value: Decimal::new(20, 0),
            chain_code: "tron".into(),
            address: "TAddress".into(),
        };
        let payload = serde_json::to_string(&body).unwrap();
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: &notification_id,
                uid: &uid,
                kind: "AWM_ADDRESS_INSUFFICIENT",
                business_key: &notification_id,
                payload: &payload,
            },
        )
        .await
        .unwrap();
        let row = ApiBusinessNotificationRepo::list(&pool, &uid, true, 0, 20)
            .await
            .unwrap()
            .data
            .remove(0);

        // An unresolved RPC must hit the existing timeout and preserve the durable alert.
        let timed_out =
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ApiBusinessNotificationService::new(ctx)
                    .unresolved_balance_notifications_for_uid_using(
                        &uid,
                        std::time::Instant::now,
                        |_, _, _| {
                            std::future::pending::<
                                Result<Decimal, crate::error::service::ServiceError>,
                            >()
                        },
                    ),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(timed_out.len(), 1);
        assert!(timed_out[0].resolved_at.is_none());
        assert!(!timed_out[0].is_read);
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, &uid, true, 0, 20).await.unwrap().total_count,
            1
        );

        ApiBusinessNotificationService::apply_balance_result(
            &pool,
            std::slice::from_ref(&row),
            &body,
            Err(crate::error::service::ServiceError::Parameter("RPC unavailable".into())),
        )
        .await
        .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, &uid, true, 0, 20).await.unwrap().total_count,
            1
        );

        ApiBusinessNotificationService::apply_balance_result(
            &pool,
            std::slice::from_ref(&row),
            &body,
            Ok(Decimal::new(19, 0)),
        )
        .await
        .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, &uid, true, 0, 20).await.unwrap().total_count,
            1
        );

        ApiBusinessNotificationService::apply_balance_result(
            &pool,
            std::slice::from_ref(&row),
            &body,
            Ok(Decimal::new(20, 0)),
        )
        .await
        .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, &uid, true, 0, 20).await.unwrap().total_count,
            0
        );
        let persisted = ApiBusinessNotificationRepo::list(&pool, &uid, false, 0, 20).await.unwrap();
        assert!(persisted.data[0].payload.contains("\"remaining\":\"10\""));
    }
}

#[cfg(test)]
#[path = "business_notification/synced_balance_tests.rs"]
mod synced_balance_tests;

#[cfg(test)]
#[path = "business_notification/notification_recovery_tests.rs"]
mod notification_recovery_tests;

#[cfg(test)]
#[path = "business_notification/global_business_query_tests.rs"]
pub(crate) mod global_business_query_tests;

#[cfg(test)]
#[path = "business_notification/withdrawal_message_center_tests.rs"]
mod withdrawal_message_center_tests;

#[cfg(test)]
mod detail_tests;

#[cfg(test)]
mod event_history_tests;

#[cfg(test)]
#[path = "business_notification/app_config_tests.rs"]
mod app_config_tests;
