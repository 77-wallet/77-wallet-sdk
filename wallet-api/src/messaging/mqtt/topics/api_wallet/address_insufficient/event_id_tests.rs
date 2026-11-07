use wallet_database::{
    SqliteContext, repositories::api_business_notification::ApiBusinessNotificationRepo,
};

use crate::messaging::mqtt::{
    message::Message, topics::api_wallet::address_insufficient::AwmAddressInsufficient,
};

fn message(body: &str, uid: Option<&str>, wallet_type: &str) -> Message {
    let mut payload = serde_json::json!({
        "msgId": "balance-alert-1",
        "bizType": "AWM_ADDRESS_INSUFFICIENT",
        "body": serde_json::from_str::<serde_json::Value>(body).unwrap(),
        "clientId": "client",
        "deviceType": "ANDROID",
        "sn": "device",
        "walletType": wallet_type,
    });
    if let Some(uid) = uid {
        payload["uid"] = uid.into();
    }
    serde_json::from_value(payload).unwrap()
}

#[test]
fn confirmed_backend_fields_parse_and_serialize_without_aliases() {
    let body = r#"{"thresholdValue":3000,"remaining":2,"token":"","symbol":"trx","merchantName":"fixture","targetAddress":"TAddress","chainCode":"tron","waddressType":"2"}"#;
    let parsed =
        AwmAddressInsufficient::from_message(&message(body, Some("owner"), "API_WAW")).unwrap();
    assert_eq!(parsed.body.threshold_value, rust_decimal::Decimal::new(3000, 0));
    assert_eq!(parsed.body.remaining, rust_decimal::Decimal::new(2, 0));
    assert_eq!(parsed.body.symbol, "trx");
    assert_eq!(parsed.body.token, "");
    let encoded = serde_json::to_value(&parsed.body).unwrap();
    assert_eq!(encoded["targetAddress"], "TAddress");
    assert_eq!(encoded["waddressType"], "2");
    assert!(encoded.get("address").is_none());
    assert!(encoded.get("wAddressType").is_none());
    let legacy = body.replace("targetAddress", "address").replace("waddressType", "wAddressType");
    assert!(
        AwmAddressInsufficient::from_message(&message(&legacy, Some("owner"), "API_WAW")).is_err()
    );
}

#[test]
fn envelope_parses_balance_alert_and_keeps_outer_uid_for_replay() {
    let body = r#"{
            "remaining": 12.50,
            "token": "",
            "symbol": "TRX",
            "merchantName": "merchant",
            "waddressType": "1",
            "thresholdValue": "20.00",
            "chainCode": "tron",
            "targetAddress": "TAddress"
        }"#;
    let parsed =
        AwmAddressInsufficient::from_message(&message(body, Some("owner"), "API_WAW")).unwrap();
    assert_eq!(parsed.uid, "owner");
    assert_eq!(parsed.body.address, "TAddress");
    assert_eq!(parsed.body.remaining, rust_decimal::Decimal::new(125, 1));
    assert_eq!(parsed.body.threshold_value.to_string(), "20.00");

    let replayed: AwmAddressInsufficient =
        serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
    assert_eq!(replayed.uid, "owner");
    assert_eq!(replayed.received_at, parsed.received_at);
    assert!(replayed.received_at.is_some());
    // Existing durable tasks without the new timestamp still decode.
    let mut legacy = serde_json::to_value(&parsed).unwrap();
    legacy.as_object_mut().unwrap().remove("received_at");
    let legacy: AwmAddressInsufficient = serde_json::from_value(legacy).unwrap();
    assert!(legacy.received_at.is_none());
}

#[test]
fn balance_alert_rejects_missing_identity_or_wrong_wallet() {
    let valid_body = r#"{
            "remaining": 1,
            "token": "",
            "symbol": "TRX",
            "merchantName": "merchant",
            "waddressType": "1",
            "thresholdValue": 20,
            "chainCode": "tron",
            "targetAddress": "TAddress"
        }"#;
    assert!(AwmAddressInsufficient::from_message(&message(valid_body, None, "API_WAW")).is_err());
    assert!(
        AwmAddressInsufficient::from_message(&message(valid_body, Some("owner"), "NORMAL_WALLET"))
            .is_err()
    );
    let missing_address =
        valid_body.replace("\"targetAddress\": \"TAddress\"", "\"targetAddress\": \"\"");
    assert!(
        AwmAddressInsufficient::from_message(&message(&missing_address, Some("owner"), "API_WAW"))
            .is_err()
    );
}

const BODY: &str = r#"{"remaining":10,"token":"","symbol":"TRX","merchantName":"merchant","waddressType":"1","thresholdValue":20,"chainCode":"tron","targetAddress":"TAddress"}"#;

async fn pool() -> wallet_database::ApiWalletDbPool {
    let dir = std::env::temp_dir()
        .join(format!("balance_alert_{}", wallet_utils::snowflake::get_uid().unwrap()));
    std::fs::create_dir_all(&dir).unwrap();
    SqliteContext::new(dir.to_str().unwrap(), Some("api_wallet.db"))
        .await
        .unwrap()
        .into_api_wallet_db_pool()
        .unwrap()
}

