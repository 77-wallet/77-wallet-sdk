use crate::harness;
use serial_test::serial;
use wallet_api::{config::Config, domain::node::NodeDomain};
use wallet_database::repositories::node::NodeRepo;

#[tokio::test]
#[serial]
async fn init_load_default_nodes_respects_feature_profile() {
    let env = harness::ensure_env().await;
    let core_pool = harness::open_core_pool(&env.db_dir).await;

    NodeDomain::init_load_default_nodes_with_ctx(env.manager.ctx())
        .await
        .expect("load default nodes");

    let nodes = NodeRepo::list(&core_pool, None).await.expect("load nodes");
    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|node| node.status == 1));

    let has_mainnet = nodes.iter().any(|node| node.network == "mainnet");
    let has_testnet = nodes.iter().any(|node| node.network == "testnet");

    if Config::active_feature_profile() == "prod" {
        assert!(has_mainnet);
        assert!(!has_testnet);
    } else {
        assert!(has_mainnet);
        assert!(has_testnet);
    }
}
