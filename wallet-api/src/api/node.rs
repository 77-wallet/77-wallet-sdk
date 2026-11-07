use crate::{api::ReturnType, manager::WalletManager, service::node::NodeService};

impl WalletManager {
    pub async fn add_node(
        &self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        ws_url: &str,
        http_url: Option<String>,
    ) -> ReturnType<String> {
        NodeService::new(self.ctx).add_node(name, chain_code, rpc_url, ws_url, http_url).await
    }

    pub async fn get_node_list(
        &self,
        chain_code: &str,
    ) -> ReturnType<Vec<crate::response_vo::standard_wallet::chain::NodeListRes>> {
        NodeService::new(self.ctx).get_node_list(chain_code).await
    }

    // 区块链网络速率/快慢接口
    pub async fn get_node_dynamic_data(
        &self,
        chain_code: &str,
    ) -> ReturnType<Vec<crate::response_vo::standard_wallet::chain::NodeDynData>> {
        NodeService::new(self.ctx).get_node_dynamic_data(chain_code).await
    }

    pub async fn get_node_dynamic_data_by_id(
        &self,
        chain_code: &str,
        node_id: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::chain::NodeDynData> {
        NodeService::new(self.ctx).get_node_dynamic_data_by_id(chain_code, node_id).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use crate::testkit::env::get_manager;
    use anyhow::Result;

    #[tokio::test]
    async fn test_add_node() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let chain_code = "tron";
        let name = "tron";
        // let address = "https://api.nileex.io/";
        let rpc_url = "https://api.nileex.io/";
        let ws_url = "https://api.nileex.io/";

        let add_node = wallet_manager.add_node(name, chain_code, rpc_url, ws_url, None).await;
        tracing::info!("add_node: {add_node:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_node_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let chain_code = "eth";
        let get_node_list = wallet_manager.get_node_list(chain_code).await?;
        let get_node_list = wallet_utils::serde_func::serde_to_string(&get_node_list).unwrap();
        tracing::info!("get_node_list: {get_node_list:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_node_speed() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = get_manager().await?;

        let res = wallet_manager.get_node_dynamic_data("tron").await?;

        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
