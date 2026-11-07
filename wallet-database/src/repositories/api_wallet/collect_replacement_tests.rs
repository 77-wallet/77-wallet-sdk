use super::*;
use crate::repositories::{
    api_wallet::{
        collect::{ApiCollectRepo, SignedCollectCandidate},
        nonce::ApiNonceRepo,
    },
    test_helper::setup_api_transaction_pool,
};
async fn seed(pool: &ApiTransactionDbPool, trade: &str) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        "sender",
        "receiver",
        "1",
        "v",
        "scroll",
        None,
        "ETH",
        trade,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .unwrap();
    ApiCollectRepo::set_order_ack_sent(pool, trade).await.unwrap();
    ApiCollectRepo::update_building_at(pool, trade).await.unwrap();
}
fn signed(trade: &str) -> SignedCollectCandidate<'_> {
    SignedCollectCandidate {
        trade_no: trade,
        from_addr: "sender",
        chain_code: "scroll",
        tx_hash: if trade == "old" { "old-hash" } else { "new-hash" },
        raw_tx: if trade == "old" { "old-raw" } else { "new-raw" },
        transaction_fee: "0.1",
        nonce: 6,
        chain_next: 6,
    }
}
async fn setup(name: &str) -> (ApiTransactionDbPool, CollectReplacementScope) {
    let pool = setup_api_transaction_pool(name).await;
    seed(&pool, "old").await;
    ApiCollectRepo::reserve_signed_collect(&pool, &signed("old")).await.unwrap();
    ApiCollectRepo::fail_verified_nonce_gap(&pool, "old", 5, 6).await.unwrap();
    ApiNonceRepo::set_nonce_exact(&pool, "sender", "scroll", 153).await.unwrap();
    seed(&pool, "new").await;
    let scope = CollectReplacementScope {
        chain_code: "scroll".into(),
        from_addr: "sender".into(),
        old_trade_no: "old".into(),
        old_hash: "old-hash".into(),
        nonce: 6,
        legacy_counter: 153,
    };
    (pool, scope)
}
#[tokio::test]
async fn replacement_signed_candidate_preserves_old_and_commits_relation() {
    let (p, s) = setup("replacement_signed").await;
    let r = CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap().unwrap();
    assert!(r.new_hash.is_none());
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap(), 1);
    assert_eq!(
        CollectReplacementRepo::find(&p, "new").await.unwrap().unwrap().new_hash.as_deref(),
        Some("new-hash")
    );
    assert_eq!(
        ApiCollectRepo::get_api_collect_by_trade_no(&p, "old").await.unwrap().raw_tx.as_deref(),
        Some("old-raw")
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 153);
}
#[tokio::test]
async fn replacement_winner_recovers_counter_only_after_chain_consumption() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision as D;
    let (p, s) = setup("replacement_checkpoint").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 7).await.unwrap(),
        D::Wait
    );
    let hash = format!("0x{}", "ab".repeat(32));
    let proof = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "new-hash",
        success: true,
        block_hash: &hash,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    assert!(CollectReplacementRepo::resolve(&p, "new", &proof).await.unwrap());
    assert!(!CollectReplacementRepo::resolve(&p, "new", &proof).await.unwrap());
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 6).await.unwrap(),
        D::Wait
    );
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 7).await.unwrap(),
        D::Ready(7)
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 6);
    // A real completed resolution must not authorize ignoring a later floor
    // merely because that floor is below the historical legacy bound.
    ApiNonceRepo::set_nonce_floor(&p, "sender", "scroll", 11).await.unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 7).await.unwrap(),
        D::Wait
    );
    assert!(!CollectReplacementRepo::resolve(&p, "new", &proof).await.unwrap());
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 11);
    // Unknown post-recovery counter changes invalidate the captured authorization.
    ApiNonceRepo::set_nonce_exact(&p, "sender", "scroll", 154).await.unwrap();
    assert_eq!(
        ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 7).await.unwrap(),
        D::Wait
    );
}
#[tokio::test]
async fn replacement_old_winner_fails_new_without_forging_new_success() {
    let (p, s) = setup("replacement_old_wins").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    let hash = format!("0x{}", "cd".repeat(32));
    let proof = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "old-hash",
        success: true,
        block_hash: &hash,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    CollectReplacementRepo::resolve(&p, "new", &proof).await.unwrap();
    let new = ApiCollectRepo::get_api_collect_by_trade_no(&p, "new").await.unwrap();
    assert!(new.err_code.is_some());
    assert!(new.transaction_time.is_none());
    assert_eq!(new.tx_hash.as_deref(), Some("new-hash"));
    assert!(new.err_msg.unwrap().contains("old-hash"));
    let old = ApiCollectRepo::get_api_collect_by_trade_no(&p, "old").await.unwrap();
    assert!(old.err_code.is_some());
}
#[tokio::test]
async fn replacement_scope_and_unique_owner_are_enforced() {
    let (p, s) = setup("replacement_scope").await;
    let mut wrong = s.clone();
    wrong.from_addr = "other".into();
    assert!(CollectReplacementRepo::prepare(&p, "new", &wrong, 6).await.unwrap().is_none());
    assert!(CollectReplacementRepo::prepare(&p, "new", &s, 7).await.unwrap().is_some());
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    seed(&p, "second").await;
    assert!(CollectReplacementRepo::prepare(&p, "second", &s, 6).await.unwrap().is_none());
}

