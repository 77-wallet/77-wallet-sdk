use super::ApiBusinessNotificationRepo as Repo;
use crate::{
    entities::api_business_notification::NewApiBusinessNotification,
    repositories::test_helper::setup_api_wallet_pool,
};

fn event<'a>(id: &'a str, payload: &'a str) -> NewApiBusinessNotification<'a> {
    NewApiBusinessNotification {
        id,
        uid: "owner",
        kind: "AWM_ADDRESS_INSUFFICIENT",
        business_key: "owner:tron:address:",
        payload,
    }
}

#[tokio::test]
async fn different_events_same_object_keep_history_and_single_read_isolation() {
    let pool = setup_api_wallet_pool("event_history_independent").await;
    let payload = r#"{"remaining":"10","thresholdValue":"20","notifiedAt":"2026-10-07T11:25:00Z"}"#;
    Repo::ingest(&pool, event("event-a", payload)).await.unwrap();
    Repo::mark_read(&pool, "owner", "event-a").await.unwrap();
    Repo::mark_resolved(&pool, "owner", "event-a", true).await.unwrap();
    Repo::ingest(&pool, event("event-b", payload)).await.unwrap();
    let history = Repo::list(&pool, "owner", false, 0, 10).await.unwrap();
    assert_eq!(history.total_count, 2);
    assert_eq!(history.data[0].id, "event-b");
    assert!(!history.data[0].is_read);
    assert!(history.data[0].resolved_at.is_none());
    assert!(history.data[1].is_read);
    assert!(history.data[1].resolved_at.is_some());
}

