use super::*;

fn record() -> ApiBusinessNotification {
    let created_at = "2026-10-07T10:28:00Z".parse().unwrap();
    ApiBusinessNotification {
        id: "fixture".into(),
        uid: "owner".into(),
        kind: "AWM_ADDRESS_INSUFFICIENT".into(),
        business_key: "fixture".into(),
        is_read: false,
        resolved_at: None,
        created_at,
        updated_at: "2026-10-07T11:31:00Z".parse().unwrap(),
        payload: serde_json::json!({"remaining":"500.25", "thresholdValue":"600", "token":"",
            "symbol":"USDT", "merchantName":"Demo Shop", "targetAddress":"demo-address",
            "chainCode":"tron", "waddressType":"2", "notifiedAt":"2026-10-07T11:25:00Z"})
        .to_string(),
    }
}

#[test]
fn summary_is_minimal_and_uses_notification_time() {
    // Arrange/Act: last received notification differs from first insertion and DB update.
    let summary = serde_json::to_value(ApiBusinessNotificationSummary::from(record())).unwrap();
    // Assert: exact lightweight public contract, never payload or owner/detail leakage.
    assert_eq!(
        summary,
        serde_json::json!({"id":"fixture", "kind":"AWM_ADDRESS_INSUFFICIENT",
        "isRead":false, "remaining":"500.25", "symbol":"USDT", "notifiedAt":"2026-10-07T11:25:00Z"})
    );
}

#[test]
fn detail_projects_typed_balance_navigation_and_resolved_state() {
    // Arrange: resolved but unread historical notification.
    let mut row = record();
    row.resolved_at = Some(row.updated_at);
    // Act: detail projection.
    let value =
        serde_json::to_value(ApiBusinessNotificationDetail::from_record(row).unwrap()).unwrap();
    // Assert: semantic fields with quantities as strings; navigation is address plus chain.
    assert_eq!(
        value,
        serde_json::json!({"id":"fixture", "kind":"AWM_ADDRESS_INSUFFICIENT",
        "isRead":false, "resolvedAt":"2026-10-07T11:31:00Z", "remaining":"500.25", "token":"",
        "symbol":"USDT", "merchantName":"Demo Shop", "thresholdValue":"600",
        "targetAddress":"demo-address", "chainCode":"tron", "waddressType":"2",
        "notifiedAt":"2026-10-07T11:25:00Z"})
    );
}

#[test]
fn legacy_timestamp_falls_back_only_when_notified_at_absent() {
    // Arrange: valid historic payload without the newer received timestamp.
    let mut row = record();
    let mut payload: serde_json::Value = serde_json::from_str(&row.payload).unwrap();
    payload.as_object_mut().unwrap().remove("notifiedAt");
    row.payload = payload.to_string();
    // Act/Assert: legacy fallback agrees in summary and detail.
    assert_eq!(ApiBusinessNotificationSummary::from(row.clone()).notified_at, Some(row.created_at));
    assert_eq!(
        ApiBusinessNotificationDetail::from_record(row.clone()).unwrap().notified_at,
        row.created_at
    );
    payload["notifiedAt"] = serde_json::json!("broken");
    row.payload = payload.to_string();
    assert!(ApiBusinessNotificationDetail::from_record(row).is_err());
}

#[test]
fn unknown_or_malformed_kind_never_relabels_amount_as_balance_or_leaks_payload() {
    // Arrange/Act/Assert: unknown kind is metadata-only and detail explicitly rejects it.
    for (kind, payload) in [
        ("FUTURE_WITHDRAWAL", "{\"amount\":\"500\"}"),
        ("AWM_ADDRESS_INSUFFICIENT", "{}"),
        ("AWM_ADDRESS_INSUFFICIENT", "invalid"),
    ] {
        let mut row = record();
        row.kind = kind.into();
        row.payload = payload.into();
        let summary =
            serde_json::to_value(ApiBusinessNotificationSummary::from(row.clone())).unwrap();
        assert_eq!(summary["id"], "fixture");
        assert_eq!(summary["kind"], kind);
        assert!(summary["remaining"].is_null());
        assert!(summary["symbol"].is_null());
        assert!(summary["notifiedAt"].is_null());
        assert!(summary.get("payload").is_none());
        assert!(matches!(
            ApiBusinessNotificationDetail::from_record(row),
            Err(ServiceError::Parameter(_))
        ));
    }
}

#[test]
fn existing_business_enum_preserves_wire_string_and_rejects_non_notification_kinds() {
    // Arrange/Act: reuse MQTT enum, but only persisted business reminders are supported.
    let kind = BizType::from_business_notification_kind("AWM_ADDRESS_INSUFFICIENT").unwrap();
    // Assert: enum JSON remains the exact existing string, never a numeric discriminant.
    assert_eq!(serde_json::to_value(&kind).unwrap(), serde_json::json!("AWM_ADDRESS_INSUFFICIENT"));
    assert_eq!(kind.business_notification_kind(), Some("AWM_ADDRESS_INSUFFICIENT"));
    assert!(matches!(
        serde_json::from_str::<BizType>(r#""AWM_ADDRESS_INSUFFICIENT""#).unwrap(),
        BizType::AwmAddressInsufficient
    ));
    for kind in [
        "FUTURE_WITHDRAWAL",
        "AWM_ORDER_TRANS",
        "AwmAddressInsufficient",
        "1",
        "",
        " AWM_ADDRESS_INSUFFICIENT",
    ] {
        assert!(BizType::from_business_notification_kind(kind).is_none());
    }
    assert!(BizType::AwmOrderTrans.business_notification_kind().is_none());
}
