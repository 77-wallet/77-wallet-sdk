use crate::harness::{
    WorkerTestEnv, decrypt_captured_api_backend_body, next_unique_id, open_api_wallet_pool,
    upsert_wallet,
};
use chrono::Utc;
use serde_json::Value;
use wallet_api::{Context, testkit::collect::upload_collect_service_fee_via_worker};
use wallet_database::{
    ApiTransactionDbPool, ApiWalletDbPool, SqliteContext,
    entities::{
        api_account::CreateApiAccountVo, api_coin::ApiCoinData, api_collect::ApiCollectStatus,
        api_wallet::ApiWalletType, api_withdraw_strategy::ApiWithdrawStrategyEntity,
        api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigEntity,
        asset_token_key::AssetTokenKey,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, coin::ApiCoinRepo, collect::ApiCollectRepo,
        withdraw_strategy::ApiWithdrawStrategyRepo,
        withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigRepo,
    },
};

use super::{ensure_eth_main_coin, ensure_sol_main_coin, seed_eth_collect_order};

pub(crate) struct ServiceFeeUploadScenario {
    pub ctx: &'static Context,
    pub trade_no: String,
    pub from_addr: String,
    pub to_addr: String,
    pub token_code: &'static str,
    pub contract_address: &'static str,
}

pub(crate) async fn given_sol_service_fee_upload_waiting(
    env: &WorkerTestEnv,
    trade_prefix: &str,
) -> ServiceFeeUploadScenario {
    let collect_pool = open_transaction_pool(env).await;
    let core_pool = open_api_wallet_pool(&env.db_dir).await;
    ensure_sol_main_coin(&core_pool).await;
    seed_sol_usdc_coin(&core_pool).await;

    let trade_no = format!("{trade_prefix}_{}", next_unique_id());
    let collect_uid = format!("collect-uid-{}", next_unique_id());
    let withdrawal_uid = format!("withdraw-uid-{}", next_unique_id());
    let from_addr = "DLcQZyqoL7ghnENR4mboeuivCNAKXBWJ8RKQA9aK3ZW8";
    let to_addr = "72vgdLcQgdudUiGXudHNPhgCPNPCdxj2ijAGuXTQ5ppB";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    seed_service_fee_wallet_account_and_strategy(
        env,
        &core_pool,
        &collect_uid,
        &withdrawal_uid,
        from_addr,
        to_addr,
        "sol",
        "m/44'/501'/0'/0/0",
    )
    .await;

    ApiCollectRepo::upsert_api_collect(
        &collect_pool,
        &collect_uid,
        "collect",
        from_addr,
        to_addr,
        "1.1",
        "digest",
        "sol",
        Some(usdc_mint.to_string()),
        "USDC",
        &trade_no,
        2,
        ApiCollectStatus::InsufficientBalance,
        1,
    )
    .await
    .expect("seed collect row");
    mark_collect_waiting_for_service_fee(&collect_pool, &trade_no, "seed fee-wait row").await;

    ServiceFeeUploadScenario {
        ctx: env.ctx(),
        trade_no,
        from_addr: from_addr.to_string(),
        to_addr: to_addr.to_string(),
        token_code: "SOL",
        contract_address: "",
    }
}

pub(crate) async fn given_eth_service_fee_upload_waiting(
    env: &WorkerTestEnv,
) -> ServiceFeeUploadScenario {
    let collect_pool = open_transaction_pool(env).await;
    let core_pool = open_api_wallet_pool(&env.db_dir).await;
    ensure_eth_main_coin(&core_pool).await;

    let trade_no = format!("T_collect_eth_fee_upload_{}", next_unique_id());
    let collect_uid = format!("collect-uid-{}", next_unique_id());
    let withdrawal_uid = format!("withdraw-uid-{}", next_unique_id());
    let from_addr = "0xFCa230313618af2a33fa00455D8A5d1466C91332";
    let to_addr = "0x477000C778C66FaAA36596Fb846Ce34C89bc652D";

    seed_service_fee_wallet_account_and_strategy(
        env,
        &core_pool,
        &collect_uid,
        &withdrawal_uid,
        from_addr,
        to_addr,
        "eth",
        "m/44'/60'/0'/0/0",
    )
    .await;
    seed_eth_collect_order(&collect_pool, &trade_no, from_addr, to_addr, "0.00078558").await;
    mark_collect_waiting_for_service_fee(&collect_pool, &trade_no, "seed eth fee-wait row").await;

    ServiceFeeUploadScenario {
        ctx: env.ctx(),
        trade_no,
        from_addr: from_addr.to_string(),
        to_addr: to_addr.to_string(),
        token_code: "ETH",
        contract_address: "",
    }
}

