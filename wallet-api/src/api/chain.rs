use wallet_database::entities::chain::{ChainEntity, ChainWithNode};

use crate::{
    api::ReturnType,
    response_vo::chain::ChainAssets,
    service::{chain::ChainService, node::NodeService},
};

impl crate::WalletManager {
    pub async fn add_chain(
        &self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        ws_url: &str,
    ) -> ReturnType<()> {
        let node_id = NodeService::new(self.repo_factory.resuource_repo())
            .add_node(name, chain_code, rpc_url, ws_url)
            .await?;
        ChainService::new(self.repo_factory.resuource_repo())
            .add(name, chain_code, &node_id, &[], "")
            .await?
            .into()
    }

    pub async fn set_chain_node(&self, chain_code: &str, node_id: &str) -> ReturnType<()> {
        ChainService::new(self.repo_factory.resuource_repo())
            .set_chain_node(chain_code, node_id)
            .await?
            .into()
    }

    pub async fn get_market_chain_list(&self) -> ReturnType<Vec<String>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_market_chain_list()
            .await?
            .into()
    }

    pub async fn get_hot_chain_list(&self) -> ReturnType<Vec<ChainEntity>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_chain_list()
            .await?
            .into()
    }

    pub async fn get_setting_chain_list(&self) -> ReturnType<Vec<ChainWithNode>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_chain_list_with_node_info()
            .await?
            .into()
    }

    pub async fn get_protocol_list(&self, chain_code: &str) -> ReturnType<Option<ChainEntity>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_protocol_list(chain_code)
            .await?
            .into()
    }

    pub async fn get_chain_list(
        &self,
        wallet_address: &str,
        account_id: u32,
        symbol: &str,
    ) -> ReturnType<Vec<ChainAssets>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_chain_list_by_address_account_id_symbol(
                wallet_address,
                Some(account_id),
                symbol,
                None,
            )
            .await?
            .into()
    }

    pub async fn get_multisig_chain_list(
        &self,
        address: &str,
        symbol: &str,
    ) -> ReturnType<Vec<ChainAssets>> {
        ChainService::new(self.repo_factory.resuource_repo())
            .get_chain_list_by_address_account_id_symbol(address, None, symbol, Some(true))
            .await?
            .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::test::env::{setup_test_environment, TestData};
    use anyhow::Result;

    #[tokio::test]
    async fn test_get_chain_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        // let address = "0x0996dc2A80F35D7075C426bf0Ac6e389e0AB99Fc";
        let address = "0x35159Ffd3Ba9b8748E16A929612e5212a4394F48";
        let symbol = "TRX";
        let res = wallet_manager.get_chain_list(address, 1, symbol).await;
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_market_chain_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let res = wallet_manager.get_market_chain_list().await;
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_multisig_chain_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        // let address = "0x0996dc2A80F35D7075C426bf0Ac6e389e0AB99Fc";
        let address = "TBk86hq1e8C1gNX6RDXhk1wLamwzKnotmo";
        let symbol = "TRX";
        let res = wallet_manager
            .get_multisig_chain_list(address, symbol)
            .await;
        let res = wallet_utils::serde_func::serde_to_string(&res)?;
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_setting_chain_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;

        let get_config_chain_list = wallet_manager.get_setting_chain_list().await;
        let get_config_chain_list =
            wallet_utils::serde_func::serde_to_string(&get_config_chain_list).unwrap();

        tracing::info!("get_config_chain_list: {get_config_chain_list:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_set_chain_node() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let chain_code = "tron";
        let node_id = "test";
        let set_chain_node = wallet_manager.set_chain_node(chain_code, node_id).await;
        tracing::info!("set_chain_node: {set_chain_node:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_protocol_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let chain_code = "tron";
        let set_chain_node = wallet_manager.get_protocol_list(chain_code).await;
        tracing::info!("get_protocol_list: {set_chain_node:?}");

        let res = wallet_utils::serde_func::serde_to_string(&set_chain_node).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_hot_chain_list() -> Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let TestData { wallet_manager, .. } =
            setup_test_environment(None, None, false, None).await?;
        let res = wallet_manager.get_hot_chain_list().await;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
