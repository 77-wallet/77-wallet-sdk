use crate::{
    context::Context, error::service::ServiceError,
    infrastructure::task_queue::task::dispatch_task_entities_with_ctx,
    service::api_wallet::business_notification::ApiBusinessNotificationService,
};
use sha2::{Digest, Sha256};
use wallet_database::{
    entities::{
        api_business_notification::ApiBusinessNotification,
        task_queue::{CreateTaskQueueEntity, KnownTaskName, TaskName},
    },
    repositories::task_queue::TaskQueueRepo,
};

/// Internal directed task; the snapshot prevents stale tasks from checking newer alerts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ApiAddressBalanceCheck {
    pub uid: String,
    pub notification_id: String,
    pub payload: String,
}
impl From<&ApiBusinessNotification> for ApiAddressBalanceCheck {
    fn from(row: &ApiBusinessNotification) -> Self {
        Self { uid: row.uid.clone(), notification_id: row.id.clone(), payload: row.payload.clone() }
    }
}
impl ApiAddressBalanceCheck {
    pub(crate) async fn execute(&self, ctx: &'static Context) -> Result<(), ServiceError> {
        ApiBusinessNotificationService::new(ctx).check_address_balance(self).await
    }
}

/// Persist before dispatch. No task-pool permits or chain IO are awaited here.
pub(crate) async fn enqueue(
    ctx: &'static Context,
    row: &ApiBusinessNotification,
    epoch: Option<&str>,
) -> Result<(), ServiceError> {
    if row.resolved_at.is_some()
        || crate::messaging::mqtt::message::BizType::from_business_notification_kind(&row.kind)
            .is_none()
    {
        return Ok(());
    }
    let body = serde_json::to_string(&ApiAddressBalanceCheck::from(row))
        .map_err(|e| ServiceError::Parameter(e.to_string()))?;
    let hash = format!("{:x}", Sha256::digest(body.as_bytes()));
    // New-event replay uses a stable ID; startup and reconciliation episodes require
    // fresh checks even if a previous observation of this payload completed.
    let id = format!("api-address-balance:{hash}:{}", epoch.unwrap_or("message"));
    let req = CreateTaskQueueEntity::new(
        Some(id),
        TaskName::Known(KnownTaskName::CheckApiAddressBalance),
        Some(body),
        3,
        0,
        None,
    )?;
    let entities = TaskQueueRepo::create_multi_task(&ctx.task_pool()?, &[req])
        .await?
        .into_iter()
        .filter(|row| row.status != 2)
        .collect();
    // Persistence is the recovery authority; a temporarily unavailable dispatcher
    // must not make callers retry already durable notification work as asset work.
    if let Err(error) = dispatch_task_entities_with_ctx(ctx, entities).await {
        tracing::warn!(%error, "balance check persisted; dispatch deferred to task recovery");
    }
    Ok(())
}

pub(crate) async fn seed_ready_unresolved(ctx: &'static Context) -> Result<(), ServiceError> {
    let epoch = format!("ready-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
    let mut first_error = None;
    for row in
        ApiBusinessNotificationService::new(ctx).unresolved_address_balance_notifications().await?
    {
        if let Err(error) = enqueue(ctx, &row, Some(&epoch)).await {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "api_wallet_balance_check_tests.rs"]
mod tests;
