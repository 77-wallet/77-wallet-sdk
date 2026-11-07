use wallet_database::{
    entities::{api_fee::ApiFeeEntity, asset_token_key::AssetTokenKey},
    repositories::api_wallet::fee::ApiFeeRepo,
};

use crate::{
    context::Context, domain::api_wallet::trans::fee::ApiFeeDomain,
    request::api_wallet::trans::ApiTransferFeeReq,
};

pub struct TransferFeeService {
    ctx: &'static Context,
}

impl TransferFeeService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_transfer_fee_order_list(
        &self,
        uid: &str,
    ) -> Result<Vec<ApiFeeEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        ApiFeeRepo::list_api_fee(&pool, uid).await.map_err(|e| e.into())
    }

    pub async fn transfer_fee_order(
        &self,
        from: &str,
        to: &str,
        value: &str,
        validate: &str,
        chain_code: &str,
        token_key: AssetTokenKey,
        symbol: &str,
        trade_no: &str,
        trade_type: u8,
        uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let req = ApiTransferFeeReq {
            from: from.to_string(),
            to: to.to_string(),
            value: value.to_string(),
            validate: validate.to_string(),
            chain_code: chain_code.to_string(),
            token_address: token_key,
            symbol: symbol.to_string(),
            trade_no: trade_no.to_string(),
            trade_type,
            uid: uid.to_string(),
        };
        let res = ApiFeeDomain::transfer_fee_with_ctx(self.ctx, &req).await;
        match res {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!("transfer_fee_order failed with {:?}", e);
                Err(e.into())
            }
        }
    }
}
