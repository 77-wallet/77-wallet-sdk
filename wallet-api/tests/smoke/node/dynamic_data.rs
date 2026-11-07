use anyhow::Result;
use wallet_api::testkit::env::get_manager_with_config;

#[tokio::test]
#[ignore = "smoke test probes live chain RPC nodes"]
async fn test_get_node_dynamic_data_by_id() -> Result<()> {
    wallet_utils::init_test_log();
    let (wallet_manager, _test_params) = get_manager_with_config("client4.toml").await?;

    let chain_code = "tron";
    let node_list = wallet_manager.get_node_list(chain_code).await?;

    for node in node_list {
        let res = wallet_manager.get_node_dynamic_data_by_id(chain_code, &node.node_id).await?;

        tracing::info!("node_id={}, dynamic={res:?}", node.node_id);
    }

    Ok(())
}
