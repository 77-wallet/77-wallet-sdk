//! Creation diagnostics only. Never format input, output or error values.
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use wallet_database::entities::api_wallet::ApiWalletType;

// Unique within this process lifetime; the existing log formatter supplies device/time.
static NEXT_CALL_ID: AtomicU64 = AtomicU64::new(1);

pub(super) struct CreateTiming {
    call_id: u64,
    wallet_type: &'static str,
    diagnostic: Option<crate::messaging::notify::diagnostic::DiagnosticEmitter>,
}

impl CreateTiming {
    pub(super) fn new(wallet_type: ApiWalletType) -> Self {
        Self {
            call_id: NEXT_CALL_ID.fetch_add(1, Ordering::Relaxed),
            wallet_type: match wallet_type {
                ApiWalletType::SubAccount => "sub_account",
                ApiWalletType::Withdrawal => "withdrawal",
            },
            diagnostic: None,
        }
    }

    pub(super) fn with_context(mut self, ctx: &crate::context::Context) -> Self {
        self.diagnostic =
            Some(crate::messaging::notify::diagnostic::DiagnosticEmitter::from_context(ctx));
        self
    }

    pub(super) fn call_id(&self) -> u64 {
        self.call_id
    }

    fn notify(
        &self,
        stage: &'static str,
        event: &'static str,
        outcome: Option<&'static str>,
        elapsed_ms: Option<f64>,
    ) {
        if let Some(emitter) = &self.diagnostic {
            emitter.emit(serde_json::json!({"component":"creation", "call_id":self.call_id,
                "wallet_type":self.wallet_type, "stage":stage, "event":event,
                "outcome":outcome, "elapsed_ms":elapsed_ms}));
        }
    }

    pub(super) async fn run<T, E>(
        &self,
        stage: &'static str,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let phase =
            Phase { timing: self, stage, started: Instant::now(), outcome: "cancelled_or_unwound" };
        self.notify(stage, "begin", None, None);
        tracing::info!(
            target: "api_wallet_create_perf",
            metric = "api_wallet_create_stage_ms",
            call_id = self.call_id,
            wallet_type = self.wallet_type,
            stage,
            event = "begin",
            "API wallet creation stage"
        );
        let result = future.await;
        phase.finish(result.is_ok());
        result
    }
}

struct Phase<'a> {
    timing: &'a CreateTiming,
    stage: &'static str,
    started: Instant,
    outcome: &'static str,
}

impl Phase<'_> {
    fn finish(mut self, ok: bool) {
        self.outcome = if ok { "ok" } else { "error" };
    }
}

impl Drop for Phase<'_> {
    fn drop(&mut self) {
        self.timing.notify(
            self.stage,
            "end",
            Some(self.outcome),
            Some(self.started.elapsed().as_secs_f64() * 1000.0),
        );
        tracing::info!(
            target: "api_wallet_create_perf",
            metric = "api_wallet_create_stage_ms",
            call_id = self.timing.call_id,
            wallet_type = self.timing.wallet_type,
            stage = self.stage,
            event = "end",
            elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0,
            outcome = self.outcome,
            "API wallet creation stage"
        );
    }
}

#[cfg(test)]
#[path = "create_timing_tests.rs"]
mod tests;