#[tokio::test]
async fn replacement_rejects_already_signed_new_and_stale_identity() {
    let (p, s) = setup("replacement_stale").await;
    sqlx::query("UPDATE api_collect SET raw_tx='other-raw',tx_hash='other-hash',nonce=7 WHERE trade_no='new'").execute(p.write_ref()).await.unwrap();
    assert!(CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap().is_none());
    sqlx::query("UPDATE api_collect SET raw_tx=NULL,tx_hash=NULL,nonce=0 WHERE trade_no='new'")
        .execute(p.write_ref())
        .await
        .unwrap();
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    assert!(
        sqlx::query("UPDATE api_collect SET tx_hash='tampered' WHERE trade_no='new'")
            .execute(p.write_ref())
            .await
            .is_err()
    );
    // Simulate legacy/out-of-band mutation to verify the transaction's own recheck too.
    sqlx::query("DROP TRIGGER api_collect_replacement_identity_guard")
        .execute(p.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE api_collect SET tx_hash='tampered' WHERE trade_no='new'")
        .execute(p.write_ref())
        .await
        .unwrap();
    let h = format!("0x{}", "ab".repeat(32));
    let e = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "old-hash",
        success: true,
        block_hash: &h,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    assert!(CollectReplacementRepo::resolve(&p, "new", &e).await.is_err());
    assert!(CollectReplacementRepo::find(&p, "new").await.unwrap().unwrap().winner_hash.is_none());
}
#[tokio::test]
async fn replacement_winner_write_failure_rolls_back_confirmation_and_checkpoint() {
    let (p, s) = setup("replacement_rollback").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_winner BEFORE UPDATE OF winner_hash ON api_collect_replacement BEGIN SELECT RAISE(ABORT,'winner store rejected'); END").execute(p.write_ref()).await.unwrap();
    let h = format!("0x{}", "ab".repeat(32));
    let e = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "new-hash",
        success: true,
        block_hash: &h,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    assert!(CollectReplacementRepo::resolve(&p, "new", &e).await.is_err());
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&p, "new")
            .await
            .unwrap()
            .transaction_time
            .is_none()
    );
    assert!(CollectReplacementRepo::find(&p, "new").await.unwrap().unwrap().winner_hash.is_none());
}
#[tokio::test]
async fn replacement_concurrent_preparation_has_one_owner() {
    let (p, s) = setup("replacement_race").await;
    seed(&p, "second").await;
    let (a, b) = tokio::join!(
        CollectReplacementRepo::prepare(&p, "new", &s, 6),
        CollectReplacementRepo::prepare(&p, "second", &s, 6)
    );
    assert_eq!(usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()), 1);
}

