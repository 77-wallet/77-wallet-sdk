use std::sync::Arc;

use crate::harness::{WorkerTestEnv, open_api_wallet_pool};
use chrono::Utc;
use tokio::sync::mpsc;
use wallet_api::infrastructure::api_trans::{
    AddressLockManager, ShadowAdvancer, ShadowCollectWorker,
};
use wallet_database::{
    ApiWalletDbPool,
    entities::{api_coin::ApiCoinData, asset_token_key::AssetTokenKey},
    repositories::api_wallet::coin::ApiCoinRepo,
};

pub(crate) async fn build_shadow_collect_worker(env: &WorkerTestEnv) -> ShadowCollectWorker {
    let core_pool = open_api_wallet_pool(&env.db_dir).await;
    ensure_sol_main_coin(&core_pool).await;
    let (intent_tx, _intent_rx) = mpsc::channel(1);
    let advancer = Arc::new(ShadowAdvancer::new(env.ctx(), intent_tx.clone(), None));

    ShadowCollectWorker::new(env.ctx(), Arc::new(AddressLockManager::new()), advancer)
}

pub(crate) async fn ensure_eth_main_coin(pool: &ApiWalletDbPool) {
    let now = Utc::now();
    let coin = ApiCoinData::new(
        Some("Ethereum".to_string()),
        "ETH",
        "eth",
        AssetTokenKey::Native,
        Some("0".to_string()),
        None,
        18,
        1,
        1,
        1,
        now,
        Some(now),
    );
    ApiCoinRepo::upsert_multi_coin(pool, vec![coin]).await.expect("seed eth main coin");
}

pub(crate) async fn build_eth_shadow_collect_worker(env: &WorkerTestEnv) -> ShadowCollectWorker {
    let core_pool = open_api_wallet_pool(&env.db_dir).await;
    ensure_eth_main_coin(&core_pool).await;
    let (intent_tx, _intent_rx) = mpsc::channel(1);
    let advancer = Arc::new(ShadowAdvancer::new(env.ctx(), intent_tx.clone(), None));

    ShadowCollectWorker::new(env.ctx(), Arc::new(AddressLockManager::new()), advancer)
}

pub(crate) async fn ensure_sol_main_coin(pool: &ApiWalletDbPool) {
    let now = Utc::now();
    let coin = ApiCoinData::new(
        Some("Solana".to_string()),
        "SOL",
        "sol",
        AssetTokenKey::Native,
        Some("0".to_string()),
        None,
        9,
        1,
        1,
        1,
        now,
        Some(now),
    );
    ApiCoinRepo::upsert_multi_coin(pool, vec![coin]).await.expect("seed sol main coin");
}