#[tokio::test]
async fn redelivery_preserves_snapshot_read_and_resolved_state() {
    let pool = pool().await;
    let first =
        AwmAddressInsufficient::from_message(&message(BODY, Some("owner"), "API_WAW")).unwrap();
    first.persist(&pool, "event-1").await.unwrap();
    let row = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20)
        .await
        .unwrap()
        .data
        .remove(0);
    ApiBusinessNotificationRepo::mark_read(&pool, "owner", &row.id).await.unwrap();
    ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", &row.id, true).await.unwrap();
    let before = ApiBusinessNotificationRepo::find(&pool, "owner", &row.id).await.unwrap().unwrap();
    let changed = AwmAddressInsufficient::from_message(&message(
        &BODY.replace("remaining\":10", "remaining\":5"),
        Some("owner"),
        "API_WAW",
    ))
    .unwrap();
    changed.persist(&pool, "event-1").await.unwrap();
    let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
    assert_eq!(rows.total_count, 1);
    let after = &rows.data[0];
    assert_eq!(after.payload, before.payload);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.is_read, before.is_read);
    assert_eq!(after.resolved_at, before.resolved_at);
}

#[tokio::test]
async fn different_msg_ids_keep_identical_bodies_and_uid_scope() {
    let pool = pool().await;
    let alert =
        AwmAddressInsufficient::from_message(&message(BODY, Some("owner"), "API_WAW")).unwrap();
    alert.persist(&pool, "event-1").await.unwrap();
    alert.persist(&pool, "event-2").await.unwrap();
    let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
    assert_eq!(rows.total_count, 2);
    let mut ids: Vec<_> = rows.data.iter().map(|row| row.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["event-1", "event-2"]);
    assert!(rows.data.iter().all(|r| r.business_key == "owner:tron:TAddress:"));
    let other =
        AwmAddressInsufficient::from_message(&message(BODY, Some("other"), "API_WAW")).unwrap();
    other.persist(&pool, "event-3").await.unwrap();
    assert_eq!(
        ApiBusinessNotificationRepo::list(&pool, "other", false, 0, 20).await.unwrap().total_count,
        1
    );
}

#[tokio::test]
async fn empty_msg_id_rejected_before_queue_and_database() {
    let pool = pool().await;
    let valid = message(BODY, Some("owner"), "API_WAW");
    let alert = AwmAddressInsufficient::from_message(&valid).unwrap();
    for id in ["", " ", "\t\n"] {
        let mut invalid = valid.clone();
        invalid.msg_id = id.into();
        assert!(AwmAddressInsufficient::from_message(&invalid).is_err());
        assert!(alert.persist(&pool, id).await.is_err());
    }
    let mut missing = serde_json::to_value(valid).unwrap();
    missing.as_object_mut().unwrap().remove("msgId");
    assert!(serde_json::from_value::<Message>(missing).is_err());
    assert_eq!(
        ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap().total_count,
        0
    );
}

#[tokio::test]
async fn raw_msg_id_keeps_original_bytes_and_concurrent_replays_deduplicate() {
    let pool = pool().await;
    let alert =
        AwmAddressInsufficient::from_message(&message(BODY, Some("owner"), "API_WAW")).unwrap();
    let id = " event:/原始-id ";
    let (first, second) = tokio::join!(alert.persist(&pool, id), alert.persist(&pool, id));
    assert_eq!(first.unwrap(), id);
    assert_eq!(second.unwrap(), id);
    let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
    assert_eq!(rows.total_count, 1);
    assert_eq!(rows.data[0].id, id);
}

#[tokio::test]
async fn legacy_redelivery_retains_existing_id_and_all_notification_state() {
    let pool = pool().await;
    let alert =
        AwmAddressInsufficient::from_message(&message(BODY, Some("owner"), "API_WAW")).unwrap();
    // Literal v1 fixture is independent of the production encoder.
    let legacy_id =
        "mqtt-event-v1/6f776e6572/41574d5f414444524553535f494e53554646494349454e54/6576656e742d31";
    ApiBusinessNotificationRepo::ingest(
        &pool,
        wallet_database::entities::api_business_notification::NewApiBusinessNotification {
            id: legacy_id,
            uid: "owner",
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: "owner:tron:TAddress:",
            payload: BODY,
        },
    )
    .await
    .unwrap();
    ApiBusinessNotificationRepo::mark_read(&pool, "owner", legacy_id).await.unwrap();
    ApiBusinessNotificationRepo::mark_resolved(&pool, "owner", legacy_id, true).await.unwrap();
    let before =
        ApiBusinessNotificationRepo::find(&pool, "owner", legacy_id).await.unwrap().unwrap();
    let selected_id = alert.persist(&pool, "event-1").await.unwrap();
    assert_eq!(selected_id, legacy_id);
    let rows = ApiBusinessNotificationRepo::list(&pool, "owner", false, 0, 20).await.unwrap();
    assert_eq!(rows.total_count, 1);
    let after = &rows.data[0];
    assert_eq!(after.id, legacy_id);
    assert_eq!(after.payload, before.payload);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
    assert!(after.is_read);
    assert_eq!(after.resolved_at, before.resolved_at);
}
