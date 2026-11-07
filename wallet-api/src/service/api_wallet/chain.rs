use std::collections::HashMap;

use crate::{application::api_wallet_chain::ApiChainApplication, context::Context};
use wallet_database::entities::api_chain::ApiChainEntity;

use crate::response_vo::standard_wallet::chain::ChainAssets;

pub struct ApiChainService {
    ctx: &'static Context,
}

impl ApiChainService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_chain_assets_list(
        self,
        address: &str,
        account_id: Option<u32>,
        chain_list: HashMap<String, String>,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError> {
        ApiChainApplication::new(self.ctx)
            .get_chain_assets_list(address, account_id, chain_list)
            .await
    }

    pub async fn get_chain_assets_list_v2(
        self,
        address: &str,
        account_id: Option<u32>,
        chain_list: HashMap<String, String>,
    ) -> Result<Vec<ChainAssets>, crate::error::service::ServiceError> {
        ApiChainApplication::new(self.ctx)
            .get_chain_assets_list_v2(address, account_id, chain_list)
            .await
    }

    pub async fn get_hot_chain_list(
        self,
    ) -> Result<Vec<ApiChainEntity>, crate::error::service::ServiceError> {
        ApiChainApplication::new(self.ctx).get_hot_chain_list().await
    }

    pub async fn sync_chains(&self) -> Result<Vec<String>, crate::error::service::ServiceError> {
        ApiChainApplication::new(self.ctx).sync_chains().await
    }

    pub async fn sync_wallet_chain_data(&self) -> Result<(), crate::error::service::ServiceError> {
        ApiChainApplication::new(self.ctx).sync_wallet_chain_data().await
    }
}
