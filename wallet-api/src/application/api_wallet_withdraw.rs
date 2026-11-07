use crate::{
    context::Context, domain::api_wallet::trans::withdraw::ApiWithdrawDomain,
    error::service::ServiceError, request::api_wallet::trans::ApiWithdrawReq,
};
use wallet_database::{
    entities::{api_trade_type::ApiTradeType, asset_token_key::AssetTokenKey},
    repositories::api_wallet::withdraw::ApiWithdrawRepo,
};
use wallet_transport_backend::request::api_wallet::audit::AuditResultReportReq;

pub(crate) struct ApiWithdrawApplication {
    ctx: &'static Context,
}

impl ApiWithdrawApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn withdrawal_order(
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
        let req = ApiWithdrawReq {
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
            audit,
            out_order_id: None,
            client_id: None,
            create_time: None,
        };
        let res = ApiWithdrawDomain::withdraw_with_ctx(self.ctx, &req).await;
        match res {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::error!("withdrawal_order failed with {:?}", e);
                Err(e.into())
            }
        }
    }

    pub(crate) async fn sign_withdrawal_order(&self, trade_no: &str) -> Result<(), ServiceError> {
        if !self.should_report_audit_result(trade_no).await? {
            return Ok(());
        }

        self.report_audit_result(trade_no, true, "OK").await?;
        ApiWithdrawDomain::sign_withdrawal_order(self.ctx, trade_no).await
    }

    pub(crate) async fn reject_withdrawal_order(&self, trade_no: &str) -> Result<(), ServiceError> {
        if !self.should_report_audit_result(trade_no).await? {
            return Ok(());
        }

        self.report_audit_result(trade_no, false, "user rejected").await?;
        ApiWithdrawDomain::reject_withdrawal_order(self.ctx, trade_no).await
    }

    async fn should_report_audit_result(&self, trade_no: &str) -> Result<bool, ServiceError> {
        let pool = self.ctx.api_transaction_pool()?;
        let entity =
            ApiWithdrawRepo::get_api_withdraw_by_trade_no(&pool, trade_no, ApiTradeType::Withdraw)
                .await
                .map_err(ServiceError::from)?;
        let should_report = ApiWithdrawDomain::has_no_audit_decision(&entity);
        if !should_report {
            tracing::info!(
                trade_no = %trade_no,
                audit_passed = entity.audit_passed_at.is_some(),
                audit_rejected = entity.audit_rejected_at.is_some(),
                "skip duplicate api withdraw audit report"
            );
        }
        Ok(should_report)
    }

    async fn report_audit_result(
        &self,
        trade_no: &str,
        result: bool,
        remark: &str,
    ) -> Result<(), ServiceError> {
        let backend_api = self.ctx.get_global_backend_api();
        let req = AuditResultReportReq::new(trade_no.to_string(), result, remark);
        backend_api.report_audit_result(&req).await?;
        Ok(())
    }
}
