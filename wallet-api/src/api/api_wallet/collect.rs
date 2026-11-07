use wallet_database::entities::{api_collect::ApiCollectEntity, asset_token_key::AssetTokenKey};

use crate::{
    api::ReturnType, manager::WalletManager, service::api_wallet::collect::CollectService,
};

impl WalletManager {
    pub async fn get_api_collect_order_list(&self) -> ReturnType<Vec<ApiCollectEntity>> {
        CollectService::new(self.ctx).get_collect_order_list().await
    }

    pub async fn api_collect_order(
        &self,
        from: &str,
        to: &str,
        value: &str,
        validate: &str,
        chain_code: &str,
        token_address: Option<String>,
        symbol: &str,
        trade_no: &str,
        trade_type: u8,
        uid: &str,
    ) -> ReturnType<()> {
        let token_key = AssetTokenKey::from_raw(token_address.as_deref());
        CollectService::new(self.ctx)
            .collect_order(
                from, to, value, validate, chain_code, token_key, symbol, trade_no, trade_type, uid,
            )
            .await
    }
}
