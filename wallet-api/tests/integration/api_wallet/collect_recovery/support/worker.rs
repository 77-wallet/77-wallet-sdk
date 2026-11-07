use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;
use wallet_api::{
    Context,
    infrastructure::api_trans::{AddressLockManager, ShadowAdvancer, ShadowCollectWorker},
};
use wallet_database::{
    ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_coin::ApiCoinData, api_collect::ApiCollectStatus, asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{coin::ApiCoinRepo, collect::ApiCollectRepo},
};

use super::fixtures::CollectRecoveryFixture;

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

pub(crate) fn build_shadow_collect_worker_from_pools(ctx: &'static Context) -> ShadowCollectWorker {
    let (intent_tx, _intent_rx) = mpsc::channel(1);
    let advancer = Arc::new(ShadowAdvancer::new(ctx, intent_tx, None));

    ShadowCollectWorker::new(ctx, Arc::new(AddressLockManager::new()), advancer)
}

pub(crate) async fn seed_tron_collect(
    pool: &ApiTransactionDbPool,
    fixture: &CollectRecoveryFixture,
) {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        "from-tron",
        "to-tron",
        "1.1325",
        "digest",
        "tron",
        Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string()),
        "USDT",
        &fixture.trade_no,
        2,
        ApiCollectStatus::SendingTx,
        0,
    )
    .await
    .expect("seed tron collect");
}
