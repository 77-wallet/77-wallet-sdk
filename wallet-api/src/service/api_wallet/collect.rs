use crate::{
    context::Context, domain::api_wallet::trans::collect::ApiCollectDomain,
    request::api_wallet::trans::ApiCollectReq,
};
use wallet_database::{
    entities::{api_collect::ApiCollectEntity, asset_token_key::AssetTokenKey},
    repositories::api_wallet::collect::ApiCollectRepo,
};

pub struct CollectService {
    ctx: &'static Context,
}

impl CollectService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_collect_order_list(
        &self,
    ) -> Result<Vec<ApiCollectEntity>, crate::error::service::ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        ApiCollectRepo::list_api_collect(&pool).await.map_err(|e| e.into())
    }

    pub async fn collect_order(
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
        let req = ApiCollectReq {
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
            risk_addr: 0,
        };
        ApiCollectDomain::collect_v2_with_ctx(self.ctx, &req).await?;
        Ok(())
    }
}
