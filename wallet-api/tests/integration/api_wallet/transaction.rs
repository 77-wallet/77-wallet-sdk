use chrono::Utc;
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::{
        api_coin::ApiCoinData, api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{coin::ApiCoinRepo, withdraw::ApiWithdrawRepo},
};

use crate::harness;

#[tokio::test]
async fn api_bill_detail_accepts_current_api_resource_payload() {
    // Given an API withdraw row matching the device log shape.
    let env = harness::ensure_env().await;
    let api_wallet_pool = harness::open_api_wallet_pool(&env.db_dir).await;
    ensure_tron_main_coin(&api_wallet_pool).await;

    let tx_pool = open_api_transaction_pool(&env.db_dir).await;
    let owner = harness::next_tag("TWithdrawOwner");
    let tx_hash = harness::next_tag("tron-hash");
    ApiWithdrawRepo::upsert_api_withdraw(
        &tx_pool,
        "withdraw-uid",
        "withdraw-wallet",
        &owner,
        "TReceiver",
        "1",
        "",
        "tron",
        "TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3",
        "JST",
        &harness::next_tag("trade"),
        None,
        None,
        None,
        ApiTradeType::Withdraw,
        0,
        Some(tx_hash.clone()),
        ApiWithdrawStatus::ConfirmSuccessReport,
        ApiWithdrawStatus::ConfirmSuccessReport,
        r#"{"bandwidth":0,"energy":0}"#,
        "0.344",
        Some(Utc::now()),
        Some("68543373".to_string()),
    )
    .await
    .expect("seed api withdraw");

    // When the public API wallet detail entrypoint is called.
    let detail = env
        .manager
        .api_bill_detail(&tx_hash, &owner)
        .await
        .expect("api_bill_detail should accept API resource payload");

    // Then it returns API wallet resource fields instead of standard wallet fields.
    let json = serde_json::to_value(detail).expect("serialize detail");
    assert_eq!(json["resourceConsume"]["bandwidth"].as_u64(), Some(0));
    assert_eq!(json["resourceConsume"]["energy"].as_u64(), Some(0));
    assert!(json["resourceConsume"].get("netUsed").is_none());
    assert!(json["resourceConsume"].get("energyUsed").is_none());
}

async fn ensure_tron_main_coin(pool: &wallet_database::ApiWalletDbPool) {
    if ApiCoinRepo::coin_by_chain_token_key_opt("tron", AssetTokenKey::Native, pool)
        .await
        .expect("query tron main coin")
        .is_some()
    {
        return;
    }

    let now = Utc::now();
    ApiCoinRepo::upsert_multi_coin(
        pool,
        vec![ApiCoinData::new(
            Some("TRON".to_string()),
            "TRX",
            "tron",
            AssetTokenKey::Native,
            Some("0".to_string()),
            Some("TRC20".to_string()),
            6,
            1,
            1,
            1,
            now,
            Some(now),
        )],
    )
    .await
    .expect("insert tron main coin");
}

async fn open_api_transaction_pool(db_dir: &std::path::Path) -> ApiTransactionDbPool {
    let sqlite = SqliteContext::new(&db_dir.to_string_lossy(), Some("api_transaction.db"))
        .await
        .expect("open api transaction sqlite");
    sqlite.into_transaction_db_pool().expect("api transaction pool")
}
