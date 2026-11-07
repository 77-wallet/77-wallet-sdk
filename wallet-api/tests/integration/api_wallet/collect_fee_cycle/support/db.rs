use wallet_database::{
    ApiTransactionDbPool,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

use super::fixtures::CollectFeeCycleFixture;

pub(crate) async fn insert_collect(pool: &ApiTransactionDbPool, fixture: &CollectFeeCycleFixture) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        fixture.from_addr,
        fixture.to_addr,
        "1.12",
        "digest",
        "sol",
        fixture.token_addr.clone(),
        fixture.symbol,
        &fixture.trade_no,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert collect");
}

pub(crate) async fn mark_stale_fee_cycle_row(
    pool: &ApiTransactionDbPool,
    fixture: &CollectFeeCycleFixture,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            need_service_fee = true,
            ever_needed_service_fee = true,
            service_fee_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            tx_fee_res_ack_sent_at = NULL,
            raw_tx = NULL,
            tx_hash = NULL,
            last_broadcast_at = NULL,
            transaction_time = NULL,
            err_code = NULL,
            finished_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(&fixture.trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed stale fee-cycle row");
}

pub(crate) async fn mark_waiting_service_fee_row(
    pool: &ApiTransactionDbPool,
    fixture: &CollectFeeCycleFixture,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            need_service_fee = true,
            ever_needed_service_fee = true,
            service_fee_uploaded_at = NULL,
            service_fee_order_received_at = NULL,
            tx_fee_res_ack_sent_at = NULL,
            resource_gate_released_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            raw_tx = NULL,
            tx_hash = NULL,
            last_broadcast_at = NULL,
            transaction_time = NULL,
            err_code = NULL,
            finished_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(&fixture.trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed waiting fee-cycle row");
}

pub(crate) async fn mark_reopened_without_fee_upload(
    pool: &ApiTransactionDbPool,
    fixture: &CollectFeeCycleFixture,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            need_service_fee = false,
            ever_needed_service_fee = true,
            service_fee_uploaded_at = NULL,
            service_fee_order_received_at = NULL,
            tx_fee_res_ack_sent_at = NULL,
            raw_tx = NULL,
            tx_hash = NULL,
            last_broadcast_at = NULL,
            transaction_time = NULL,
            err_code = NULL,
            finished_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(&fixture.trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed reopened fee-cycle row");
}

pub(crate) async fn mark_completed_fee_cycle_row(
    pool: &ApiTransactionDbPool,
    fixture: &CollectFeeCycleFixture,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            need_service_fee = false,
            ever_needed_service_fee = true,
            tx_fee_res_ack_sent_at = NULL,
            raw_tx = NULL,
            tx_hash = NULL,
            last_broadcast_at = NULL,
            transaction_time = NULL,
            finished_at = NULL,
            err_code = NULL,
            service_fee_uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(&fixture.trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed completed fee-cycle row");
}

pub(crate) async fn load_collect(
    pool: &ApiTransactionDbPool,
    fixture: &CollectFeeCycleFixture,
) -> ApiCollectEntity {
    ApiCollectRepo::get_api_collect_by_trade_no(pool, &fixture.trade_no)
        .await
        .expect("load collect after scanner round")
}
