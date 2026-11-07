use super::{ApiWalletRepo, test_support::seed_delete_wallet};
use crate::{dao::api_wallet::ApiWalletDao, repositories::test_helper::setup_api_wallet_pool};

#[tokio::test]
async fn api_wallet_repo_upsert_and_find_success() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_success").await;
    let uid = "uid_wallet_s_1";
    let address = "0xapi_wallet_s_1";
    let seed = b"seed".to_vec();

    ApiWalletRepo::upsert(
        &pool,
        uid,
        "wallet_name",
        address,
        b"phrase",
        &seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_1",
        0,
    )
    .await
    .unwrap();

    let got = ApiWalletRepo::find_by_address(&pool, address).await.unwrap().unwrap();
    assert_eq!(got.address, address);
    assert_eq!(got.uid, uid);
    assert_eq!(got.name, "wallet_name");
    assert_eq!(got.phrase, b"phrase".to_vec());
    assert_eq!(got.seed, seed);

    let by_uid = ApiWalletRepo::find_by_uid(&pool, uid).await.unwrap().unwrap();
    assert_eq!(by_uid.address, address);
    assert_eq!(by_uid.seed, seed);

    let list =
        ApiWalletRepo::list(&pool, Some(crate::entities::api_wallet::ApiWalletType::SubAccount))
            .await
            .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].uid, uid);
}

#[tokio::test]
async fn api_wallet_repo_missing_address_returns_none() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_edge").await;
    let got = ApiWalletRepo::find_by_address(&pool, "0xapi_wallet_missing").await.unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn api_wallet_physical_delete_only_removes_exact_address() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_delete_exact").await;
    seed_delete_wallet(&pool, "uid_a", "wallet_a").await;
    seed_delete_wallet(&pool, "uid_b", "wallet_b").await;

    let deleted = ApiWalletRepo::physical_delete(&pool, &["wallet_a"]).await.unwrap();

    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].address, "wallet_a");
    assert!(ApiWalletRepo::find_by_address(&pool, "wallet_a").await.unwrap().is_none());
    assert!(ApiWalletRepo::find_by_address(&pool, "wallet_b").await.unwrap().is_some());
}

#[tokio::test]
async fn api_wallet_physical_delete_treats_sql_shaped_address_as_data() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_delete_bind").await;
    seed_delete_wallet(&pool, "uid_a", "wallet_a").await;
    seed_delete_wallet(&pool, "uid_b", "wallet_b").await;

    let deleted = ApiWalletRepo::physical_delete(&pool, &["wallet_a') OR 1=1 --"]).await.unwrap();

    assert!(deleted.is_empty());
    assert_eq!(ApiWalletRepo::list(&pool, None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn api_wallet_repo_tx_rollback_keeps_name_unchanged() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_rollback").await;
    let address = "0xapi_wallet_rb_1";
    let seed = b"seed".to_vec();

    ApiWalletRepo::upsert(
        &pool,
        "uid_wallet_rb_1",
        "old_name",
        address,
        b"phrase",
        &seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_rb_1",
        0,
    )
    .await
    .unwrap();

    let mut tx = pool.write_ref().begin().await.unwrap();
    let changed = ApiWalletDao::edit_name(tx.as_mut(), address, "new_name").await.unwrap();
    assert!(changed);
    tx.rollback().await.unwrap();

    let got = ApiWalletRepo::find_by_address(&pool, address).await.unwrap().unwrap();
    assert_eq!(got.name, "old_name");
    assert_eq!(got.seed, seed);

    let by_uid = ApiWalletRepo::find_by_uid(&pool, "uid_wallet_rb_1").await.unwrap().unwrap();
    assert_eq!(by_uid.name, "old_name");
    assert_eq!(by_uid.seed, seed);
}

#[tokio::test]
async fn api_wallet_repo_upsert_roundtrip_and_storage() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_success").await;
    let uid = "uid_wallet_1";
    let address = "0xapi_wallet_1";
    let seed = vec![0x11, 0x22, 0x33, 0x44, 0xff, 0x00, 0x99];

    ApiWalletRepo::upsert(
        &pool,
        uid,
        "wallet_name",
        address,
        b"phrase",
        &seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_1",
        0,
    )
    .await
    .unwrap();

    let got = ApiWalletRepo::find_by_address(&pool, address).await.unwrap().unwrap();
    assert_eq!(got.address, address);
    assert_eq!(got.uid, uid);
    assert_eq!(got.seed, seed);

    let by_uid = ApiWalletRepo::find_by_uid(&pool, uid).await.unwrap().unwrap();
    assert_eq!(by_uid.address, address);
    assert_eq!(by_uid.seed, seed);

    let list =
        ApiWalletRepo::list(&pool, Some(crate::entities::api_wallet::ApiWalletType::SubAccount))
            .await
            .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].uid, uid);

    let latest = ApiWalletRepo::wallet_latest(&pool).await.unwrap().unwrap();
    assert_eq!(latest.uid, uid);

    let typeof_seed =
        sqlx::query_as::<_, (String,)>("SELECT typeof(seed) FROM api_wallet WHERE address = ?")
            .bind(address)
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(typeof_seed.0, "blob");

    let typeof_phrase =
        sqlx::query_as::<_, (String,)>("SELECT typeof(phrase) FROM api_wallet WHERE address = ?")
            .bind(address)
            .fetch_one(pool.read_ref())
            .await
            .unwrap();
    assert_eq!(typeof_phrase.0, "blob");
}

