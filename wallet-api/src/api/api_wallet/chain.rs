use std::collections::HashMap;

use wallet_database::entities::api_chain::ApiChainEntity;

use crate::{
    api::ReturnType, manager::WalletManager, response_vo::standard_wallet::chain::ChainAssets,
    service::api_wallet::chain::ApiChainService,
};

impl WalletManager {
    pub async fn get_api_chain_list(
        &self,
        wallet_address: &str,
        account_id: u32,
        chain_list: HashMap<String, String>,
    ) -> ReturnType<Vec<ChainAssets>> {
        ApiChainService::new(self.ctx)
            .get_chain_assets_list_v2(wallet_address, Some(account_id), chain_list)
            .await
    }

    pub async fn get_api_hot_chain_list(&self) -> ReturnType<Vec<ApiChainEntity>> {
        ApiChainService::new(self.ctx).get_hot_chain_list().await
    }

    pub async fn sync_api_chains(&self) -> ReturnType<Vec<String>> {
        ApiChainService::new(self.ctx).sync_chains().await
    }

    pub async fn sync_api_wallet_chain_data(&self) -> ReturnType<()> {
        ApiChainService::new(self.ctx).sync_wallet_chain_data().await
    }
}
