use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::collect::shadow::{self, CollectorShadowActorSystem},
};

#[derive(Debug)]
pub(crate) struct ProcessCollectTxHandle {
    shadow_system: Option<CollectorShadowActorSystem>,
}

impl ProcessCollectTxHandle {
    pub(crate) async fn new_with_ctx(
        ctx: &'static crate::context::Context,
    ) -> Result<Self, ServiceError> {
        shadow::enable();
        Ok(Self { shadow_system: shadow::init(ctx).await })
    }

    pub(crate) async fn close(&self) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(crate) fn get_shadow_system(&self) -> Option<&CollectorShadowActorSystem> {
        self.shadow_system.as_ref()
    }
}
