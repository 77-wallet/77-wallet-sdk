//! Test-only access to the production first-unlock bootstrap and scanner hints.
use crate::{context::Context, error::service::ServiceError};

pub async fn start_after_unlock(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::infrastructure::expand_address::bootstrap::ExpandBootstrap::start_after_first_wallet_unlock(ctx).await
}

pub async fn plan_all(ctx: &'static Context) -> Result<(), ServiceError> {
    crate::infrastructure::expand_address::planner::ExpandPlanner::new(ctx, None)
        .plan_all_batches()
        .await
}

pub async fn plan_batch(ctx: &'static Context, batch: &str) -> Result<(), ServiceError> {
    crate::infrastructure::expand_address::planner::ExpandPlanner::new(ctx, None)
        .plan_batch(batch)
        .await
}

pub struct ScannerHandle(crate::infrastructure::expand_address::event::ExpandEventSender);

impl ScannerHandle {
    pub fn same_channel(&self, other: &Self) -> bool {
        self.0.same_channel(&other.0)
    }

    pub async fn hint(&self) {
        self.0
            .send(crate::infrastructure::expand_address::event::ExpandEvent::HintScan)
            .await
            .expect("live scanner");
    }
}

pub async fn scanner_handle(ctx: &'static Context) -> Option<ScannerHandle> {
    ctx.get_expand_event_tx().await.map(ScannerHandle)
}
