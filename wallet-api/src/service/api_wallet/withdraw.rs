use crate::{
    application::api_wallet_withdraw::ApiWithdrawApplication,
    context::Context,
    error::service::ServiceError,
    response_vo::api_wallet::withdraw::{ApiWithdrawOrderDetailVo, ApiWithdrawOrderVo},
};
use wallet_database::{
    entities::{
        api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus,
        asset_token_key::AssetTokenKey,
    },
    pagination::Pagination,
    repositories::api_wallet::withdraw::ApiWithdrawRepo,
};

pub struct WithdrawService {
    ctx: &'static Context,
}

impl WithdrawService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    /// Pending review orders across active API withdrawal wallets; no balance IO.
    pub async fn pending_withdrawals(&self) -> Result<Vec<ApiWithdrawOrderVo>, ServiceError> {
        use wallet_database::repositories::api_wallet::wallet::ApiWalletRepo;
        let pool = self.ctx.api_wallet_pool()?;
        let mut items = Vec::new();
        for (uid,) in ApiWalletRepo::active_withdrawal_uids(&pool).await? {
            items.extend(self.pending_withdrawals_for_uid(&uid).await?);
        }
        Ok(items)
    }

    pub(crate) async fn pending_withdrawals_for_uid(
        &self,
        uid: &str,
    ) -> Result<Vec<ApiWithdrawOrderVo>, ServiceError> {
        let mut items = Vec::new();
        let mut page = 0;
        loop {
            let batch =
                self.page_withdraw_order(uid, vec![ApiWithdrawStatus::Init], page, 100).await?;
            let count = batch.data.len();
            items.extend(
                batch.data.into_iter().filter(|item| {
                    item.audit_passed_at.is_none() && item.audit_rejected_at.is_none()
                }),
            );
            if count < 100 {
                break;
            }
            page += 1;
        }
        Ok(items)
    }

    pub async fn list_withdraw_order(
        &self,
        uid: &str,
    ) -> Result<Vec<ApiWithdrawOrderVo>, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let entities =
            ApiWithdrawRepo::list_api_withdraw(&pool, uid).await.map_err(ServiceError::from)?;
        Ok(entities.into_iter().map(ApiWithdrawOrderVo::from).collect())
    }

    pub async fn page_withdraw_order_with_init_status(
        &self,
        uid: &str,
        init_status: ApiWithdrawStatus,
        status: Vec<ApiWithdrawStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawOrderVo>, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let pagination = ApiWithdrawRepo::page_api_withdraw_with_init_status(
            &pool,
            uid,
            init_status,
            status,
            page,
            page_size,
        )
        .await
        .map_err(ServiceError::from)?;
        let data = pagination.data.into_iter().map(ApiWithdrawOrderVo::from).collect();
        Ok(Pagination {
            page: pagination.page,
            page_size: pagination.page_size,
            total_count: pagination.total_count,
            data,
        })
    }

    pub async fn page_withdraw_order(
        &self,
        uid: &str,
        status: Vec<ApiWithdrawStatus>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiWithdrawOrderVo>, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let pagination = ApiWithdrawRepo::page_api_withdraw(&pool, uid, status, page, page_size)
            .await
            .map_err(ServiceError::from)?;
        let data = pagination.data.into_iter().map(ApiWithdrawOrderVo::from).collect();
        Ok(Pagination {
            page: pagination.page,
            page_size: pagination.page_size,
            total_count: pagination.total_count,
            data,
        })
    }

    pub async fn detail_withdraw_order(
        &self,
        trade_no: &str,
    ) -> Result<ApiWithdrawOrderDetailVo, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let entity =
            ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, trade_no, ApiTradeType::Withdraw)
                .await
                .map_err(ServiceError::from)?;
        Ok(ApiWithdrawOrderDetailVo::from(entity))
    }

    pub async fn withdrawal_order(
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
        audit: u32,
    ) -> Result<(), ServiceError> {
        ApiWithdrawApplication::new(self.ctx)
            .withdrawal_order(
                from, to, value, validate, chain_code, token_key, symbol, trade_no, trade_type,
                uid, audit,
            )
            .await
    }

    pub async fn sign_withdrawal_order(&self, trade_no: &str) -> Result<(), ServiceError> {
        ApiWithdrawApplication::new(self.ctx).sign_withdrawal_order(trade_no).await
    }

    pub async fn reject_withdrawal_order(&self, trade_no: &str) -> Result<(), ServiceError> {
        ApiWithdrawApplication::new(self.ctx).reject_withdrawal_order(trade_no).await
    }
}
