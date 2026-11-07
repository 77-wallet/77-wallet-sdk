//! Diagnostic scopes only: never accept authentication material or format raw errors.
use std::time::Instant;

pub(super) fn task_started(
    operation: &'static str,
    parent_task_id: Option<tokio::task::Id>,
    dispatched_at: Instant,
) {
    tracing::info!(
        metric = "wallet_unlock_task",
        operation,
        event = "task_start",
        task_id = ?tokio::task::try_id(),
        parent_task_id = ?parent_task_id,
        dispatch_ms = dispatched_at.elapsed().as_secs_f64() * 1000.0,
        "wallet unlock task"
    );
}

/// Nested intervals are not additive. End means scope exit, not authentication success.
pub(super) struct Phase {
    phase: &'static str,
    started: Instant,
}

impl Phase {
    pub(super) fn start(phase: &'static str) -> Self {
        tracing::info!(
            metric = "wallet_unlock_stage_ms",
            phase,
            event = "begin",
            task_id = ?tokio::task::try_id(),
            "wallet unlock phase"
        );
        Self { phase, started: Instant::now() }
    }
}

impl Drop for Phase {
    fn drop(&mut self) {
        let value_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            metric = "wallet_unlock_stage_ms",
            phase = self.phase,
            event = "end",
            task_id = ?tokio::task::try_id(),
            value_ms,
            "wallet unlock phase"
        );
    }
}
