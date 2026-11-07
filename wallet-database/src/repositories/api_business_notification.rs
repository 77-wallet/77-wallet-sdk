use crate::{
    ApiWalletDbPool, Error,
    dao::api_business_notification::ApiBusinessNotificationDao,
    db::sqlite_retry::with_sqlite_locked_retry,
    entities::api_business_notification::{ApiBusinessNotification, NewApiBusinessNotification},
    pagination::Pagination,
};

pub struct ApiBusinessNotificationRepo;

impl ApiBusinessNotificationRepo {
    pub async fn latest_for_import(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain: &str,
    ) -> Result<Vec<ApiBusinessNotification>, Error> {
        ApiBusinessNotificationDao::latest_for_import(pool.read_ref(), uid, chain).await
    }

    pub async fn list_for_withdrawal_wallets(
        pool: &ApiWalletDbPool,
        unresolved_only: bool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, Error> {
        let offset = page.checked_mul(page_size).ok_or_else(|| {
            Error::Database(sqlx::Error::Protocol("pagination offset overflow".into()).into())
        })?;
        let total_count = ApiBusinessNotificationDao::count_for_withdrawal_wallets(
            pool.read_ref(),
            unresolved_only,
        )
        .await?;
        let data = ApiBusinessNotificationDao::list_for_withdrawal_wallets(
            pool.read_ref(),
            unresolved_only,
            page_size,
            offset,
        )
        .await?;
        Ok(Pagination { page, page_size, total_count, data })
    }

    pub async fn unread_count_for_withdrawal_wallets(pool: &ApiWalletDbPool) -> Result<i64, Error> {
        ApiBusinessNotificationDao::unread_count_for_withdrawal_wallets(pool.read_ref()).await
    }

    pub async fn mark_read_for_withdrawal_wallets(
        pool: &ApiWalletDbPool,
        id: &str,
    ) -> Result<(), Error> {
        let _write_guard = pool.lock_write_with_metric("mark_read_for_withdrawal_wallets").await;
        with_sqlite_locked_retry(|| {
            ApiBusinessNotificationDao::mark_read_for_withdrawal_wallets(pool.write_ref(), id)
        })
        .await
    }

    pub async fn mark_all_read_for_withdrawal_wallets(pool: &ApiWalletDbPool) -> Result<(), Error> {
        ApiBusinessNotificationDao::mark_all_read_for_withdrawal_wallets(pool.write_ref()).await
    }

    pub async fn find_for_withdrawal_wallets(
        pool: &ApiWalletDbPool,
        id: &str,
    ) -> Result<Option<ApiBusinessNotification>, Error> {
        ApiBusinessNotificationDao::find_for_withdrawal_wallets(pool.read_ref(), id).await
    }

    pub async fn find(
        pool: &ApiWalletDbPool,
        uid: &str,
        id: &str,
    ) -> Result<Option<ApiBusinessNotification>, Error> {
        crate::dao::api_business_notification::ApiBusinessNotificationDao::find(
            pool.read_ref(),
            uid,
            id,
        )
        .await
    }
    pub async fn ingest(
        pool: &ApiWalletDbPool,
        n: NewApiBusinessNotification<'_>,
    ) -> Result<(), Error> {
        crate::dao::api_business_notification::ApiBusinessNotificationDao::ingest(
            pool.write_ref(),
            n,
        )
        .await
    }

    /// Compatibility update by row ID; transport event ingestion must use `ingest`.
    pub async fn upsert(
        pool: &ApiWalletDbPool,
        notification: NewApiBusinessNotification<'_>,
    ) -> Result<(), Error> {
        ApiBusinessNotificationDao::upsert(pool.write_ref(), notification).await
    }

    pub async fn list(
        pool: &ApiWalletDbPool,
        uid: &str,
        unresolved_only: bool,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiBusinessNotification>, Error> {
        let total_count =
            ApiBusinessNotificationDao::count(pool.read_ref(), uid, unresolved_only).await?;
        let data = ApiBusinessNotificationDao::list(
            pool.read_ref(),
            uid,
            unresolved_only,
            page_size,
            page * page_size,
        )
        .await?;
        Ok(Pagination { page, page_size, total_count, data })
    }

    /// Counts unread history, including reminders whose business condition is resolved.
    pub async fn unread_count(pool: &ApiWalletDbPool, uid: &str) -> Result<i64, Error> {
        ApiBusinessNotificationDao::unread_count(pool.read_ref(), uid).await
    }

    /// Marks every page of this UID's history read without resolving business conditions.
    pub async fn mark_all_read(pool: &ApiWalletDbPool, uid: &str) -> Result<(), Error> {
        ApiBusinessNotificationDao::mark_all_read(pool.write_ref(), uid).await
    }

    pub async fn mark_read(pool: &ApiWalletDbPool, uid: &str, id: &str) -> Result<(), Error> {
        ApiBusinessNotificationDao::mark_read(pool.write_ref(), uid, id).await
    }

    pub async fn mark_resolved(
        pool: &ApiWalletDbPool,
        uid: &str,
        id: &str,
        resolved: bool,
    ) -> Result<(), Error> {
        ApiBusinessNotificationDao::mark_resolved(pool.write_ref(), uid, id, resolved).await
    }

    /// Resolve only observed versions, atomically; later arrivals are never selected.
    pub async fn resolve_observed_group(
        pool: &ApiWalletDbPool,
        events: &[ApiBusinessNotification],
    ) -> Result<bool, Error> {
        Self::resolve_observed_group_guarded(pool, events, None, None).await
    }

    /// Also reject a wallet rebind/deactivation that happened during a config/balance query.
    pub async fn resolve_observed_group_for_binding(
        pool: &ApiWalletDbPool,
        events: &[ApiBusinessNotification],
        org_id: &str,
        app_id: &str,
    ) -> Result<bool, Error> {
        Self::resolve_observed_group_guarded(pool, events, Some((org_id, app_id)), None).await
    }

    /// A removed configuration generation cannot authorize an in-flight balance result.
    pub async fn resolve_observed_group_for_config(
        pool: &ApiWalletDbPool,
        events: &[ApiBusinessNotification],
        org_id: &str,
        app_id: &str,
        generation: &str,
    ) -> Result<bool, Error> {
        Self::resolve_observed_group_guarded(pool, events, Some((org_id, app_id)), Some(generation))
            .await
    }

    async fn resolve_observed_group_guarded(
        pool: &ApiWalletDbPool,
        events: &[ApiBusinessNotification],
        binding: Option<(&str, &str)>,
        generation: Option<&str>,
    ) -> Result<bool, Error> {
        if events.is_empty() || events.iter().any(|event| event.uid != events[0].uid) {
            return Ok(false);
        }
        let mut tx = pool
            .write_ref()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|e| Error::Database(e.into()))?;
        if let Some((org_id, app_id)) = binding {
            let wallet =
                crate::dao::api_wallet::ApiWalletDao::detail_by_uid(&mut *tx, &events[0].uid)
                    .await?;
            if !wallet.is_some_and(|wallet| {
                wallet.api_wallet_type == crate::entities::api_wallet::ApiWalletType::Withdrawal
                    && wallet.merchant_id.as_deref() == Some(org_id)
                    && wallet.app_id.as_deref() == Some(app_id)
            }) {
                tx.rollback().await.map_err(|e| Error::Database(e.into()))?;
                return Ok(false);
            }
            if let Some(generation) = generation {
                let matches =
                    crate::dao::api_app_warning_config::ApiAppWarningConfigDao::generation_exists(
                        tx.as_mut(),
                        org_id,
                        app_id,
                        generation,
                    )
                    .await?;
                if !matches {
                    tx.rollback().await.map_err(|e| Error::Database(e.into()))?;
                    return Ok(false);
                }
            }
        }
        for event in events {
            let current = ApiBusinessNotificationDao::find(&mut *tx, &event.uid, &event.id).await?;
            if !current.is_some_and(|row| {
                row.payload == event.payload
                    && row.created_at == event.created_at
                    && row.resolved_at == event.resolved_at
            }) {
                tx.rollback().await.map_err(|e| Error::Database(e.into()))?;
                return Ok(false);
            }
        }
        for event in events {
            ApiBusinessNotificationDao::resolve_if_payload_matches(
                &mut *tx,
                &event.uid,
                &event.id,
                &event.payload,
            )
            .await?;
        }
        tx.commit().await.map_err(|e| Error::Database(e.into()))?;
        Ok(true)
    }

    /// A balance observation may resolve only the alert version it checked.
    pub async fn resolve_if_payload_matches(
        pool: &ApiWalletDbPool,
        uid: &str,
        id: &str,
        payload: &str,
    ) -> Result<(), Error> {
        ApiBusinessNotificationDao::resolve_if_payload_matches(pool.write_ref(), uid, id, payload)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::test_helper::{make_temp_dir, setup_api_wallet_pool, setup_core_pool};

    fn balance_alert<'a>(uid: &'a str, business_key: &'a str) -> NewApiBusinessNotification<'a> {
        NewApiBusinessNotification {
            id: business_key,
            uid,
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key,
            payload: r#"{"remaining":"10","thresholdValue":"20"}"#,
        }
    }

    #[tokio::test]
    async fn binding_guard_rejects_rebound_owner_and_preserves_history() {
        use crate::{
            entities::api_wallet::ApiWalletType, repositories::api_wallet::wallet::ApiWalletRepo,
        };
        let pool = setup_api_wallet_pool("warning_config_binding_guard").await;
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
        ApiWalletRepo::update_merchant_id(&pool, "wallet", "org").await.unwrap();
        ApiWalletRepo::update_app_id(&pool, "wallet", Some("app")).await.unwrap();
        ApiBusinessNotificationRepo::upsert(&pool, balance_alert("owner", "alert")).await.unwrap();
        let event =
            ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
        ApiWalletRepo::update_merchant_id(&pool, "wallet", "other").await.unwrap();
        assert!(
            !ApiBusinessNotificationRepo::resolve_observed_group_for_binding(
                &pool,
                std::slice::from_ref(&event),
                "org",
                "app"
            )
            .await
            .unwrap()
        );
        let current =
            ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
        assert!(current.resolved_at.is_none());
        assert_eq!(current.payload, event.payload);
        assert!(!current.is_read);
        assert!(
            ApiBusinessNotificationRepo::resolve_observed_group_for_binding(
                &pool,
                std::slice::from_ref(&event),
                "other",
                "app"
            )
            .await
            .unwrap()
        );
        let history =
            ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
        assert!(history.resolved_at.is_some());
        assert_eq!(history.payload, event.payload);
        assert!(!history.is_read);
    }

    #[tokio::test]
    async fn pagination_count_and_unresolved_filter_remain_uid_scoped() {
        // Arrange: five owner records, one resolved record and independent foreign history.
        let pool = setup_api_wallet_pool("api_business_notice_dao_pagination").await;
        for id in ["owner-00", "owner-01", "owner-02", "owner-03", "owner-04"] {
            ApiBusinessNotificationRepo::upsert(&pool, balance_alert("owner", id)).await.unwrap();
        }
        ApiBusinessNotificationRepo::upsert(&pool, balance_alert("other", "other-00"))
            .await
            .unwrap();
        ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "owner-04", true).await.unwrap();
        ApiBusinessNotificationRepo::mark_read(&pool, "owner", "owner-02").await.unwrap();
        // Act: offset pagination counts the whole filtered owner scope, not the current page.
        let first = ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 2).await.unwrap();
        let second = ApiBusinessNotificationRepo::list(&pool, "owner", true, 1, 2).await.unwrap();
        let past_end = ApiBusinessNotificationRepo::list(&pool, "owner", true, 2, 2).await.unwrap();
        let history = ApiBusinessNotificationRepo::list(&pool, "owner", false, 2, 2).await.unwrap();
        // Assert: filtering precedes offset, read rows remain eligible, and foreign UID is excluded.
        assert_eq!((first.page, first.page_size, first.total_count), (0, 2, 4));
        assert_eq!(
            first.data.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["owner-03", "owner-02"]
        );
        assert_eq!(
            second.data.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["owner-01", "owner-00"]
        );
        assert!(first.data[1].is_read);
        assert_eq!((second.page, second.total_count, past_end.total_count), (1, 4, 4));
        assert!(past_end.data.is_empty());
        assert_eq!(history.total_count, 5);
        assert_eq!(history.data.len(), 1);
        assert_eq!(history.data[0].id, "owner-00");
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "other").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn mqtt_ingest_event_id_replay_preserves_resolution_and_payload() {
        // Arrange: a real notification row resolved after the first durable event.
        let pool = setup_api_wallet_pool("api_business_notice_ingest_replay").await;
        let n = balance_alert("owner", "alert");
        ApiBusinessNotificationRepo::ingest(&pool, n).await.unwrap();
        ApiBusinessNotificationRepo::mark_read(&pool, "owner", "alert").await.unwrap();
        ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "alert", true).await.unwrap();
        // Act/Assert: exact retry leaves the completed fact intact.
        ApiBusinessNotificationRepo::ingest(&pool, balance_alert("owner", "alert")).await.unwrap();
        let replay =
            ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
        assert!(replay.resolved_at.is_some());
        assert!(replay.is_read);
        assert!(
            ApiBusinessNotificationRepo::find(&pool, "other", "alert").await.unwrap().is_none()
        );
        let changed = NewApiBusinessNotification {
            payload: r#"{"remaining":"1","thresholdValue":"30"}"#,
            ..balance_alert("owner", "alert")
        };
        ApiBusinessNotificationRepo::ingest(&pool, changed).await.unwrap();
        let changed =
            ApiBusinessNotificationRepo::find(&pool, "owner", "alert").await.unwrap().unwrap();
        assert!(changed.resolved_at.is_some());
        assert!(changed.is_read);
        assert_eq!(changed.payload, r#"{"remaining":"10","thresholdValue":"20"}"#);
    }

