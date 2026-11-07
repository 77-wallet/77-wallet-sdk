use crate::{
    api::ReturnType, manager::WalletManager, service::api_wallet::fee::TransferFeeService,
};
use wallet_database::entities::{api_fee::ApiFeeEntity, asset_token_key::AssetTokenKey};

impl WalletManager {
    pub async fn get_api_transfer_fee_order_list(
        &self,
        uid: &str,
    ) -> ReturnType<Vec<ApiFeeEntity>> {
        TransferFeeService::new(self.ctx).get_transfer_fee_order_list(uid).await
    }

    // 测试
    pub async fn api_transfer_fee_order(
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
        TransferFeeService::new(self.ctx)
            .transfer_fee_order(
                from, to, value, validate, chain_code, token_key, symbol, trade_no, trade_type, uid,
            )
            .await
    }
}
