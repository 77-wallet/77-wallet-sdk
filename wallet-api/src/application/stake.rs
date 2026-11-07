use crate::{
    context::Context,
    domain::stake::batch::StakeBatchDomain,
    error::service::ServiceError,
    request::stake::{BatchDelegate, BatchUnDelegate},
    response_vo::standard_wallet::stake::BatchDelegateResp,
};

pub(crate) struct StakeApplication {
    ctx: &'static Context,
}

impl StakeApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn batch_delegate(
        &self,
        req: BatchDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        StakeBatchDomain::new(self.ctx).await?.batch_delegate(req, password).await
    }

    pub(crate) async fn batch_un_delegate(
        &self,
        req: BatchUnDelegate,
        password: String,
    ) -> Result<BatchDelegateResp, ServiceError> {
        StakeBatchDomain::new(self.ctx).await?.batch_un_delegate(req, password).await
    }
}