    #[tokio::test]
    async fn stale_balance_observation_cannot_resolve_new_alert_payload() {
        // Arrange: a new threshold replaces the payload while a balance check is in flight.
        let pool = setup_api_wallet_pool("api_business_notice_stale_balance").await;
        let previous = balance_alert("owner", "alert");
        let old_payload = previous.payload;
        ApiBusinessNotificationRepo::upsert(&pool, previous).await.unwrap();
        let mut next = balance_alert("owner", "alert");
        next.payload = r#"{"remaining":"10","thresholdValue":"100"}"#;
        let new_payload = next.payload;
        ApiBusinessNotificationRepo::upsert(&pool, next).await.unwrap();

        // Act/Assert: neither a stale payload nor a different UID may clear the alert.
        ApiBusinessNotificationRepo::resolve_if_payload_matches(
            &pool,
            "owner",
            "alert",
            old_payload,
        )
        .await
        .unwrap();
        ApiBusinessNotificationRepo::resolve_if_payload_matches(
            &pool,
            "other",
            "alert",
            new_payload,
        )
        .await
        .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 10)
                .await
                .unwrap()
                .total_count,
            1
        );
        ApiBusinessNotificationRepo::resolve_if_payload_matches(
            &pool,
            "owner",
            "alert",
            new_payload,
        )
        .await
        .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 10)
                .await
                .unwrap()
                .total_count,
            0
        );
    }

    #[tokio::test]
    async fn unread_count_and_mark_all_read_cover_history_and_preserve_uid_isolation() {
        // Arrange: more messages than a page, including resolved unread history.
        let pool = setup_api_wallet_pool("api_business_notice_read_all").await;
        for index in 0..12 {
            let key = format!("owner-alert-{index}");
            ApiBusinessNotificationRepo::upsert(&pool, balance_alert("owner", &key)).await.unwrap();
        }
        ApiBusinessNotificationRepo::upsert(&pool, balance_alert("other", "other-alert"))
            .await
            .unwrap();
        ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "owner-alert-0", true)
            .await
            .unwrap();
        ApiBusinessNotificationRepo::mark_read(&pool, "owner", "owner-alert-1").await.unwrap();
        let before = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "owner").await.unwrap(), 11);

        // Act: a nonexistent UID cannot change another wallet's read state.
        ApiBusinessNotificationRepo::mark_all_read(&pool, "missing").await.unwrap();
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "owner").await.unwrap(), 11);
        ApiBusinessNotificationRepo::mark_all_read(&pool, "owner").await.unwrap();
        ApiBusinessNotificationRepo::mark_all_read(&pool, "owner").await.unwrap();

        // Assert: all pages become read; payload and resolution facts survive.
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "owner").await.unwrap(), 0);
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "other").await.unwrap(), 1);
        assert_eq!(ApiBusinessNotificationRepo::unread_count(&pool, "missing").await.unwrap(), 0);
        let after = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
        assert_eq!(after.total_count, 12);
        for (previous, current) in before.data.iter().zip(&after.data) {
            assert!(current.is_read);
            assert_eq!(current.id, previous.id);
            assert_eq!(current.payload, previous.payload);
            assert_eq!(current.resolved_at, previous.resolved_at);
            assert_eq!(current.created_at, previous.created_at);
        }
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 20)
                .await
                .unwrap()
                .total_count,
            11
        );
    }

    #[tokio::test]
    async fn notification_survives_reopen_and_is_scoped_by_uid() {
        let dir = make_temp_dir("api_business_notice_persist");
        let pool = crate::SqliteContext::new(&dir, Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
        ApiBusinessNotificationRepo::upsert(&pool, balance_alert("owner-a", "tron:address-1:TRX"))
            .await
            .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, "owner-a", false, 0, 20)
                .await
                .unwrap()
                .data
                .len(),
            1
        );
        assert!(
            ApiBusinessNotificationRepo::list(&pool, "owner-b", false, 0, 20)
                .await
                .unwrap()
                .data
                .is_empty()
        );

        drop(pool);
        let reopened = crate::SqliteContext::new(&dir, Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
        let rows =
            ApiBusinessNotificationRepo::list(&reopened, "owner-a", true, 0, 20).await.unwrap();
        assert_eq!(rows.data[0].business_key, "tron:address-1:TRX");
    }

    #[tokio::test]
    async fn core_database_does_not_create_api_business_notification_table() {
        let core = setup_core_pool("api_business_notice_core_absent").await;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'api_business_notification'",
        )
        .fetch_one(core.read_ref())
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn duplicate_delivery_preserves_read_and_resolution_state() {
        let pool = setup_api_wallet_pool("api_business_notice_idempotent").await;
        ApiBusinessNotificationRepo::upsert(&pool, balance_alert("owner", "tron:address-2:TRX"))
            .await
            .unwrap();
        ApiBusinessNotificationRepo::mark_read(&pool, "other-owner", "tron:address-2:TRX")
            .await
            .unwrap();
        assert!(
            !ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap().data[0]
                .is_read
        );
        ApiBusinessNotificationRepo::mark_read(&pool, "owner", "tron:address-2:TRX").await.unwrap();
        ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "tron:address-2:TRX", true)
            .await
            .unwrap();
        ApiBusinessNotificationRepo::upsert(
            &pool,
            NewApiBusinessNotification {
                id: "tron:address-2:TRX",
                ..balance_alert("owner", "tron:address-2:TRX")
            },
        )
        .await
        .unwrap();

        let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
        assert_eq!(rows.data.len(), 1);
        assert_eq!(rows.data[0].id, "tron:address-2:TRX");
        assert!(rows.data[0].is_read);
        assert!(rows.data[0].resolved_at.is_some());
        assert!(
            ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 20)
                .await
                .unwrap()
                .data
                .is_empty()
        );

        ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", "tron:address-2:TRX", false)
            .await
            .unwrap();
        assert_eq!(
            ApiBusinessNotificationRepo::list(&pool, "owner", true, 0, 20)
                .await
                .unwrap()
                .data
                .len(),
            1
        );
    }
}

#[cfg(test)]
#[path = "api_business_notification/withdrawal_message_center_tests.rs"]
mod withdrawal_message_center_tests;

#[cfg(test)]
mod detail_tests;

#[cfg(test)]
#[path = "api_business_notification/event_history_tests.rs"]
mod event_history_tests;
