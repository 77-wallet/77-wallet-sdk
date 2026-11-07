use crate::{
    api::ReturnType, manager::WalletManager, service::api_wallet::strategy::StrategyService,
};
use wallet_transport_backend::{
    request::api_wallet::strategy::ChainConfig,
    response_vo::api_wallet::strategy::{CollectionStrategyResp, WithdrawStrategyResp},
};

impl WalletManager {
    pub async fn update_collect_strategy(
        &self,
        uid: &str,
        threshold: u32,
        chain_config: Vec<ChainConfig>,
    ) -> ReturnType<()> {
        tracing::info!(
            "update_collect_strategy: uid={}, threshold={}, chain_config={:?}",
            uid,
            threshold,
            chain_config
        );
        StrategyService::new(self.ctx).update_collect_strategy(uid, threshold, chain_config).await
    }

    pub async fn get_collect_strategy(&self, uid: &str) -> ReturnType<CollectionStrategyResp> {
        StrategyService::new(self.ctx).query_collect_strategy(uid).await
    }

    pub async fn update_withdrawal_strategy(
        &self,
        uid: &str,
        threshold: u32,
        chain_config: Vec<ChainConfig>,
    ) -> ReturnType<()> {
        StrategyService::new(self.ctx)
            .update_withdrawal_strategy(uid, threshold, chain_config)
            .await
    }

    pub async fn get_withdrawal_strategy(&self, uid: &str) -> ReturnType<WithdrawStrategyResp> {
        StrategyService::new(self.ctx).query_withdrawal_strategy(uid).await
    }

    pub async fn query_api_wallet_configs(&self) -> ReturnType<serde_json::Value> {
        StrategyService::new(self.ctx).query_api_wallet_configs().await
    }
}
