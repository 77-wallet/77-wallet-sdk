use chrono::Utc;
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_coin::ApiCoinData,
        api_resource_delegation::ApiResourceDelegationResultStatus,
        api_resource_gate::{ApiResourceBlockReason, ApiResourceDependencyType},
        api_trade_type::ApiTradeType,
        api_withdraw::ApiWithdrawStatus,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{coin::ApiCoinRepo, withdraw::ApiWithdrawRepo},
};

pub(crate) async fn seed_withdraw(pool: &ApiTransactionDbPool, trade_no: &str) {
    ApiWithdrawRepo::upsert_api_withdraw(
        pool,
        "uid",
        "withdraw",
        "from",
        "to",
        "1.12",
        "digest",
        "tron",
        None,
        "TRX",
        trade_no,
        None,
        None,
        None,
        ApiTradeType::Withdraw,
        1,
        None,
        ApiWithdrawStatus::AuditPass,
        ApiWithdrawStatus::InitOrder,
        "",
        "",
        None,
        None,
    )
    .await
    .expect("insert withdraw");
}

pub(crate) async fn seed_tron_withdraw_resource_gate_basics(wallet_pool: &ApiWalletDbPool) {
    let now = Utc::now();
    ApiCoinRepo::upsert_multi_coin(
        wallet_pool,
        vec![
            ApiCoinData::new(
                Some("TRON".to_string()),
                "TRX",
                "tron",
                AssetTokenKey::Native,
                Some("0".to_string()),
                None,
                6,
                1,
                1,
                1,
                now,
                Some(now),
            ),
            ApiCoinData::new(
                Some("Tether USD".to_string()),
                "USDT",
                "tron",
                AssetTokenKey::Contract("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
                Some("0".to_string()),
                None,
                6,
                0,
                1,
                1,
                now,
                Some(now),
            ),
        ],
    )
    .await
    .expect("seed tron withdraw coins");
}

pub(crate) async fn seed_same_address_withdraw_resource_window(
    pool: &ApiTransactionDbPool,
    first_trade_no: &str,
    second_trade_no: &str,
    from_addr: &str,
) {
    seed_withdraw_order(pool, first_trade_no, from_addr, "2026-07-20T00:00:00Z").await;
    seed_withdraw_order(pool, second_trade_no, from_addr, "2026-07-20T00:00:01Z").await;

    sqlx::query(
        r#"
        UPDATE api_withdraws
        SET estimated_resource_consume = '{"bandwidth":345,"energy":14650}'
        WHERE trade_no = ?
        "#,
    )
    .bind(first_trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed pending withdraw resource budget");
}

async fn seed_withdraw_order(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    from_addr: &str,
    created_at: &str,
) {
    ApiWithdrawRepo::upsert_api_withdraw(
        pool,
        "uid",
        "withdraw",
        from_addr,
        "TReceiver11111111111111111111111111111",
        "1.12",
        "digest",
        "tron",
        Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
        "USDT",
        trade_no,
        None,
        None,
        None,
        ApiTradeType::Withdraw,
        1,
        None,
        ApiWithdrawStatus::AuditPass,
        ApiWithdrawStatus::InitOrder,
        "",
        "",
        None,
        None,
    )
    .await
    .expect("insert withdraw resource gate order");

    sqlx::query(
        r#"
        UPDATE api_withdraws
        SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            created_at = ?,
            updated_at = ?
        WHERE trade_no = ?
        "#,
    )
    .bind(created_at)
    .bind(created_at)
    .bind(trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed withdraw resource gate facts");
}

pub(crate) async fn mark_withdraw_blocked(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    resource_trade_no: &str,
) {
    sqlx::query(
        r#"
        UPDATE api_withdraws
        SET tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            audit_passed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            resource_block_reason = ?,
            resource_dependency_trade_no = ?,
            resource_dependency_type = ?
        WHERE trade_no = ?
        "#,
    )
    .bind(ApiResourceBlockReason::NeedPlatformDelegate.as_i64())
    .bind(resource_trade_no)
    .bind(ApiResourceDependencyType::PlatformDelegate.as_i64())
    .bind(trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed blocked withdraw");
}

pub(crate) async fn seed_resource_delegation_ready_for_ack(
    pool: &ApiResourceDbPool,
    origin_trade_no: &str,
    resource_trade_no: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            result_status, result_received_at, result_payload,
            tx_res_received_at, created_at, updated_at
        ) VALUES (
            'uid', 1, 1, ?, 1,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), '{"status":true}',
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(origin_trade_no)
    .bind(resource_trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed withdraw delegation row for result ack");
}

pub(crate) async fn seed_failed_original_order_resource_result(
    pool: &ApiResourceDbPool,
    trade_no: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            result_status, result_received_at, result_payload,
            tx_res_received_at, created_at, updated_at
        ) VALUES (
            'uid', 1, 1, ?, 1,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), '{"status":false}',
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(trade_no)
    .bind(trade_no)
    .bind(ApiResourceDelegationResultStatus::Fail as i64)
    .execute(pool.as_ref())
    .await
    .expect("seed failed original-order withdraw resource result");
}

pub(crate) async fn seed_successful_resource_delegation(
    pool: &ApiResourceDbPool,
    origin: Option<(&str, ApiTradeType)>,
    resource_trade_no: &str,
    tx_hash: &str,
) {
    let (origin_trade_no, origin_trade_type) = origin
        .map_or((None, ApiTradeType::Withdraw), |(trade_no, trade_type)| {
            (Some(trade_no), trade_type)
        });

    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            tx_hash, tx_status, result_status, result_received_at, result_payload,
            tx_res_received_at, created_at, updated_at
        ) VALUES (
            'uid', 1, 1, ?, ?,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            ?, 'success', 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), '{"status":true}',
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(origin_trade_no)
    .bind(origin_trade_type as i64)
    .bind(resource_trade_no)
    .bind(tx_hash)
    .execute(pool.as_ref())
    .await
    .expect("seed successful resource delegation row");
}

pub(crate) async fn seed_failed_resource_delegation(
    pool: &ApiResourceDbPool,
    origin_trade_no: &str,
    origin_trade_type: ApiTradeType,
    resource_trade_no: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            err_code, err_msg, tx_status,
            created_at, updated_at
        ) VALUES (
            'uid', 1, 1, ?, ?,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            'delegate_failed', 'delegate failed', 'fail',
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(origin_trade_no)
    .bind(origin_trade_type as i64)
    .bind(resource_trade_no)
    .execute(pool.as_ref())
    .await
    .expect("seed failed withdraw delegation row");
}
