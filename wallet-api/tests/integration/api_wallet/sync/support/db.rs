use chrono::Utc;
use wallet_database::{
    entities::{
        api_account::CreateApiAccountVo,
        api_assets::{ApiAssetsEntity, ApiCreateAssetsVo},
        api_chain::{ApiChainCreateVo, NodeBindType},
        api_coin::ApiCoinData,
        api_wallet::ApiWalletType,
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
    },
    repositories::api_wallet::{
        account::ApiAccountRepo, assets::ApiAssetsRepo, chain::ApiChainRepo, coin::ApiCoinRepo,
    },
};

use crate::harness::{TestEnv, next_tag, open_api_wallet_pool, upsert_wallet};

use super::fixtures::{CHAIN_CODE, SyncAssetsFixture};

pub(crate) async fn prepare_wallet_fixture(
    env: &TestEnv,
    wallet_uid: &str,
    account_address: &str,
    token_address: AssetTokenKey,
    wallet_type: ApiWalletType,
) -> anyhow::Result<String> {
    let api_pool = open_api_wallet_pool(&env.db_dir).await;
    let now = Utc::now();

    ApiChainRepo::add(
        &api_pool,
        ApiChainCreateVo::new(
            "BNB Smart Chain",
            CHAIN_CODE,
            &["m/44'/60'/0'/0".to_string()],
            NodeBindType::AutoBackend,
            "BNB",
        ),
    )
    .await?;

    ApiCoinRepo::upsert_multi_coin(
        &api_pool,
        vec![ApiCoinData::new(
            Some("BNB Smart Chain".to_string()),
            "BNB",
            CHAIN_CODE,
            token_address.clone(),
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

    let wallet_address = upsert_wallet(&env.db_dir, "sn-sync", wallet_uid, wallet_type, None).await;

    let account = CreateApiAccountVo::new(
        1,
        account_address,
        "pubkey",
        &wallet_address,
        wallet_uid,
        "m/44'/60'/0'/0/0",
        0,
        CHAIN_CODE,
        "account",
        wallet_type,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(&api_pool, vec![account]).await?;

    let asset = ApiCreateAssetsVo::new(
        AssetsId::new(account_address, CHAIN_CODE, token_address),
        "BNB",
        18,
        None,
        0,
    )
    .with_name("BNB")
    .with_balance("0");
    ApiAssetsRepo::upsert_assets_multi(&api_pool, vec![asset]).await?;

    Ok(wallet_address)
}

pub(crate) async fn load_native_asset(
    env: &TestEnv,
    fixture: &SyncAssetsFixture,
) -> anyhow::Result<ApiAssetsEntity> {
    let api_pool = open_api_wallet_pool(&env.db_dir).await;
    let saved = ApiAssetsRepo::find_by_id(
        &api_pool,
        &AssetsId::new(&fixture.account_address, CHAIN_CODE, AssetTokenKey::Native),
    )
    .await?
    .expect("asset should exist");

    Ok(saved)
}

pub(crate) async fn append_native_assets(
    env: &TestEnv,
    wallet_address: &str,
    count: usize,
) -> anyhow::Result<String> {
    let api_pool = open_api_wallet_pool(&env.db_dir).await;
    let mut accounts = Vec::with_capacity(count);
    let mut assets = Vec::with_capacity(count);
    let mut last_address = String::new();

    for index in 1..=count {
        let address = format!("0x{}", next_tag("paged-acct"));
        accounts.push(
            CreateApiAccountVo::new(
                1,
                &address,
                "pubkey",
                wallet_address,
                "api-wallet-page",
                &format!("m/44'/60'/0'/0/{index}"),
                index as i32,
                CHAIN_CODE,
                "account",
                ApiWalletType::Withdrawal,
            )
            .with_is_init(true),
        );
        assets.push(
            ApiCreateAssetsVo::new(
                AssetsId::new(&address, CHAIN_CODE, AssetTokenKey::Native),
                "BNB",
                18,
                None,
                0,
            )
            .with_name("BNB")
            .with_balance("0"),
        );
        last_address = address;
    }

    ApiAccountRepo::upsert_account_multi(&api_pool, accounts).await?;
    ApiAssetsRepo::upsert_assets_multi(&api_pool, assets).await?;
    Ok(last_address)
}

pub(crate) async fn prepare_empty_withdrawal_wallet(env: &TestEnv) -> String {
    let wallet_uid = next_tag("api-wallet-empty");
    upsert_wallet(&env.db_dir, "sn-sync-empty", &wallet_uid, ApiWalletType::Withdrawal, None).await
}

pub(crate) async fn seed_balance_reminder(
    env: &TestEnv,
    fixture: &SyncAssetsFixture,
    threshold: &str,
) -> anyhow::Result<()> {
    use wallet_database::{
        entities::api_business_notification::NewApiBusinessNotification,
        repositories::api_business_notification::ApiBusinessNotificationRepo,
    };
    let pool = open_api_wallet_pool(&env.db_dir).await;
    let account =
        ApiAccountRepo::find_one_by_address_chain_code(&fixture.account_address, CHAIN_CODE, &pool)
            .await?
            .unwrap();
    let key = format!("{}:{}:{}:", account.uid, CHAIN_CODE, fixture.account_address);
    let payload = serde_json::json!({"remaining":"0", "thresholdValue":threshold,
        "token":"", "symbol":"BNB", "merchantName":"fixture", "waddressType":"1",
        "chainCode":CHAIN_CODE, "targetAddress":fixture.account_address})
    .to_string();
    ApiBusinessNotificationRepo::upsert(
        &pool,
        NewApiBusinessNotification {
            id: &key,
            uid: &account.uid,
            kind: "AWM_ADDRESS_INSUFFICIENT",
            business_key: &key,
            payload: &payload,
        },
    )
    .await?;
    Ok(())
}

pub(crate) async fn assert_balance_reminder_resolved(
    env: &TestEnv,
    fixture: &SyncAssetsFixture,
    resolved: bool,
) -> anyhow::Result<()> {
    use wallet_database::repositories::api_business_notification::ApiBusinessNotificationRepo;
    let pool = open_api_wallet_pool(&env.db_dir).await;
    let account =
        ApiAccountRepo::find_one_by_address_chain_code(&fixture.account_address, CHAIN_CODE, &pool)
            .await?
            .unwrap();
    let history = ApiBusinessNotificationRepo::list(&pool, &account.uid, false, 0, 10).await?;
    assert_eq!(history.total_count, 1);
    assert_eq!(history.data[0].resolved_at.is_some(), resolved);
    assert!(!history.data[0].is_read);
    Ok(())
}

pub(crate) async fn seed_same_address_on_other_chain(
    env: &TestEnv,
    fixture: &SyncAssetsFixture,
) -> anyhow::Result<()> {
    let pool = open_api_wallet_pool(&env.db_dir).await;
    let account =
        ApiAccountRepo::find_one_by_address_chain_code(&fixture.account_address, CHAIN_CODE, &pool)
            .await?
            .unwrap();
    let other = CreateApiAccountVo::new(
        1,
        &fixture.account_address,
        "pubkey",
        &fixture.wallet_address,
        &account.uid,
        "m/44'/60'/0'/0/0",
        0,
        "eth",
        "account",
        ApiWalletType::Withdrawal,
    )
    .with_is_init(true);
    ApiAccountRepo::upsert_account_multi(&pool, vec![other]).await?;
    Ok(())
}