#[tokio::test]
async fn api_wallet_repo_update_seed_missing_uid_returns_false() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_missing_uid").await;
    let uid = "uid_wallet_missing";
    let address = "0xapi_wallet_missing";
    let initial_seed = vec![0xaa, 0xbb, 0xcc];

    ApiWalletRepo::upsert(
        &pool,
        uid,
        "wallet_missing_name",
        address,
        b"phrase",
        &initial_seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_missing",
        0,
    )
    .await
    .unwrap();

    let changed =
        ApiWalletRepo::update_seed_and_phrase(&pool, "missing_uid", b"new_phrase", b"new_seed")
            .await
            .unwrap();
    assert!(!changed);

    let got = ApiWalletRepo::find_by_address(&pool, address).await.unwrap().unwrap();
    assert_eq!(got.seed, initial_seed);
    assert_eq!(got.phrase, b"phrase".to_vec());
}

#[tokio::test]
async fn clear_binding_relation_clears_both_wallets_in_one_write() {
    let pool = setup_api_wallet_pool("wallet_db_clear_binding_relation").await;
    let subaccount_address = "0xapi_wallet_relation_sub";
    let withdrawal_address = "0xapi_wallet_relation_withdrawal";

    ApiWalletRepo::upsert(
        &pool,
        "uid_relation_sub",
        "relation_sub",
        subaccount_address,
        b"phrase_sub",
        b"seed_sub",
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        Some(withdrawal_address),
        "sn_relation",
        3,
    )
    .await
    .unwrap();
    ApiWalletRepo::upsert(
        &pool,
        "uid_relation_withdrawal",
        "relation_withdrawal",
        withdrawal_address,
        b"phrase_withdrawal",
        b"seed_withdrawal",
        crate::entities::api_wallet::ApiWalletType::Withdrawal,
        Some(subaccount_address),
        "sn_relation",
        3,
    )
    .await
    .unwrap();

    let affected =
        ApiWalletRepo::clear_binding_relation(&pool, subaccount_address, withdrawal_address)
            .await
            .unwrap();

    assert_eq!(affected, 2);
    let subaccount =
        ApiWalletRepo::find_by_address(&pool, subaccount_address).await.unwrap().unwrap();
    let withdrawal =
        ApiWalletRepo::find_by_address(&pool, withdrawal_address).await.unwrap().unwrap();
    assert_eq!(subaccount.binding_address, None);
    assert_eq!(withdrawal.binding_address, None);
}

#[tokio::test]
async fn api_wallet_repo_wallet_latest_returns_a_single_row() {
    let pool = setup_api_wallet_pool("wallet_db_api_wallet_latest").await;
    let seed = vec![0xaa, 0xbb, 0xcc];
    use std::{thread::sleep, time::Duration};

    ApiWalletRepo::upsert(
        &pool,
        "uid_wallet_latest_1",
        "wallet_latest_name_1",
        "0xapi_wallet_latest_1",
        b"phrase_1",
        &seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_latest",
        0,
    )
    .await
    .unwrap();

    sleep(Duration::from_secs(1));

    ApiWalletRepo::upsert(
        &pool,
        "uid_wallet_latest_2",
        "wallet_latest_name_2",
        "0xapi_wallet_latest_2",
        b"phrase_2",
        &seed,
        crate::entities::api_wallet::ApiWalletType::SubAccount,
        None,
        "sn_latest",
        0,
    )
    .await
    .unwrap();

    let latest = ApiWalletRepo::wallet_latest(&pool).await.unwrap().unwrap();
    assert_eq!(latest.address, "0xapi_wallet_latest_2");
}