pub(crate) async fn when_upload_collect_service_fee(
    scenario: &ServiceFeeUploadScenario,
    expect_msg: &str,
) {
    upload_collect_service_fee_via_worker(scenario.ctx, &scenario.trade_no)
        .await
        .expect(expect_msg);
}

pub(crate) fn then_service_fee_upload_payload(env: &WorkerTestEnv, trade_no: &str) -> Value {
    let requests = env.recorder.snapshot();
    let request = requests
        .iter()
        .find(|req| {
            req.path.contains(
                wallet_transport_backend::consts::endpoint::api_wallet::TRANS_SERVICE_FEE_TRANS,
            ) && decrypt_captured_api_backend_body(&req.body)["tradeNo"].as_str() == Some(trade_no)
        })
        .unwrap_or_else(|| {
            panic!(
                "service fee upload must call the fee-trans endpoint, captured paths: {:?}",
                requests.iter().map(|req| req.path.clone()).collect::<Vec<_>>()
            )
        });

    decrypt_captured_api_backend_body(&request.body)
}

async fn open_transaction_pool(env: &WorkerTestEnv) -> ApiTransactionDbPool {
    let collect_pool_ctx =
        SqliteContext::new(&env.db_dir.to_string_lossy(), Some("api_transaction.db"))
            .await
            .expect("open api transaction sqlite");
    collect_pool_ctx.into_transaction_db_pool().expect("transaction pool")
}

async fn seed_sol_usdc_coin(core_pool: &ApiWalletDbPool) {
    let now = Utc::now();
    ApiCoinRepo::upsert_multi_coin(
        core_pool,
        vec![ApiCoinData::new(
            Some("Solana".to_string()),
            "USDC",
            "sol",
            AssetTokenKey::Contract("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string()),
            Some("0".to_string()),
            None,
            6,
            1,
            1,
            1,
            now,
            Some(now),
        )],
    )
    .await
    .expect("seed sol usdc coin");
}

async fn seed_service_fee_wallet_account_and_strategy(
    env: &WorkerTestEnv,
    core_pool: &ApiWalletDbPool,
    collect_uid: &str,
    withdrawal_uid: &str,
    from_addr: &str,
    to_addr: &str,
    chain_code: &str,
    derivation_path: &str,
) {
    let withdrawal_wallet =
        upsert_wallet(&env.db_dir, "sn-collect", withdrawal_uid, ApiWalletType::Withdrawal, None)
            .await;
    let subaccount_wallet = upsert_wallet(
        &env.db_dir,
        "sn-collect",
        collect_uid,
        ApiWalletType::SubAccount,
        Some(&withdrawal_wallet),
    )
    .await;

    let account = CreateApiAccountVo::new(
        1,
        from_addr,
        "pubkey",
        &subaccount_wallet,
        collect_uid,
        derivation_path,
        0,
        chain_code,
        "account",
        ApiWalletType::SubAccount,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(core_pool, vec![account])
        .await
        .expect("seed collect account");

    let withdraw_strategy = ApiWithdrawStrategyEntity {
        id: 0,
        uid: withdrawal_uid.to_string(),
        threshold: 50,
        created_at: Utc::now(),
        updated_at: None,
    };
    ApiWithdrawStrategyRepo::upsert(core_pool, withdraw_strategy)
        .await
        .expect("seed withdraw strategy");
    let withdraw_strategy_id = ApiWithdrawStrategyRepo::get_by_uid(core_pool, withdrawal_uid)
        .await
        .expect("load withdraw strategy")
        .expect("withdraw strategy exists")
        .id;
    ApiWithdrawStrategyChainConfigRepo::upsert(
        core_pool,
        ApiWithdrawStrategyChainConfigEntity {
            id: 0,
            strategy_id: withdraw_strategy_id,
            chain_code: chain_code.to_string(),
            chain_address_type: None,
            normal_idx: Some(0),
            normal_address: to_addr.to_string(),
            risk_idx: Some(1),
            risk_address: to_addr.to_string(),
            created_at: Utc::now(),
            updated_at: None,
        },
    )
    .await
    .expect("seed withdraw strategy chain config");
}

async fn mark_collect_waiting_for_service_fee(
    collect_pool: &ApiTransactionDbPool,
    trade_no: &str,
    expect_msg: &str,
) {
    sqlx::query(
        r#"
        UPDATE api_collect
        SET need_service_fee = true,
            ever_needed_service_fee = true,
            service_fee_uploaded_at = NULL,
            service_fee_order_received_at = NULL,
            transaction_fee = '',
            status = ?,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        WHERE trade_no = ?
        "#,
    )
    .bind(ApiCollectStatus::InsufficientBalance)
    .bind(trade_no)
    .execute(collect_pool.as_ref())
    .await
    .expect(expect_msg);
}
