use crate::{
    application::api_wallet_coin::ApiCoinApplication, context::Context,
    response_vo::standard_wallet::coin::CoinInfo,
};
use wallet_database::pagination::Pagination;

pub struct ApiCoinService {
    ctx: &'static Context,
}

impl ApiCoinService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn get_hot_coin_list(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<CoinInfo>, crate::error::service::ServiceError> {
        ApiCoinApplication::new(self.ctx)
            .get_hot_coin_list(
                wallet_address,
                account_id,
                chain_code,
                keyword,
                is_multisig,
                page,
                page_size,
            )
            .await
    }

    pub async fn customize_coin(
        &mut self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        token_address: String,
        protocol: Option<String>,
        is_multisig: bool,
        status: u8,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiCoinApplication::new(self.ctx)
            .customize_coin(
                address,
                account_id,
                chain_code,
                token_address,
                protocol,
                is_multisig,
                status,
            )
            .await
    }
}