#[tokio::test]
async fn replacement_unsigned_fee_failure_releases_only_its_prepared_scope() {
    let (p, s) = setup("replacement_fee_failure").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    assert!(
        CollectReplacementRepo::fail_unsigned_attempt(&p, "new", "fee insufficient").await.unwrap()
    );
    let relation = CollectReplacementRepo::find(&p, "new").await.unwrap().unwrap();
    assert!(relation.abandoned_at.is_some());
    assert!(
        ApiCollectRepo::get_api_collect_by_trade_no(&p, "new").await.unwrap().err_code.is_some()
    );
    seed(&p, "next").await;
    assert!(CollectReplacementRepo::prepare(&p, "next", &s, 6).await.unwrap().is_some());
    assert_eq!(
        ApiCollectRepo::get_api_collect_by_trade_no(&p, "old").await.unwrap().raw_tx.as_deref(),
        Some("old-raw")
    );
}
#[tokio::test]
async fn replacement_failed_signed_order_remains_observable_until_consumed() {
    let (p, s) = setup("replacement_failed_signed").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    sqlx::query(
        "UPDATE api_collect SET err_code=6008,finished_at=CURRENT_TIMESTAMP WHERE trade_no='new'",
    )
    .execute(p.write_ref())
    .await
    .unwrap();
    assert_eq!(CollectReplacementRepo::pending_orders(&p, 10).await.unwrap().len(), 1);
    let h = format!("0x{}", "ab".repeat(32));
    let e = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "old-hash",
        success: false,
        block_hash: &h,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    assert!(CollectReplacementRepo::resolve(&p, "new", &e).await.unwrap());
    assert!(CollectReplacementRepo::pending_orders(&p, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn replacement_checkpoint_allows_actual_seven_and_fences_legacy_reservation() {
    let (p, s) = setup("replacement_real_seven").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    assert!(ApiNonceRepo::set_nonce_exact(&p, "sender", "scroll", 154).await.is_err());
    let h = format!("0x{}", "ab".repeat(32));
    let e = ReplacementResolution {
        transaction_fee: "0.000000000000000123",
        resource_consume: "21000",
        winner_hash: "new-hash",
        success: true,
        block_hash: &h,
        block_number: 42,
        transaction_time: "2026-10-19T00:00:00Z",
    };
    CollectReplacementRepo::resolve(&p, "new", &e).await.unwrap();
    seed(&p, "seven").await;
    let mut seven = signed("seven");
    seven.nonce = 7;
    seven.chain_next = 7;
    seven.tx_hash = "seven-hash";
    seven.raw_tx = "seven-raw";
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&p, &seven).await.unwrap(), 1);
    assert_eq!(ApiCollectRepo::get_api_collect_by_trade_no(&p, "seven").await.unwrap().nonce, 7);
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 7);
    // Canonical external consumption can advance beyond the historical bound.
    seed(&p, "external-next").await;
    let mut next = signed("external-next");
    next.nonce = 155;
    next.chain_next = 155;
    next.tx_hash = "next-hash";
    next.raw_tx = "next-raw";
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&p, &next).await.unwrap(), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 155);
}
#[tokio::test]
async fn replacement_migration_preserves_legacy_signed_rows() {
    let (p, _) = setup("replacement_upgrade").await;
    sqlx::raw_sql("DROP TRIGGER api_collect_replacement_identity_guard; DROP TRIGGER api_collect_replacement_counter_fence; DROP TRIGGER api_collect_replacement_counter_insert_fence; DROP TABLE api_collect_replacement;").execute(p.write_ref()).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../schema/api_transaction/migrations/20261019090000_collect_replacement.sql"
    ))
    .execute(p.write_ref())
    .await
    .unwrap();
    assert_eq!(
        ApiCollectRepo::get_api_collect_by_trade_no(&p, "old").await.unwrap().raw_tx.as_deref(),
        Some("old-raw")
    );
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 153);
}
#[tokio::test]
async fn replacement_relation_survives_reopen() {
    let (p, s) = setup("replacement_reopen").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    let filename: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(p.read_ref())
            .await
            .unwrap();
    let directory = std::path::Path::new(&filename).parent().unwrap().to_str().unwrap().to_owned();
    drop(p);
    let reopened = crate::SqliteContext::new(&directory, Some("api_transaction.db"))
        .await
        .unwrap()
        .into_transaction_db_pool()
        .unwrap();
    let r = CollectReplacementRepo::find(&reopened, "new").await.unwrap().unwrap();
    assert_eq!(r.new_raw.as_deref(), Some("new-raw"));
    assert_eq!(r.old_raw, "old-raw");
}

