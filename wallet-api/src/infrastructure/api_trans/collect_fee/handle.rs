use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::collect_fee::shadow::{self, FeeShadowActorSystem},
};

#[derive(Debug)]
pub(crate) struct ProcessFeeTxHandle {
    shadow_system: Option<FeeShadowActorSystem>,
}

impl ProcessFeeTxHandle {
    pub(crate) async fn new_with_ctx(
        ctx: &'static crate::context::Context,
    ) -> Result<Self, ServiceError> {
        shadow::enable();
        Ok(Self { shadow_system: shadow::init(ctx).await })
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(crate) fn get_shadow_system(&self) -> Option<&FeeShadowActorSystem> {
        self.shadow_system.as_ref()
    }
}
