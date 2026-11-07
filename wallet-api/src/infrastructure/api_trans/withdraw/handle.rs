use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::withdraw::shadow::{self, WithdrawShadowActorSystem},
};

#[derive(Debug)]
pub(crate) struct ProcessWithdrawTxHandle {
    shadow_system: Option<WithdrawShadowActorSystem>,
}

impl ProcessWithdrawTxHandle {
    pub(crate) async fn new_with_ctx(
        ctx: &'static crate::context::Context,
    ) -> Result<Self, ServiceError> {
        shadow::enable();
        Ok(Self { shadow_system: shadow::init(ctx).await })
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(crate) fn get_shadow_system(&self) -> Option<&WithdrawShadowActorSystem> {
        self.shadow_system.as_ref()
    }
}