#[tokio::test]
async fn replacement_padded_legacy_sender_is_rejected_before_preparation() {
    let (p, scope) = setup("replacement-padded").await;
    sqlx::query("UPDATE api_collect SET from_addr=' sender ' WHERE trade_no='old'")
        .execute(p.write_ref())
        .await
        .unwrap();
    assert!(CollectReplacementRepo::prepare(&p, "new", &scope, 6).await.unwrap().is_none());
    assert!(CollectReplacementRepo::find(&p, "new").await.unwrap().is_none());
}

#[tokio::test]
async fn replacement_underpriced_reports_requote_but_keeps_signed_watch() {
    let (p, s) = setup("replacement-requote").await;
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    assert!(CollectReplacementRepo::fail_signed_quote(&p, "new").await.unwrap());
    let row = ApiCollectRepo::get_api_collect_by_trade_no(&p, "new").await.unwrap();
    assert!(row.err_msg.unwrap().starts_with("replacement_requires_requote:"));
    assert_eq!(row.raw_tx.as_deref(), Some("new-raw"));
    assert_eq!(CollectReplacementRepo::pending_orders(&p, 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn replacement_case_alias_unknown_counter_still_blocks() {
    let (p, s) = setup("replacement-case-counter").await;
    ApiNonceRepo::set_nonce_exact(&p, "SENDER", "scroll", 154).await.unwrap();
    assert!(CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap().is_none());
}

#[tokio::test]
async fn replacement_retained_scope_is_released_only_by_matching_verified_resolution() {
    let (p, s) = setup("replacement-retained-scope").await;
    assert!(!CollectReplacementRepo::scope_is_resolved(&p, &s).await.unwrap());
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    assert!(!CollectReplacementRepo::scope_is_resolved(&p, &s).await.unwrap());
    let block = format!("0x{}", "ab".repeat(32));
    CollectReplacementRepo::resolve(
        &p,
        "new",
        &ReplacementResolution {
            transaction_fee: "0.001",
            resource_consume: "21000",
            winner_hash: "new-hash",
            success: true,
            block_hash: &block,
            block_number: 42,
            transaction_time: "2026-10-19T00:00:00Z",
        },
    )
    .await
    .unwrap();
    assert!(CollectReplacementRepo::scope_is_resolved(&p, &s).await.unwrap());
    let mut wrong = s.clone();
    wrong.old_hash = "unknown".into();
    assert!(!CollectReplacementRepo::scope_is_resolved(&p, &wrong).await.unwrap());
    wrong = s.clone();
    wrong.from_addr = "other".into();
    assert!(!CollectReplacementRepo::scope_is_resolved(&p, &wrong).await.unwrap());
    wrong = s.clone();
    wrong.legacy_counter = 154;
    assert!(!CollectReplacementRepo::scope_is_resolved(&p, &wrong).await.unwrap());
}

#[tokio::test]
async fn replacement_discovery_uses_persisted_reported_intent_and_counter_without_configuration() {
    let (p, expected) = setup("replacement-auto-discover").await;
    // A local failure is not enough until backend failure upload completed.
    assert!(CollectReplacementRepo::discover(&p, "new").await.unwrap().is_none());
    sqlx::query(
        "UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='old'",
    )
    .execute(p.write_ref())
    .await
    .unwrap();
    let found = CollectReplacementRepo::discover(&p, "new").await.unwrap().unwrap();
    assert_eq!(found, expected);
    assert!(CollectReplacementRepo::prepare(&p, "new", &found, 6).await.unwrap().is_some());
    assert_eq!(ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap(), 1);
    assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 153);
}

#[tokio::test]
async fn replacement_discovery_rejects_other_destination_and_ambiguous_old_intents() {
    let (p, _) = setup("replacement-auto-ambiguous").await;
    sqlx::query(
        "UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='old'",
    )
    .execute(p.write_ref())
    .await
    .unwrap();
    sqlx::query("UPDATE api_collect SET to_addr='other' WHERE trade_no='new'")
        .execute(p.write_ref())
        .await
        .unwrap();
    assert!(CollectReplacementRepo::discover(&p, "new").await.unwrap().is_none());
    sqlx::query("UPDATE api_collect SET to_addr='receiver' WHERE trade_no='new'")
        .execute(p.write_ref())
        .await
        .unwrap();
    seed(&p, "other-old").await;
    sqlx::query("UPDATE api_collect SET nonce=6,tx_hash='another-hash',raw_tx='another-raw',err_code=6008,err_msg=?,tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='other-old'")
        .bind(format!("{} synthetic", crate::entities::api_nonce_claim::COLLECT_NONCE_GAP_REPORTED))
        .execute(p.write_ref()).await.unwrap();
    assert!(CollectReplacementRepo::discover(&p, "new").await.unwrap().is_none());
    assert!(CollectReplacementRepo::find(&p, "new").await.unwrap().is_none());
}

#[tokio::test]
async fn replacement_discovery_does_not_reselect_canonically_resolved_old_intent() {
    let (p, s) = setup("replacement-auto-resolved").await;
    sqlx::query(
        "UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='old'",
    )
    .execute(p.write_ref())
    .await
    .unwrap();
    CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap().unwrap();
    ApiCollectRepo::reserve_signed_collect(&p, &signed("new")).await.unwrap();
    let block = format!("0x{}", "ab".repeat(32));
    CollectReplacementRepo::resolve(
        &p,
        "new",
        &ReplacementResolution {
            transaction_fee: "0.001",
            resource_consume: "21000",
            winner_hash: "new-hash",
            success: true,
            block_hash: &block,
            block_number: 42,
            transaction_time: "2026-10-19T00:00:00Z",
        },
    )
    .await
    .unwrap();
    seed(&p, "next").await;
    assert!(CollectReplacementRepo::discover(&p, "next").await.unwrap().is_none());
}

#[tokio::test]
async fn replacement_automatic_checkpoint_preserves_higher_signed_or_uncertain_ownership() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision as D;
    for signed in [true, false] {
        let (p, _) = setup(if signed {
            "replacement-auto-higher-signed"
        } else {
            "replacement-auto-higher-uncertain"
        })
        .await;
        sqlx::query("UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='old'")
            .execute(p.write_ref()).await.unwrap();
        seed(&p, "higher").await;
        sqlx::query("UPDATE api_collect SET nonce=7,raw_tx=?,tx_hash=?,last_broadcast_at=CURRENT_TIMESTAMP,broadcast_uncertain_since_at=CURRENT_TIMESTAMP WHERE trade_no='higher'")
            .bind(signed.then_some("higher-raw")).bind(signed.then_some("higher-hash"))
            .execute(p.write_ref()).await.unwrap();
        let s = CollectReplacementRepo::discover(&p, "new").await.unwrap().unwrap();
        CollectReplacementRepo::prepare(&p, "new", &s, 6).await.unwrap().unwrap();
        ApiCollectRepo::reserve_signed_collect(&p, &super::tests::signed("new")).await.unwrap();
        let block = format!("0x{}", "ab".repeat(32));
        CollectReplacementRepo::resolve(
            &p,
            "new",
            &ReplacementResolution {
                transaction_fee: "0.001",
                resource_consume: "21000",
                winner_hash: "new-hash",
                success: true,
                block_hash: &block,
                block_number: 42,
                transaction_time: "2026-10-19T00:00:00Z",
            },
        )
        .await
        .unwrap();
        assert_eq!(
            ApiNonceRepo::observe_signed_nonce(&p, "sender", "scroll", 7).await.unwrap(),
            D::Ready(8)
        );
        assert_eq!(ApiNonceRepo::get_api_nonce(&p, "sender", "scroll").await.unwrap(), 7);
        assert_eq!(
            CollectReplacementRepo::find(&p, "new").await.unwrap().unwrap().legacy_counter,
            153
        );
        let higher = ApiCollectRepo::get_api_collect_by_trade_no(&p, "higher").await.unwrap();
        assert!(higher.broadcast_uncertain_since_at.is_some());
        assert_eq!(higher.raw_tx.is_some(), signed);
    }
}

#[tokio::test]
async fn replacement_discovery_and_resolution_use_dynamic_identity_and_nonce() {
    use crate::entities::api_nonce_claim::WithdrawNonceDecision;
    for (chain, sender, nonce, high_water) in [
        ("eth", "sender-alpha", 1_i64, 9_i64),
        ("base", "sender-beta", 42, 97),
        ("arbitrum", "sender-gamma", 1001, 4096),
    ] {
        let (pool, _) = setup(&format!("generic_recovery_{chain}")).await;
        // Move the synthetic history to an unrelated address, chain and nonce.
        sqlx::query("UPDATE api_collect SET chain_code=?,from_addr=?,nonce=CASE WHEN trade_no='old' THEN ? ELSE nonce END")
            .bind(chain).bind(sender).bind(nonce).execute(pool.write_ref()).await.unwrap();
        sqlx::query("UPDATE api_collect SET tx_exec_receipt_uploaded_at=CURRENT_TIMESTAMP WHERE trade_no='old'")
            .execute(pool.write_ref()).await.unwrap();
        ApiNonceRepo::set_nonce_exact(&pool, sender, chain, high_water).await.unwrap();
        let scope = CollectReplacementRepo::discover(&pool, "new").await.unwrap().unwrap();
        assert_eq!(
            (&*scope.chain_code, &*scope.from_addr, scope.nonce, scope.legacy_counter),
            (chain, sender, nonce, high_water)
        );
        assert!(
            CollectReplacementRepo::prepare(&pool, "new", &scope, nonce).await.unwrap().is_some()
        );
        let candidate = SignedCollectCandidate {
            trade_no: "new",
            from_addr: sender,
            chain_code: chain,
            tx_hash: "new-hash",
            raw_tx: "new-raw",
            transaction_fee: "0.1",
            nonce,
            chain_next: nonce,
        };
        ApiCollectRepo::reserve_signed_collect(&pool, &candidate).await.unwrap();
        let block_hash = format!("0x{}", "ab".repeat(32));
        let proof = ReplacementResolution {
            transaction_fee: "0.01",
            resource_consume: "21000",
            winner_hash: "new-hash",
            success: true,
            block_hash: &block_hash,
            block_number: 100,
            transaction_time: "2026-10-19T00:00:00Z",
        };
        assert!(CollectReplacementRepo::resolve(&pool, "new", &proof).await.unwrap());
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, sender, chain).await.unwrap(), nonce);
        assert_eq!(
            ApiNonceRepo::observe_signed_nonce(&pool, sender, chain, nonce + 1).await.unwrap(),
            WithdrawNonceDecision::Ready(nonce + 1)
        );
        assert!(!CollectReplacementRepo::resolve(&pool, "new", &proof).await.unwrap());
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, sender, chain).await.unwrap(), nonce);
        // Another identity's counter is never reset by this recovery.
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "scroll").await.unwrap(), 153);
    }
}
