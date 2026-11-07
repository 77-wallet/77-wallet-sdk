use std::path::Path;

use chrono::Utc;
use wallet_database::{
    ApiTransactionDbPool, SqliteContext,
    entities::{
        api_account::CreateApiAccountVo, api_coin::ApiCoinData, api_wallet::ApiWalletType,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{account::ApiAccountRepo, coin::ApiCoinRepo, nonce::ApiNonceRepo},
};

use crate::harness::{self, next_tag, reset_fake, upsert_wallet};

pub(crate) async fn ensure_bnb_transfer_fixture(env: &harness::TestEnv) -> anyhow::Result<String> {
    ensure_evm_transfer_fixture(env, "bnb").await
}

pub(crate) async fn ensure_evm_transfer_fixture(
    env: &harness::TestEnv,
    chain: &str,
) -> anyhow::Result<String> {
    reset_fake(env);

    let api_pool = harness::open_api_wallet_pool(&env.db_dir).await;

    if ApiCoinRepo::coin_by_chain_token_key_opt(chain, AssetTokenKey::Native, &api_pool)
        .await?
        .is_none()
    {
        let now = Utc::now();
        ApiCoinRepo::upsert_multi_coin(
            &api_pool,
            vec![ApiCoinData::new(
                Some(chain.to_string()),
                super::request::native_symbol(chain),
                chain,
                AssetTokenKey::Native,
                Some("0".to_string()),
                None,
                18,
                1,
                1,
                1,
                now,
                Some(now),
            )],
        )
        .await?;
    }

    let wallet_uid = next_tag("wallet-uid");
    let wallet_address =
        upsert_wallet(&env.db_dir, &env.sn, &wallet_uid, ApiWalletType::SubAccount, None).await;

    let account = CreateApiAccountVo::new(
        1,
        &wallet_address,
        "pubkey",
        &wallet_address,
        &wallet_uid,
        "m/44'/60'/0'/0/0",
        0,
        chain,
        "account",
        ApiWalletType::SubAccount,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(&api_pool, vec![account]).await?;

    let tx_pool = open_api_transaction_pool(&env.db_dir).await;
    ApiNonceRepo::set_nonce_floor(&tx_pool, &wallet_address, chain, 0).await?;

    Ok(wallet_address)
}

pub(crate) async fn load_bnb_nonce_floor(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<i64> {
    let tx_pool = open_api_transaction_pool(&env.db_dir).await;
    Ok(ApiNonceRepo::get_api_nonce(&tx_pool, address, "bnb").await?)
}

pub(crate) async fn open_api_transaction_pool(db_dir: &Path) -> ApiTransactionDbPool {
    let sqlite = SqliteContext::new(&db_dir.to_string_lossy(), Some("api_transaction.db"))
        .await
        .expect("open api transaction sqlite");
    sqlite.into_transaction_db_pool().expect("api transaction pool")
}

pub(crate) async fn pending_ordinary_transfers(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<Vec<wallet_database::entities::api_withdraw::ApiWithdrawEntity>> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    Ok(wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::pending_self_transfers_for_sender(&pool, address, "bnb").await?)
}

pub(crate) async fn reject_signed_persistence(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    let address = address.replace("'", "''");
    sqlx::query(&format!("CREATE TRIGGER reject_signed_{} BEFORE INSERT ON api_withdraws WHEN NEW.from_addr = '{}' BEGIN SELECT RAISE(ABORT, 'simulated signed persistence failure'); END", uuid::Uuid::new_v4().simple(), address))
        .execute(pool.write_ref()).await?;
    Ok(())
}

pub(crate) async fn reject_acceptance_persistence(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    let address = address.replace("'", "''");
    sqlx::query(&format!("CREATE TRIGGER reject_acceptance_{} BEFORE UPDATE OF last_broadcast_at ON api_withdraws WHEN NEW.from_addr = '{}' BEGIN SELECT RAISE(ABORT, 'simulated acceptance persistence failure'); END", uuid::Uuid::new_v4().simple(), address))
        .execute(pool.write_ref()).await?;
    Ok(())
}

pub(crate) async fn ordinary_transfer_by_hash(
    env: &harness::TestEnv,
    address: &str,
    hash: &str,
) -> anyhow::Result<wallet_database::entities::api_withdraw::ApiWithdrawEntity> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    Ok(wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo::get_by_hash_and_owner(
        &pool, address, hash,
    )
    .await?)
}

pub(crate) async fn age_self_transfer(env: &harness::TestEnv, address: &str) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    sqlx::query("UPDATE api_withdraws SET broadcast_uncertain_last_checked_at = strftime('%Y-%m-%dT%H:%M:%SZ','now','-61 seconds') WHERE from_addr = ? AND trade_type = 4")
        .bind(address).execute(pool.write_ref()).await?;
    Ok(())
}

pub(crate) async fn corrupt_self_transfer_raw(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    let raw = serde_json::to_string(&wallet_api::domain::api_wallet::RawTx::Evm(
        vec![1, 2, 3],
        alloy::primitives::U256::ZERO,
    ))?;
    sqlx::query("UPDATE api_withdraws SET raw_tx = ? WHERE from_addr = ? AND trade_type = 4")
        .bind(raw)
        .bind(address)
        .execute(pool.write_ref())
        .await?;
    Ok(())
}

pub(crate) async fn overwrite_legacy_status(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    // Reproduce an old application's MQTT overwrite while retaining receipt facts.
    sqlx::query("UPDATE api_withdraws SET status = 9 WHERE from_addr = ? AND trade_type = 4")
        .bind(address)
        .execute(pool.write_ref())
        .await?;
    Ok(())
}

pub(crate) async fn allow_signed_persistence(
    env: &harness::TestEnv,
    address: &str,
) -> anyhow::Result<()> {
    let pool = open_api_transaction_pool(&env.db_dir).await;
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'reject_signed_%' AND instr(sql, ?) > 0")
        .bind(address).fetch_all(pool.read_ref()).await?;
    for name in names {
        sqlx::query(&format!("DROP TRIGGER {}", name)).execute(pool.write_ref()).await?;
    }
    Ok(())
}

pub(crate) async fn seed_signed_collect_six(
    pool: &ApiTransactionDbPool,
    address: &str,
    chain: &str,
) -> anyhow::Result<String> {
    use wallet_database::{
        entities::api_collect::ApiCollectStatus,
        repositories::api_wallet::collect::{ApiCollectRepo, SignedCollectCandidate},
    };
    let trade = next_tag("historical-six");
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "old",
        address,
        "receiver",
        "1",
        "v",
        chain,
        None,
        "ETH",
        &trade,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await?;
    ApiCollectRepo::set_order_ack_sent(pool, &trade).await?;
    ApiCollectRepo::update_building_at(pool, &trade).await?;
    ApiCollectRepo::reserve_signed_collect(
        pool,
        &SignedCollectCandidate {
            trade_no: &trade,
            from_addr: address,
            chain_code: chain,
            nonce: 6,
            chain_next: 6,
            tx_hash: "saved-hash-six",
            raw_tx: "saved-raw-six",
            transaction_fee: "0.1",
        },
    )
    .await?;
    sqlx::query("UPDATE api_collect SET last_broadcast_at=CURRENT_TIMESTAMP WHERE trade_no=?")
        .bind(&trade)
        .execute(pool.write_ref())
        .await?;
    Ok(trade)
}
