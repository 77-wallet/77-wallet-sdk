//! Component regressions: isolated migrated SQLite, no wallet manager or live network.
use super::*;
use wallet_database::{
    entities::{
        config::{AppVersion, config_key::APP_VERSION},
        node::NodeCreateVo,
    },
    repositories::{chain::ChainRepo, config::ConfigRepo},
};

pub(super) const CODES: [&str; 5] = ["polygon", "avalanche", "arbitrum", "base", "scroll"];

pub(super) async fn context() -> (tempfile::TempDir, &'static Context) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context_at(dir.path()).await;
    (dir, ctx)
}

async fn context_at(path: &std::path::Path) -> &'static Context {
    let config = crate::config::Config::new(r#"
app_code: "test"
crypto: { aes_key: "1234567890abcdef", aes_iv: "abcdef1234567890" }
backend_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
aggregate_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
oss: { access_key_id: "test", access_key_secret: "test", bucket_name: "test", endpoint: "test" }
"#).unwrap();
    let ctx = Box::leak(Box::new(
        Context::new_without_handles_for_test(
            crate::dirs::Dirs::new(path.to_str().unwrap()).unwrap(),
            config,
        )
        .await
        .unwrap(),
    ));
    let version = AppVersion { app_version: "2.2.0".into() };
    ConfigRepo::upsert(
        APP_VERSION,
        &version.to_json_str().unwrap(),
        Some(1),
        &ctx.core_pool().unwrap(),
    )
    .await
    .unwrap();
    ctx
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
async fn initialize(ctx: &'static Context) {
    // A context without dispatch handles still persists the request before dispatch fails.
    // This fixture intentionally drives only configuration/DB effects, no background tasks.
    crate::domain::node::NodeDomain::init_load_default_nodes_with_ctx(ctx).await.unwrap();
    let err = ApiChainDomain::init_api_chain_info(ctx).await.unwrap_err();
    assert!(err.to_string().contains("global handles not ready"), "{err}");
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn dev_both_wallets_share_five_mainnet_defaults_without_duplicate_nodes() {
    let (_dir, ctx) = context().await;
    initialize(ctx).await;
    let core = ctx.core_pool().unwrap();
    let api = ctx.api_wallet_pool().unwrap();
    crate::domain::chain::ChainDomain::init_load_default_chain_with_ctx(ctx).await.unwrap();
    crate::domain::coin::CoinDomain::init_coins(&core).await.unwrap();
    ChainNodeEnsurer::new(core.clone(), api.clone()).ensure_all().await.unwrap();
    crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx).await.unwrap();
    let coins = ApiCoinRepo::coin_list(&api).await.unwrap();
    assert_eq!(coins.len(), 5, "API native defaults are missing");
    for (code, symbol) in CODES.into_iter().zip(["POL", "AVAX", "ETH", "ETH", "ETH"]) {
        let chain = ApiChainRepo::detail_with_node(&core, &api, code).await.unwrap().unwrap();
        assert_eq!(chain.network, "mainnet");
        assert_eq!(chain.main_symbol, symbol);
        let coin = coins.iter().find(|coin| coin.chain_code == code).unwrap();
        assert_eq!(coin.symbol, symbol);
        assert_eq!(coin.decimals, 18);
        assert!(coin.token_address.is_native());
        assert_eq!(coin.status, 1);
        let ordinary = ChainRepo::detail_with_node(&core, code).await.unwrap().unwrap();
        assert_eq!(ordinary.node_id, chain.node_id, "both wallets share defaults");
        let ordinary_coin =
            wallet_database::repositories::coin::CoinRepo::main_coin(code, &core).await.unwrap();
        assert_eq!(ordinary_coin.symbol, symbol);
        assert_eq!(ordinary_coin.status, 1);
    }
    initialize(ctx).await;
    assert_eq!(ApiCoinRepo::coin_count(&api).await.unwrap(), 5);
}

#[tokio::test]
async fn api_only_chain_derives_without_a_standard_chain_and_retries_unregistered_address() {
    let (_dir, ctx) = context().await;
    let api = ctx.api_wallet_pool().unwrap();
    ApiChainRepo::add(
        &api,
        wallet_database::entities::api_chain::ApiChainCreateVo::new(
            "Base",
            "base",
            &["ERC20".into()],
            NodeBindType::AutoLocal,
            "ETH",
        ),
    )
    .await
    .unwrap();
    NodeRepo::upsert(
        &ctx.core_pool().unwrap(),
        NodeCreateVo::new("api-main", "local fake", "base", "http://127.0.0.1:9", None)
            .with_is_local(1),
    )
    .await
    .unwrap();
    ApiChainDomain::get_node("base", ctx).await.unwrap();
    let index = wallet_utils::address::AccountIndexMap::from_account_id(0).unwrap();
    let mut req = ApiAddressInitReq::new(1);
    let mut prices = TokenQueryPriceReq(vec![]);
    ApiChainDomain::init_chains_api_assets(
        ctx,
        &[],
        &mut prices,
        &mut req,
        &["base".into()],
        &[7u8; 64],
        &index,
        "test-uid",
        "test-wallet",
        "test-account",
        false,
        ApiWalletType::Withdrawal,
        false,
    )
    .await
    .unwrap();
    assert_eq!(req.address_list.0.len(), 1, "API chain must not depend on standard chain rows");
    let address = req.address_list.0[0].address.clone();
    let mut retry = ApiAddressInitReq::new(1);
    ApiChainDomain::init_chains_api_assets(
        ctx,
        &[],
        &mut prices,
        &mut retry,
        &["base".into()],
        &[7u8; 64],
        &index,
        "test-uid",
        "test-wallet",
        "test-account",
        false,
        ApiWalletType::Withdrawal,
        false,
    )
    .await
    .unwrap();
    assert_eq!(retry.address_list.0.len(), 1);
    assert_eq!(retry.address_list.0[0].address, address);
    ApiAccountRepo::init_many(&api, &[(address, "base".into())]).await.unwrap();
    let mut repeat = ApiAddressInitReq::new(1);
    ApiChainDomain::init_chains_api_assets(
        ctx,
        &[],
        &mut prices,
        &mut repeat,
        &["base".into()],
        &[7u8; 64],
        &index,
        "test-uid",
        "test-wallet",
        "test-account",
        false,
        ApiWalletType::Withdrawal,
        false,
    )
    .await
    .unwrap();
    assert!(repeat.address_list.0.is_empty());
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn backend_disable_version_gate_and_missing_master_are_not_restored_by_dev_defaults() {
    let (_dir, ctx) = context().await;
    initialize(ctx).await;
    let response = serde_json::json!([
        {"name":"Polygon", "code":"polygon", "enable":false, "masterTokenCode":"POL", "appVersionCode":"2.2.0"},
        {"name":"Base", "code":"base", "enable":true, "masterTokenCode":"ETH", "appVersionCode":"9.0.0"},
        {"name":"Scroll", "code":"scroll", "enable":true, "appVersionCode":"2.2.0"}
    ]);
    let result = ApiChainDomain::upsert_multi_api_chain_than_toggle(
        ctx,
        serde_json::from_value(response).unwrap(),
    )
    .await;
    if let Err(err) = result {
        assert!(err.to_string().contains("global handles not ready"));
    }
    initialize(ctx).await;
    let api = ctx.api_wallet_pool().unwrap();
    let enabled = ApiChainRepo::get_chain_list(&api).await.unwrap();
    for code in ["polygon", "base", "scroll"] {
        assert!(!enabled.iter().any(|chain| chain.chain_code == code), "{code} was restored");
    }
}

#[tokio::test]
async fn node_arriving_later_and_partial_account_registration_remain_pending() {
    let (_dir, ctx) = context().await;
    let api = ctx.api_wallet_pool().unwrap();
    ApiChainRepo::add(
        &api,
        wallet_database::entities::api_chain::ApiChainCreateVo::new(
            "Base",
            "base",
            &["ERC20".into()],
            NodeBindType::AutoBackend,
            "ETH",
        ),
    )
    .await
    .unwrap();
    for account_id in [0, 1] {
        ApiAccountRepo::upsert_account_multi(
            &api,
            vec![
                wallet_database::entities::api_account::CreateApiAccountVo::new(
                    account_id,
                    &format!("old-{account_id}"),
                    "test-pub",
                    "test-wallet",
                    "test-uid",
                    "m/44'/60'/0'/0/0",
                    account_id as i32,
                    "eth",
                    "test",
                    ApiWalletType::SubAccount,
                )
                .with_is_init(true),
            ],
        )
        .await
        .unwrap();
    }
    assert!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap().is_empty());
    NodeRepo::upsert(
        &ctx.core_pool().unwrap(),
        NodeCreateVo::new("late-main", "local fake", "base", "http://127.0.0.1:9", None),
    )
    .await
    .unwrap();
    ChainNodeEnsurer::new(ctx.core_pool().unwrap(), api.clone()).ensure_all().await.unwrap();
    assert_eq!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap(), ["base"]);
    ApiAccountRepo::upsert_account_multi(
        &api,
        vec![
            wallet_database::entities::api_account::CreateApiAccountVo::new(
                0,
                "new-0",
                "test-pub",
                "test-wallet",
                "test-uid",
                "m/44'/60'/0'/0/0",
                0,
                "base",
                "test",
                ApiWalletType::SubAccount,
            )
            .with_is_init(true),
        ],
    )
    .await
    .unwrap();
    assert_eq!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap(), ["base"]);
    ApiAccountRepo::upsert_account_multi(
        &api,
        vec![
            wallet_database::entities::api_account::CreateApiAccountVo::new(
                1,
                "new-1",
                "test-pub",
                "test-wallet",
                "test-uid",
                "m/44'/60'/0'/0/1",
                1,
                "base",
                "test",
                ApiWalletType::SubAccount,
            )
            .with_is_init(false),
        ],
    )
    .await
    .unwrap();
    assert_eq!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap(), ["base"]);
    ApiAccountRepo::init_many(&api, &[("new-1".into(), "base".into())]).await.unwrap();
    assert!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap().is_empty());
    // Locked reconciliation must preserve unfinished rows and never call backend.
    ApiAccountRepo::upsert_account_multi(
        &api,
        vec![
            wallet_database::entities::api_account::CreateApiAccountVo::new(
                2,
                "old-2",
                "test-pub",
                "test-wallet",
                "test-uid",
                "m/44'/60'/0'/0/2",
                2,
                "eth",
                "test",
                ApiWalletType::SubAccount,
            )
            .with_is_init(true),
        ],
    )
    .await
    .unwrap();
    ApiChainDomain::reconcile_accounts_if_unlocked(ctx).await.unwrap();
    assert_eq!(ApiChainDomain::pending_chain_codes(ctx).await.unwrap(), ["base"]);
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn dev_defaults_preserve_manual_testnet_binding_and_disabled_backend_coin() {
    let (_dir, ctx) = context().await;
    let api = ctx.api_wallet_pool().unwrap();
    let core = ctx.core_pool().unwrap();
    ApiChainRepo::add(
        &api,
        wallet_database::entities::api_chain::ApiChainCreateVo::new(
            "Base",
            "base",
            &["ERC20".into()],
            NodeBindType::ManualUser,
            "ETH",
        ),
    )
    .await
    .unwrap();
    NodeRepo::upsert(
        &core,
        NodeCreateVo::new("manual-test", "manual", "base", "http://127.0.0.1:9", None)
            .with_network("testnet")
            .with_is_local(1),
    )
    .await
    .unwrap();
    ApiChainRepo::user_select(&api, "base", "manual-test").await.unwrap();
    let mut backend: wallet_database::entities::api_coin::ApiCoinData =
        crate::default_data::coin::DefaultCoin {
            name: "Backend AVAX".into(),
            chain_code: "avalanche".into(),
            symbol: "AVAX".into(),
            decimals: 9,
            token_address: Default::default(),
            protocol: None,
            default: true,
            popular: false,
            active: false,
        }
        .into();
    backend.price = Some("123".into());
    ApiCoinRepo::upsert_multi_coin(&api, vec![backend]).await.unwrap();
    initialize(ctx).await;
    let bound = ApiChainRepo::detail_with_node(&core, &api, "base").await.unwrap().unwrap();
    assert_eq!(bound.node_id, "manual-test");
    assert_eq!(bound.network, "testnet");
    let coins = ApiCoinRepo::coin_list(&api).await.unwrap();
    assert!(!coins.iter().any(|coin| coin.chain_code == "base"));
    let avax = coins.iter().find(|coin| coin.chain_code == "avalanche").unwrap();
    assert_eq!((avax.status, avax.decimals), (0, 9));
    assert_eq!(avax.price, "123");
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn dev_coin_defaults_require_enabled_chain_valid_mainnet_node_and_matching_native_symbol() {
    let (_dir, ctx) = context().await;
    initialize(ctx).await;
    let api = ctx.api_wallet_pool().unwrap();
    let core = ctx.core_pool().unwrap();
    sqlx::query("DELETE FROM api_coin").execute(api.write_ref()).await.unwrap();
    sqlx::query("UPDATE api_chain SET status=0 WHERE chain_code='polygon'")
        .execute(api.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE node SET status=0 WHERE chain_code='avalanche'")
        .execute(core.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE api_chain SET main_symbol='BROKEN' WHERE chain_code='base'")
        .execute(api.write_ref())
        .await
        .unwrap();
    super::super::dev_defaults::sync_native_coins(ctx).await.unwrap();
    let coins = ApiCoinRepo::coin_list(&api).await.unwrap();
    assert_eq!(coins.len(), 2);
    let eligible = super::super::dev_defaults::eligible_native_coins(ctx).await.unwrap();
    assert_eq!(eligible.len(), 2);
    assert!(eligible.iter().all(|coin| matches!(coin.chain_code.as_str(), "arbitrum" | "scroll")));
    assert!(coins.iter().all(|coin| matches!(coin.chain_code.as_str(), "arbitrum" | "scroll")));
}

#[cfg(any(feature = "prod", all(feature = "test", not(feature = "dev"))))]
#[tokio::test]
async fn non_dev_builds_do_not_inject_api_mainnet_nodes_or_native_coins() {
    let (_dir, ctx) = context().await;
    let err = ApiChainDomain::init_api_chain_info(ctx).await.unwrap_err();
    assert!(err.to_string().contains("global handles not ready"), "{err}");
    let core = ctx.core_pool().unwrap();
    let api = ctx.api_wallet_pool().unwrap();
    assert_eq!(ApiCoinRepo::coin_count(&api).await.unwrap(), 0);
    crate::domain::node::NodeDomain::init_load_default_nodes_with_ctx(ctx).await.unwrap();
    assert!(
        NodeRepo::list(&core, None)
            .await
            .unwrap()
            .iter()
            .all(|node| !CODES.contains(&node.chain_code.as_str()) || node.network != "mainnet")
    );
    if crate::config::Config::active_feature_profile() == "prod" {
        let chains = ApiChainRepo::get_chain_list_all_status(&api).await.unwrap();
        assert!(chains.iter().all(|chain| !CODES.contains(&chain.chain_code.as_str())));
    }
}

#[tokio::test]
async fn expansion_selection_ignores_stopped_accounts_and_deduplicates_both_wallet_types() {
    let (_dir, ctx) = context().await;
    let api = ctx.api_wallet_pool().unwrap();
    for kind in [ApiWalletType::SubAccount, ApiWalletType::Withdrawal] {
        for id in [0, 1] {
            let wallet = format!("wallet-{}", kind as u8);
            ApiAccountRepo::upsert_account_multi(
                &api,
                vec![wallet_database::entities::api_account::CreateApiAccountVo::new(
                    id,
                    &format!("address-{}-{id}", kind as u8),
                    "test-pub",
                    &wallet,
                    "test-uid",
                    "m/44'/60'/0'/0/0",
                    id as i32,
                    "eth",
                    "test",
                    kind,
                )],
            )
            .await
            .unwrap();
        }
    }
    sqlx::query("UPDATE api_account SET status = 0 WHERE account_id = 1")
        .execute(api.write_ref())
        .await
        .unwrap();
    let rows = ApiAccountRepo::list(&api).await.unwrap();
    let mut mappings = Vec::new();
    for row in &rows {
        for address in ["old-chain-address", "other-chain-address"] {
            mappings.push(wallet_database::entities::api_account::ApiAccountWalletMapping {
                account_id: row.account_id,
                account_name: row.name.clone(),
                address: address.into(),
                wallet_address: row.wallet_address.clone(),
                uid: row.uid.clone(),
                seed: String::new(),
                api_wallet_type: row.api_wallet_type,
            });
        }
    }
    ApiChainDomain::retain_active_account_mappings(&rows, &mut mappings);
    assert_eq!(mappings.len(), 2);
    assert!(mappings.iter().all(|row| row.account_id == 0));
    assert!(mappings.iter().any(|row| row.api_wallet_type == ApiWalletType::SubAccount));
    assert!(mappings.iter().any(|row| row.api_wallet_type == ApiWalletType::Withdrawal));
    assert_eq!(
        ApiAccountRepo::list(&api).await.unwrap().iter().filter(|row| row.status == 0).count(),
        2
    );
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn missing_master_backend_response_before_initialization_stays_disabled() {
    let (_dir, ctx) = context().await;
    let response = serde_json::json!([{"name":"Scroll", "code":"scroll", "enable":true, "appVersionCode":"2.2.0"}]);
    ApiChainDomain::upsert_multi_api_chain_than_toggle(
        ctx,
        serde_json::from_value(response).unwrap(),
    )
    .await
    .unwrap();
    initialize(ctx).await;
    let api = ctx.api_wallet_pool().unwrap();
    let rows = ApiChainRepo::get_chain_list_all_status(&api).await.unwrap();
    let scroll = rows.iter().find(|row| row.chain_code == "scroll").unwrap();
    assert_eq!(scroll.status, 0);
    assert!(scroll.node_id.is_none());
    assert!(
        !ApiCoinRepo::coin_list(&api).await.unwrap().iter().any(|coin| coin.chain_code == "scroll")
    );
    assert_eq!(
        ApiChainRepo::get_chain_list(&api)
            .await
            .unwrap()
            .iter()
            .filter(|row| CODES.contains(&row.chain_code.as_str()))
            .count(),
        4
    );
}

#[tokio::test]
async fn ordinary_coin_sync_uses_five_mainnet_defaults_in_dev() {
    let (_dir, ctx) = context().await;
    let core = ctx.core_pool().unwrap();
    crate::domain::coin::CoinDomain::init_coins(&core).await.unwrap();
    ChainRepo::add(
        &core,
        wallet_database::entities::chain::ChainCreateVo::new(
            "standard",
            "base",
            &[],
            NodeBindType::ManualUser,
            "ETH",
        ),
    )
    .await
    .unwrap();
    NodeRepo::upsert(
        &core,
        NodeCreateVo::new("manual-base", "manual", "base", "http://127.0.0.1:9", None)
            .with_network("mainnet"),
    )
    .await
    .unwrap();
    ChainRepo::set_chain_node_with_type(&core, "base", "manual-base", NodeBindType::ManualUser)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE coin SET status = 0, name = 'ordinary-preserved' WHERE chain_code = 'base'",
    )
    .execute(core.write_ref())
    .await
    .unwrap();
    crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx).await.unwrap();
    let coins = wallet_database::repositories::coin::CoinRepo::list_v2(
        &core,
        None,
        Some("base".into()),
        None,
    )
    .await
    .unwrap();
    assert!(!coins.is_empty());
    if crate::config::Config::active_feature_profile() == "dev" {
        assert!(coins.iter().any(|coin| coin.token_address.is_native()
            && coin.status == 1
            && coin.symbol == "ETH"));
    } else {
        assert!(coins.iter().all(|coin| coin.status == 0 && coin.name == "ordinary-preserved"));
    }
    sqlx::query("UPDATE node SET network = 'testnet' WHERE node_id = 'manual-base'")
        .execute(core.write_ref())
        .await
        .unwrap();
    crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx).await.unwrap();
    let coins = wallet_database::repositories::coin::CoinRepo::list_v2(
        &core,
        None,
        Some("base".into()),
        None,
    )
    .await
    .unwrap();
    assert!(
        coins
            .iter()
            .any(|coin| coin.token_address.is_native() && coin.status == 1 && coin.symbol == "ETH")
    );
}

#[tokio::test]
async fn compiled_feature_priority_and_fresh_default_injection_match_policy() {
    let expected = if cfg!(feature = "prod") {
        "prod"
    } else if cfg!(feature = "dev") {
        "dev"
    } else {
        "test"
    };
    assert_eq!(crate::config::Config::active_feature_profile(), expected);
    let (_dir, ctx) = context().await;
    crate::domain::node::NodeDomain::init_load_default_nodes_with_ctx(ctx).await.unwrap();
    let err = ApiChainDomain::init_api_chain_info(ctx).await.unwrap_err();
    assert!(err.to_string().contains("global handles not ready"), "{err}");
    let api = ctx.api_wallet_pool().unwrap();
    let core = ctx.core_pool().unwrap();
    let inject = cfg!(all(feature = "dev", not(feature = "prod")));
    assert_eq!(ApiCoinRepo::coin_count(&api).await.unwrap(), if inject { 5 } else { 0 });
    let nodes = NodeRepo::list(&core, None).await.unwrap();
    assert_eq!(
        nodes
            .iter()
            .filter(|node| CODES.contains(&node.chain_code.as_str()) && node.network == "mainnet")
            .count(),
        if inject { 5 } else { 0 }
    );
    assert!(
        nodes
            .iter()
            .filter(|node| CODES.contains(&node.chain_code.as_str()) && node.network == "mainnet")
            .all(|node| inject && node.is_local == 1)
    );
    if expected == "prod" {
        assert!(
            ApiChainRepo::get_chain_list_all_status(&api)
                .await
                .unwrap()
                .iter()
                .all(|chain| !CODES.contains(&chain.chain_code.as_str()))
        );
    }
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
async fn same_dev_upgrade_preserves_disabled_nodes_chains_coins_and_manual_bindings() {
    let (dir, ctx) = context().await;
    initialize(ctx).await;
    let core = ctx.core_pool().unwrap();
    let api = ctx.api_wallet_pool().unwrap();
    let node_id = ApiChainRepo::detail(&api, "base").await.unwrap().unwrap().node_id.unwrap();
    sqlx::query("UPDATE node SET status=0, rpc_url='http://127.0.0.1:8' WHERE node_id=?")
        .bind(&node_id)
        .execute(core.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE api_chain SET status=0 WHERE chain_code='polygon'")
        .execute(api.write_ref())
        .await
        .unwrap();
    sqlx::query("UPDATE api_coin SET status=0,decimals=9,price='123' WHERE chain_code='avalanche'")
        .execute(api.write_ref())
        .await
        .unwrap();
    NodeRepo::upsert(
        &core,
        NodeCreateVo::new("user-scroll", "manual", "scroll", "http://127.0.0.1:9", None)
            .with_network("testnet")
            .with_is_local(1),
    )
    .await
    .unwrap();
    ApiChainRepo::user_select(&api, "scroll", "user-scroll").await.unwrap();
    crate::domain::chain::ChainDomain::init_load_default_chain_with_ctx(ctx).await.unwrap();
    ChainRepo::user_select(&core, "scroll", "user-scroll").await.unwrap();
    let count = NodeRepo::list(&core, None).await.unwrap().len();
    let reopened = context_at(dir.path()).await;
    initialize(reopened).await;
    initialize(reopened).await;
    let node = NodeRepo::detail(&core, &node_id).await.unwrap().unwrap();
    assert_eq!((node.status, node.rpc_url.as_str()), (0, "http://127.0.0.1:8"));
    assert_eq!(NodeRepo::list(&core, None).await.unwrap().len(), count);
    let rows = ApiChainRepo::get_chain_list_all_status(&api).await.unwrap();
    assert_eq!(rows.iter().find(|row| row.chain_code == "polygon").unwrap().status, 0);
    let coins = ApiCoinRepo::coin_list(&api).await.unwrap();
    let avax = coins.iter().find(|coin| coin.chain_code == "avalanche").unwrap();
    assert_eq!((avax.status, avax.decimals, avax.price.as_str()), (0, 9, "123"));
    assert_eq!(
        ApiChainRepo::detail(&api, "scroll").await.unwrap().unwrap().node_id.as_deref(),
        Some("user-scroll")
    );
    assert_eq!(
        ChainRepo::detail(&core, "scroll").await.unwrap().unwrap().node_id.as_deref(),
        Some("user-scroll")
    );
}

#[cfg(all(feature = "dev", not(feature = "prod")))]

fn fixture_path() -> std::path::PathBuf {
    let p = std::path::PathBuf::from(std::env::var("API_SAME_DEV_UPGRADE_FIXTURE_DIR").unwrap());
    assert!(p.starts_with("/Users/apple/Work/rust/77_wallet_sdk/target"));
    assert!(p.file_name().unwrap().to_str().unwrap().starts_with("api-same-dev-upgrade-"));
    p
}

#[cfg(all(feature = "dev", not(feature = "prod")))]
#[tokio::test]
#[ignore = "runner-owned actual base dev SQLite fixture, offline"]
async fn actual_base_dev_database_upgrade_fills_defaults_and_preserves_saved_state() {
    let path = fixture_path();
    let facts: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(path.join("base-upgrade-seeded.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(facts["base"], "2dd18166a0154e72406a75430c980e3d983f804b");
    let ctx = context_at(&path).await;
    let core = ctx.core_pool().unwrap();
    let api = ctx.api_wallet_pool().unwrap();
    assert_eq!(
        NodeRepo::list(&core, None)
            .await
            .unwrap()
            .iter()
            .filter(|n| CODES.contains(&n.chain_code.as_str()) && n.network == "mainnet")
            .count(),
        0
    );
    initialize(ctx).await;
    crate::domain::chain::ChainDomain::init_load_default_chain_with_ctx(ctx).await.unwrap();
    ChainNodeEnsurer::new(core.clone(), api.clone()).ensure_all().await.unwrap();
    let nodes = NodeRepo::list(&core, None).await.unwrap();
    assert_eq!(nodes.len(), facts["node_count"].as_u64().unwrap() as usize + 5);
    for code in CODES {
        assert_eq!(
            nodes.iter().filter(|n| n.chain_code == code && n.network == "mainnet").count(),
            1
        );
    }
    let rows = ApiChainRepo::get_chain_list_all_status(&api).await.unwrap();
    assert_eq!(rows.iter().find(|c| c.chain_code == "polygon").unwrap().status, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT status FROM chain WHERE chain_code='polygon'")
            .fetch_one(core.read_ref())
            .await
            .unwrap(),
        0
    );
    for bound in [
        ApiChainRepo::detail_with_node(&core, &api, "base").await.unwrap().unwrap().node_id,
        ChainRepo::detail_with_node(&core, "base").await.unwrap().unwrap().node_id,
    ] {
        assert_eq!(bound, facts["manual"].as_str().unwrap());
    }
    assert_eq!(
        ApiChainRepo::detail_with_node(&core, &api, "base").await.unwrap().unwrap().network,
        "testnet"
    );
    for code in ["avalanche", "arbitrum", "scroll"] {
        assert_eq!(
            ApiChainRepo::detail_with_node(&core, &api, code).await.unwrap().unwrap().network,
            "mainnet"
        );
    }
    for code in ["arbitrum", "scroll"] {
        assert_eq!(
            ChainRepo::detail_with_node(&core, code).await.unwrap().unwrap().network,
            "testnet"
        );
    }
    let stopped =
        NodeRepo::detail(&core, facts["stopped"].as_str().unwrap()).await.unwrap().unwrap();
    assert_eq!((stopped.status, stopped.rpc_url.as_str()), (0, "http://127.0.0.1:8"));
    let coins = ApiCoinRepo::coin_list(&api).await.unwrap();
    let avax = coins.iter().find(|c| c.chain_code == "avalanche").unwrap();
    assert_eq!(
        (avax.status, avax.decimals, avax.name.as_str(), avax.price.as_str()),
        (0, 9, "preserved old AVAX", "123")
    );
    assert!(coins.iter().all(|c| !matches!(c.chain_code.as_str(), "base" | "polygon")));
    for code in ["arbitrum", "scroll"] {
        assert!(
            coins
                .iter()
                .any(|c| c.chain_code == code && c.token_address.is_native() && c.status == 1)
        );
    }
    initialize(ctx).await;
    assert_eq!(NodeRepo::list(&core, None).await.unwrap().len(), nodes.len());
    assert_eq!(ApiCoinRepo::coin_count(&api).await.unwrap(), 3);
}

#[cfg(all(not(feature = "prod"), any(feature = "dev", feature = "test")))]
#[tokio::test]
async fn ordinary_five_defaults_preserve_backend_disable_manual_rows_and_fill_missing_chain() {
    let (_dir, ctx) = context().await;
    let core = ctx.core_pool().unwrap();
    ChainRepo::add(
        &core,
        wallet_database::entities::chain::ChainCreateVo::new(
            "Existing Base",
            "base",
            &[],
            NodeBindType::ManualUser,
            "ETH",
        ),
    )
    .await
    .unwrap();
    NodeRepo::upsert(
        &core,
        NodeCreateVo::new("default-guard-manual", "manual", "base", "http://127.0.0.1:9", None)
            .with_network("testnet")
            .with_is_local(1),
    )
    .await
    .unwrap();
    ChainRepo::user_select(&core, "base", "default-guard-manual").await.unwrap();
    let response = serde_json::json!({"list": CODES.into_iter().zip(["POL","AVAX","ETH","ETH","ETH"]).map(|(code,symbol)|serde_json::json!({"name":format!("Backend {code}"),"code":code,"enable":code!="polygon","masterTokenCode":symbol,"appVersionCode":"2.2.0"})).collect::<Vec<_>>()});
    // This is the real Domain entry used by the CHAIN_LIST handler.
    crate::domain::chain::ChainDomain::init_load_backend_chains_with_ctx(
        ctx,
        serde_json::from_value(response.clone()).unwrap(),
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM chain WHERE chain_code='scroll'")
        .execute(core.write_ref())
        .await
        .unwrap();
    let before=sqlx::query_as::<_,wallet_database::entities::chain::ChainEntity>("SELECT * FROM chain WHERE chain_code IN ('polygon','avalanche','arbitrum','base') ORDER BY chain_code").fetch_all(core.read_ref()).await.unwrap();
    assert_eq!(before.iter().find(|row| row.chain_code == "polygon").unwrap().status, 0);
    ChainRepo::add(
        &core,
        wallet_database::entities::chain::ChainCreateVo::new(
            "Other chain control",
            "eth",
            &[],
            NodeBindType::AutoBackend,
            "ETH",
        )
        .with_status(0),
    )
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        crate::domain::chain::ChainDomain::init_load_default_chain_with_ctx(ctx),
        crate::domain::chain::ChainDomain::init_load_default_chain_with_ctx(ctx)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT status FROM chain WHERE chain_code='polygon'")
            .fetch_one(core.read_ref())
            .await
            .unwrap(),
        0
    );
    for saved in before {
        let now = sqlx::query_as::<_, wallet_database::entities::chain::ChainEntity>(
            "SELECT * FROM chain WHERE chain_code=?",
        )
        .bind(&saved.chain_code)
        .fetch_one(core.read_ref())
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(now).unwrap(),
            serde_json::to_value(saved).unwrap(),
            "default initialization overwrote an existing five-chain record"
        );
    }
    assert!(ChainRepo::detail(&core, "scroll").await.unwrap().is_some());
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM chain WHERE chain_code IN ('polygon','avalanche','arbitrum','base','scroll')").fetch_one(core.read_ref()).await.unwrap(),5);
    let manual = ChainRepo::detail(&core, "base").await.unwrap().unwrap();
    assert_eq!(manual.node_id.as_deref(), Some("default-guard-manual"));
    assert_eq!(manual.node_bind_type, NodeBindType::ManualUser);
    assert_eq!(
        ChainRepo::detail(&core, "eth").await.unwrap().unwrap().status,
        1,
        "other default chains retain legacy behavior"
    );
    let mut enabled = response;
    enabled["list"][0]["enable"] = serde_json::json!(true);
    crate::domain::chain::ChainDomain::init_load_backend_chains_with_ctx(
        ctx,
        serde_json::from_value(enabled).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        ChainRepo::detail(&core, "polygon").await.unwrap().unwrap().status,
        1,
        "backend updates remain authoritative"
    );
    ChainRepo::add(
        &core,
        wallet_database::entities::chain::ChainCreateVo::new(
            "Generic upsert control",
            "polygon",
            &[],
            NodeBindType::AutoBackend,
            "POL",
        )
        .with_status(0),
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT status FROM chain WHERE chain_code='polygon'")
            .fetch_one(core.read_ref())
            .await
            .unwrap(),
        0,
        "generic upsert semantics stay unchanged"
    );
}

#[tokio::test]
async fn backend_catalog_renames_existing_chains_without_replacing_bindings() {
    let (_dir, ctx) = context().await;
    let core = ctx.core_pool().unwrap();
    let api = ctx.api_wallet_pool().unwrap();
    wallet_database::repositories::wallet::WalletRepo::upsert_wallet(
        core.clone(),
        "wallet",
        "wallet-uid",
        "wallet",
    )
    .await
    .unwrap();
    wallet_database::repositories::account::AccountRepo::upsert_multi_account(
        core.clone(),
        vec![wallet_database::entities::account::CreateAccountVo::new(
            0,
            "owner",
            "public-key",
            "wallet",
            "m/44'/607'/0'",
            "ton",
            "account",
        )],
    )
    .await
    .unwrap();
    ChainRepo::add(
        &core,
        wallet_database::entities::chain::ChainCreateVo::new(
            "Ton",
            "ton",
            &[],
            NodeBindType::AutoLocal,
            "TON",
        ),
    )
    .await
    .unwrap();
    ApiChainRepo::add(
        &api,
        wallet_database::entities::api_chain::ApiChainCreateVo::new(
            "BNB Smart Chain",
            "bnb",
            &[],
            NodeBindType::AutoLocal,
            "BNB",
        ),
    )
    .await
    .unwrap();
    ChainRepo::user_select(&core, "ton", "manual-ton").await.unwrap();
    ApiChainRepo::user_select(&api, "bnb", "manual-bnb").await.unwrap();
    for _ in 0..2 {
        let ordinary = serde_json::from_value(serde_json::json!({"list":[{
            "code":"ton","name":"TON","masterTokenCode":"GRAM","enable":true,"appVersionCode":"2.2.0"
        }]})).unwrap();
        let err =
            ChainDomain::upsert_multi_chain_than_toggle_with_ctx(ctx, ordinary).await.unwrap_err();
        assert!(err.to_string().contains("global handles not ready"), "{err}");
        let api_chains = serde_json::from_value(serde_json::json!([{
            "code":"bnb","name":"BNB Chain","masterTokenCode":"BNB","enable":true,"appVersionCode":"2.2.0"
        }])).unwrap();
        let err =
            ApiChainDomain::upsert_multi_api_chain_than_toggle(ctx, api_chains).await.unwrap_err();
        assert!(err.to_string().contains("global handles not ready"), "{err}");
    }
    let ordinary: (String, String, Option<String>) =
        sqlx::query_as("SELECT name,main_symbol,node_id FROM chain WHERE chain_code='ton'")
            .fetch_one(core.read_ref())
            .await
            .unwrap();
    assert_eq!(ordinary, ("TON".into(), "GRAM".into(), Some("manual-ton".into())));
    let api_chain: (String, String, Option<String>) =
        sqlx::query_as("SELECT name,main_symbol,node_id FROM api_chain WHERE chain_code='bnb'")
            .fetch_one(api.read_ref())
            .await
            .unwrap();
    assert_eq!(api_chain, ("BNB Chain".into(), "BNB".into(), Some("manual-bnb".into())));
}
