use crate::{api::ReturnType, service::node::NodeService};

impl crate::WalletManager {
    pub async fn add_node(
        &self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        ws_url: &str,
    ) -> ReturnType<String> {
        NodeService::new(self.repo_factory.resuource_repo())
            .add_node(name, chain_code, rpc_url, ws_url)
            .await
            .into()
    }

    pub async fn get_node_list(
        &self,
        chain_code: &str,
    ) -> ReturnType<Vec<crate::response_vo::chain::NodeListRes>> {
        NodeService::new(self.repo_factory.resuource_repo())
            .get_node_list(chain_code)
            .await
            .into()
    }

    // 区块链网络速率/快慢接口
    pub async fn get_node_dynamic_data(
        &self,
        chain_code: &str,
    ) -> ReturnType<Vec<crate::response_vo::chain::NodeDynData>> {
        NodeService::new(self.repo_factory.resuource_repo())
            .get_node_dynamic_data(chain_code)
            .await
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;

    #[tokio::test]
    async fn test_add_node() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let chain_code = "tron";
        let name = "tron";
        // let address = "https://api.nileex.io/";
        let rpc_url = "https://api.nileex.io/";
        let ws_url = "https://api.nileex.io/";

        let add_node = wallet_manager
            .add_node(name, chain_code, rpc_url, ws_url)
            .await;
        tracing::info!("add_node: {add_node:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_node_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let chain_code = "tron";
        let get_node_list = wallet_manager.get_node_list(chain_code).await;
        let get_node_list = wallet_utils::serde_func::serde_to_string(&get_node_list).unwrap();
        tracing::info!("get_node_list: {get_node_list:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_node_speed() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager.get_node_dynamic_data("tron").await;

        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