#[tokio::test]
async fn event_id_replay_never_reopens_or_mutates_event_snapshot() {
    let pool = setup_api_wallet_pool("event_history_replay").await;
    let original =
        r#"{"remaining":"10","thresholdValue":"20","notifiedAt":"2026-10-07T11:25:00Z"}"#;
    Repo::ingest(&pool, event("event-a", original)).await.unwrap();
    Repo::mark_read(&pool, "owner", "event-a").await.unwrap();
    Repo::mark_resolved(&pool, "owner", "event-a", true).await.unwrap();
    let before = Repo::find(&pool, "owner", "event-a").await.unwrap().unwrap();
    Repo::ingest(&pool, event("event-a", r#"{"remaining":"1","thresholdValue":"100"}"#))
        .await
        .unwrap();
    let after = Repo::find(&pool, "owner", "event-a").await.unwrap().unwrap();
    assert_eq!(after.payload, original);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.resolved_at, before.resolved_at);
    assert!(after.is_read);
}

#[tokio::test]
async fn event_time_precedes_insertion_order_and_id_breaks_ties_across_pages() {
    let pool = setup_api_wallet_pool("event_history_order").await;
    for (id, time) in [
        ("z-old", "2026-10-07T10:00:00Z"),
        ("a-new", "2026-10-07T11:00:00Z"),
        ("b-new", "2026-10-07T11:00:00Z"),
    ] {
        let payload = format!(r#"{{"notifiedAt":"{time}"}}"#);
        Repo::ingest(&pool, event(id, &payload)).await.unwrap();
    }
    Repo::mark_resolved(&pool, "owner", "a-new", true).await.unwrap();
    let first = Repo::list(&pool, "owner", false, 0, 2).await.unwrap();
    let second = Repo::list(&pool, "owner", false, 1, 2).await.unwrap();
    assert_eq!(first.total_count, 3);
    assert_eq!(first.data.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["b-new", "a-new"]);
    assert_eq!(second.data[0].id, "z-old");
}

#[tokio::test]
async fn submillisecond_event_time_is_not_replaced_by_id_order() {
    let pool = setup_api_wallet_pool("event_history_precision").await;
    Repo::ingest(&pool, event("z-earlier", r#"{"notifiedAt":"2026-10-07T11:00:00.000000001Z"}"#))
        .await
        .unwrap();
    Repo::ingest(&pool, event("a-later", r#"{"notifiedAt":"2026-10-07T11:00:00.000000002Z"}"#))
        .await
        .unwrap();
    let page = Repo::list(&pool, "owner", false, 0, 10).await.unwrap();
    assert_eq!(page.data[0].id, "a-later");
    assert_eq!(page.data[0].created_at.timestamp_subsec_nanos(), 2);
}

#[tokio::test]
async fn initial_notification_schema_supports_event_history_without_upgrade() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../schema/api_wallet/migrations/20260928000000_api_business_notification.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    for id in ["event-a", "event-b"] {
        sqlx::query("INSERT INTO api_business_notification(id,uid,kind,business_key,payload) VALUES(?,'owner','AWM_ADDRESS_INSUFFICIENT','same-object','{}')")
            .bind(id).execute(&pool).await.expect("initial schema must retain each event of the same object");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_business_notification")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let indexes: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='api_business_notification' AND name NOT LIKE 'sqlite_autoindex%' ORDER BY name").fetch_all(&pool).await.unwrap();
    assert_eq!(
        indexes,
        [
            "api_business_notification_object_idx",
            "api_business_notification_owner_active_idx",
            "api_business_notification_owner_time_idx",
            "api_business_notification_time_idx"
        ]
    );
    let created_at: String =
        sqlx::query_scalar("SELECT created_at FROM api_business_notification WHERE id='event-a'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(created_at.len(), 30);
    assert!(chrono::DateTime::parse_from_rfc3339(&created_at).is_ok());
}

#[tokio::test]
async fn observed_group_resolution_is_atomic_and_preserves_read_state() {
    let pool = setup_api_wallet_pool("group_atomic").await;
    for id in ["a-old", "z-new"] {
        Repo::ingest(&pool, event(id, r#"{"thresholdValue":"20"}"#)).await.unwrap();
    }
    Repo::mark_read(&pool, "owner", "a-old").await.unwrap();
    let observed = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    sqlx::query("CREATE TRIGGER reject_old BEFORE UPDATE OF resolved_at ON api_business_notification WHEN OLD.id='a-old' BEGIN SELECT RAISE(FAIL,'reject group write'); END").execute(pool.write_ref()).await.unwrap();
    assert!(Repo::resolve_observed_group(&pool, &observed).await.is_err());
    let unchanged = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    assert!(unchanged.iter().all(|e| e.resolved_at.is_none()));
    assert!(unchanged[1].is_read && !unchanged[0].is_read);
    sqlx::query("DROP TRIGGER reject_old").execute(pool.write_ref()).await.unwrap();
    assert!(Repo::resolve_observed_group(&pool, &observed).await.unwrap());
    let rows = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    assert!(rows.iter().all(|e| e.resolved_at.is_some()));
    assert!(rows[1].is_read && !rows[0].is_read);
}

#[tokio::test]
async fn observed_group_versions_and_uid_are_guarded() {
    let pool = setup_api_wallet_pool("group_versions").await;
    for id in ["old", "latest"] {
        Repo::ingest(&pool, event(id, r#"{"thresholdValue":"20"}"#)).await.unwrap();
    }
    let observed = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    Repo::upsert(&pool, event("old", r#"{"thresholdValue":"100"}"#)).await.unwrap();
    assert!(!Repo::resolve_observed_group(&pool, &observed).await.unwrap());
    assert!(
        Repo::list(&pool, "owner", false, 0, 10)
            .await
            .unwrap()
            .data
            .iter()
            .all(|e| e.resolved_at.is_none())
    );
    let mut foreign = event("foreign", r#"{"thresholdValue":"20"}"#);
    foreign.uid = "other";
    Repo::ingest(&pool, foreign).await.unwrap();
    let mut mixed = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    mixed.push(Repo::find(&pool, "other", "foreign").await.unwrap().unwrap());
    assert!(!Repo::resolve_observed_group(&pool, &mixed).await.unwrap());
    assert!(Repo::find(&pool, "other", "foreign").await.unwrap().unwrap().resolved_at.is_none());
}

#[tokio::test]
async fn observed_group_waits_for_other_writer_before_reading_versions() {
    let pool = setup_api_wallet_pool("group_writer_lock").await;
    Repo::ingest(&pool, event("latest", r#"{"thresholdValue":"20"}"#)).await.unwrap();
    let observed = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    // Independent connection holds the writer lock before the group transaction starts.
    let blocker = pool.read_ref().begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut resolve = Box::pin(Repo::resolve_observed_group(&pool, &observed));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), resolve.as_mut())
            .await
            .is_err(),
        "must wait for writer instead of opening a read snapshot and failing its upgrade"
    );
    blocker.commit().await.unwrap();
    assert!(resolve.await.unwrap());
    assert!(Repo::find(&pool, "owner", "latest").await.unwrap().unwrap().resolved_at.is_some());
}

#[tokio::test]
async fn observed_group_does_not_capture_event_inserted_during_resolution() {
    let pool = setup_api_wallet_pool("group_new_during_write").await;
    for id in ["older", "latest"] {
        Repo::ingest(&pool, event(id, r#"{"thresholdValue":"20"}"#)).await.unwrap();
    }
    let observed = Repo::list(&pool, "owner", false, 0, 10).await.unwrap().data;
    sqlx::query(r#"CREATE TRIGGER new_arrival AFTER UPDATE OF resolved_at ON api_business_notification
        WHEN OLD.id='latest' AND NEW.resolved_at IS NOT NULL BEGIN
        INSERT INTO api_business_notification (id,uid,kind,business_key,payload)
        VALUES ('during-write','owner','AWM_ADDRESS_INSUFFICIENT','owner:tron:address:','{"thresholdValue":"10"}'); END"#).execute(pool.write_ref()).await.unwrap();
    assert!(Repo::resolve_observed_group(&pool, &observed).await.unwrap());
    assert!(Repo::find(&pool, "owner", "latest").await.unwrap().unwrap().resolved_at.is_some());
    assert!(Repo::find(&pool, "owner", "older").await.unwrap().unwrap().resolved_at.is_some());
    assert!(
        Repo::find(&pool, "owner", "during-write").await.unwrap().unwrap().resolved_at.is_none()
    );
}
