use chrono::Utc;
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool,
    entities::{
        api_account::CreateApiAccountVo,
        api_coin::ApiCoinData,
        api_collect::ApiCollectStatus,
        api_resource_gate::{ApiResourceBlockReason, ApiResourceDependencyType},
        api_trade_type::ApiTradeType,
        api_wallet::ApiWalletType,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, coin::ApiCoinRepo, collect::ApiCollectRepo, wallet::ApiWalletRepo,
    },
};

pub(crate) async fn seed_blocked_collect(collect_pool: &ApiTransactionDbPool, trade_no: &str) {
    ApiCollectRepo::upsert_api_collect(
        collect_pool,
        "uid",
        "collect",
        "from",
        "to",
        "1.12",
        "digest",
        "tron",
        None,
        "TRX",
        trade_no,
        ApiTradeType::Collect as u8,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert collect");

    sqlx::query(
        "UPDATE api_collect SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE trade_no = ?",
    )
    .bind(trade_no)
    .execute(collect_pool.as_ref())
    .await
    .expect("seed order ack");

    ApiCollectRepo::mark_resource_blocked(
        collect_pool,
        trade_no,
        ApiResourceBlockReason::NeedPlatformDelegate,
        Some(&format!("rsc_delegate_{trade_no}")),
        Some(ApiResourceDependencyType::PlatformDelegate),
    )
    .await
    .expect("seed blocked collect");
}

pub(crate) async fn seed_tron_collect_resource_gate_basics(
    wallet_pool: &wallet_database::ApiWalletDbPool,
    uid: &str,
) {
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
    .expect("seed tron coins");

    ApiWalletRepo::upsert(
        wallet_pool,
        uid,
        "collect resource gate wallet",
        "TWithdrawal1111111111111111111111111111",
        &[],
        &[],
        ApiWalletType::SubAccount,
        None,
        "sn-collect-resource-gate",
        0,
    )
    .await
    .expect("seed api wallet");
}

pub(crate) async fn seed_same_address_collect_resource_window(
    wallet_pool: &wallet_database::ApiWalletDbPool,
    collect_pool: &ApiTransactionDbPool,
    first_trade_no: &str,
    second_trade_no: &str,
    from_addr: &str,
    uid: &str,
) {
    let account = CreateApiAccountVo::new(
        1,
        from_addr,
        "pubkey",
        "TWithdrawal1111111111111111111111111111",
        uid,
        "m/44'/195'/0'/0/0",
        0,
        "tron",
        "collect resource gate account",
        ApiWalletType::SubAccount,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(wallet_pool, vec![account])
        .await
        .expect("seed api account");

    seed_collect_order(collect_pool, first_trade_no, from_addr, uid, "2026-07-20T00:00:00Z").await;
    seed_collect_order(collect_pool, second_trade_no, from_addr, uid, "2026-07-20T00:00:01Z").await;

    ApiCollectRepo::mark_resource_released(
        collect_pool,
        first_trade_no,
        wallet_database::entities::api_resource_gate::ApiResourceGateResult::PlatformDelegateSuccess,
    )
    .await
    .expect("release first collect resource gate");
}

pub(crate) async fn seed_tron_native_collect_order(
    wallet_pool: &wallet_database::ApiWalletDbPool,
    collect_pool: &ApiTransactionDbPool,
    trade_no: &str,
) {
    let uid = "uid_collect_native_shortage";
    seed_tron_collect_resource_gate_basics(wallet_pool, uid).await;

    ApiCollectRepo::upsert_api_collect(
        collect_pool,
        uid,
        "collect",
        "TCollectAmountShortage111111111111111111",
        "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5",
        "5.00100",
        "digest",
        "tron",
        None,
        "TRX",
        trade_no,
        ApiTradeType::Collect as u8,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert native collect order");

    sqlx::query(
        "UPDATE api_collect SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE trade_no = ?",
    )
    .bind(trade_no)
    .execute(collect_pool.as_ref())
    .await
    .expect("seed native collect order ack");
}

pub(crate) async fn seed_tron_token_collect_with_fee_funding_wallet(
    wallet_pool: &wallet_database::ApiWalletDbPool,
    collect_pool: &ApiTransactionDbPool,
    trade_no: &str,
    funding_addr: &str,
) {
    let uid = "uid_collect_fee_reservation";
    let sub_wallet_addr = "TSubWalletFeeReservation111111111111111";
    let collect_from = "TCollectFeeReservation11111111111111111";
    seed_tron_collect_resource_gate_basics(wallet_pool, uid).await;

    ApiWalletRepo::upsert(
        wallet_pool,
        uid,
        "collect fee reservation sub wallet",
        sub_wallet_addr,
        &[],
        &[],
        ApiWalletType::SubAccount,
        Some(funding_addr),
        "sn-collect-fee-reservation-sub",
        0,
    )
    .await
    .expect("seed bound sub wallet");
    ApiWalletRepo::upsert(
        wallet_pool,
        "uid_collect_fee_funding",
        "collect fee funding wallet",
        funding_addr,
        &[],
        &[],
        ApiWalletType::Withdrawal,
        None,
        "sn-collect-fee-funding",
        0,
    )
    .await
    .expect("seed funding wallet");
    let account = CreateApiAccountVo::new(
        1,
        collect_from,
        "pubkey",
        sub_wallet_addr,
        uid,
        "m/44'/195'/0'/0/0",
        0,
        "tron",
        "collect fee reservation account",
        ApiWalletType::SubAccount,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(wallet_pool, vec![account])
        .await
        .expect("seed collect fee reservation account");
    seed_collect_order(collect_pool, trade_no, collect_from, uid, "2026-08-06T00:00:00Z").await;
}

async fn seed_collect_order(
    collect_pool: &ApiTransactionDbPool,
    trade_no: &str,
    from_addr: &str,
    uid: &str,
    created_at: &str,
) {
    ApiCollectRepo::upsert_api_collect(
        collect_pool,
        uid,
        "collect",
        from_addr,
        "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5",
        "3.000000",
        "digest",
        "tron",
        Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
        "USDT",
        trade_no,
        ApiTradeType::Collect as u8,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert collect order");

    sqlx::query(
        r#"
        UPDATE api_collect
        SET order_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            created_at = ?,
            updated_at = ?
        WHERE trade_no = ?
        "#,
    )
    .bind(created_at)
    .bind(created_at)
    .bind(trade_no)
    .execute(collect_pool.as_ref())
    .await
    .expect("seed collect order facts");
}

pub(crate) async fn seed_resource_result(
    collect_pool: &ApiTransactionDbPool,
    resource_pool: &ApiResourceDbPool,
    trade_no: &str,
    resource_trade_no: &str,
    origin_trade_type: ApiTradeType,
    success: bool,
) {
    let (tx_status, err_code, err_msg, result_status, result_payload) = if success {
        ("success", None, None, 1_i64, r#"{"status":true}"#)
    } else {
        ("fail", Some("ERR_6008"), Some("delegate failed"), 2_i64, r#"{"status":false}"#)
    };

    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            tx_hash, tx_status, err_code, err_msg, result_status, result_received_at, result_payload,
            tx_res_received_at, created_at, updated_at
        ) VALUES (
            'uid', 1, 1, ?, ?,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            'tx_hash_collect_resource', ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?,
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(trade_no)
    .bind(origin_trade_type as i64)
    .bind(resource_trade_no)
    .bind(tx_status)
    .bind(err_code)
    .bind(err_msg)
    .bind(result_status)
    .bind(result_payload)
    .execute(resource_pool.as_ref())
    .await
    .expect("seed resource delegation row for result ack");

    if !success && origin_trade_type == ApiTradeType::Collect {
        ApiCollectRepo::mark_resource_blocked(
            collect_pool,
            trade_no,
            ApiResourceBlockReason::NeedLocalDelegate,
            None,
            Some(ApiResourceDependencyType::LocalDelegate),
        )
        .await
        .expect("seed collect local delegate fallback");
    }
}

pub(crate) async fn mark_collect_chain_success(
    collect_pool: &ApiTransactionDbPool,
    trade_no: &str,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET transaction_time = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            tx_hash = 'collect_chain_success_hash',
            block_height = '12345',
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(trade_no)
    .execute(collect_pool.as_ref())
    .await
    .expect("mark collect chain success");
}

pub(crate) async fn seed_failed_resource_receipt_row(
    resource_pool: &ApiResourceDbPool,
    trade_no: Option<&str>,
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
    .bind(trade_no)
    .bind(ApiTradeType::Collect as i64)
    .bind(resource_trade_no)
    .execute(resource_pool.as_ref())
    .await
    .expect("seed failed collect delegation row");
}

pub(crate) async fn insert_failed_delegation_ready_for_receipt_scan(
    resource_pool: &ApiResourceDbPool,
    resource_trade_no: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO api_resource_delegation (
            uid, source, operation_type, origin_trade_no, origin_trade_type,
            resource_trade_no, chain_code, owner_address, receiver_address,
            resource_type, native_amount, amount, status,
            task_ack_sent_at, building_at, tx_status, err_code, err_msg,
            created_at, updated_at
        ) VALUES (
            'uid', 1, 1, NULL, 2,
            ?, 'tron', 'owner', 'receiver',
            1, '2', '32000', 3,
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            'fail', 'ERR_6008', 'sdk internal error',
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        )
        "#,
    )
    .bind(resource_trade_no)
    .execute(resource_pool.as_ref())
    .await
    .expect("seed failed resource delegation row");
}
