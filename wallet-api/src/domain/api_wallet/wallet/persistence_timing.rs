//! Temporary creation diagnostics: never format inputs, results or errors.
use crate::messaging::notify::diagnostic::DiagnosticEmitter;
use std::{future::Future, time::Instant};
pub(super) struct PersistenceTiming {
    emitter: Option<DiagnosticEmitter>,
    call_id: u64,
}
impl PersistenceTiming {
    pub(super) fn disabled() -> Self {
        Self { emitter: None, call_id: 0 }
    }
    pub(super) fn new(ctx: &crate::context::Context, call_id: u64) -> Self {
        Self { emitter: Some(DiagnosticEmitter::from_context(ctx)), call_id }
    }
    fn emit(
        &self,
        stage: &'static str,
        event: &'static str,
        outcome: Option<&'static str>,
        elapsed_ms: Option<f64>,
    ) {
        if let Some(emitter) = &self.emitter {
            emitter.emit(serde_json::json!({"component":"creation_persistence","call_id":self.call_id,"stage":stage,"event":event,"outcome":outcome,"elapsed_ms":elapsed_ms}));
        }
    }
    pub(super) fn write_timing(
        &self,
        timing: wallet_database::repositories::api_wallet::wallet::ApiWalletWriteTiming,
    ) {
        if let Some(emitter) = &self.emitter {
            emitter.emit(serde_json::json!({"component":"creation_persistence","call_id":self.call_id,"stage":"repo_upsert","event":"write_timing","writer_wait_ms":timing.writer_wait_ms,"write_action_ms":timing.write_action_ms}));
        }
    }
    pub(super) async fn run<T, E>(
        &self,
        stage: &'static str,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        if self.emitter.is_none() {
            return future.await;
        }
        let mut phase =
            Phase { timing: self, stage, start: Instant::now(), outcome: "cancelled_or_unwound" };
        self.emit(stage, "begin", None, None);
        let result = future.await;
        phase.outcome = if result.is_ok() { "ok" } else { "error" };
        result
    }
}
struct Phase<'a> {
    timing: &'a PersistenceTiming,
    stage: &'static str,
    start: Instant,
    outcome: &'static str,
}
impl Drop for Phase<'_> {
    fn drop(&mut self) {
        self.timing.emit(
            self.stage,
            "end",
            Some(self.outcome),
            Some(self.start.elapsed().as_secs_f64() * 1000.0),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn message(event: crate::messaging::notify::FrontendNotifyEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()["data"]["message"].clone()
    }
    #[tokio::test]
    async fn preserves_results_and_omits_sensitive_values() {
        let (emitter, mut rx) = DiagnosticEmitter::test_channel();
        let timing = PersistenceTiming { emitter: Some(emitter), call_id: 42 };
        assert_eq!(
            timing.run("encrypt_phrase", async { Ok::<_, String>("secret-result") }).await.unwrap(),
            "secret-result"
        );
        assert_eq!(
            timing.run("encrypt_seed", async { Err::<(), _>("secret-error") }).await.unwrap_err(),
            "secret-error"
        );
        for expected in [None, Some("ok"), None, Some("error")] {
            let value = message(rx.try_recv().unwrap());
            assert_eq!(value["call_id"], 42);
            assert_eq!(value["outcome"].as_str(), expected);
            assert!(!value.to_string().contains("secret"));
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(
            PersistenceTiming::disabled().run("ignored", async { Ok::<_, ()>(7) }).await,
            Ok(7)
        );
    }
    #[tokio::test]
    async fn interrupted_calls_keep_identity_and_report_cancellation() {
        let (emitter, mut rx) = DiagnosticEmitter::test_channel();
        let first = PersistenceTiming { emitter: Some(emitter.clone()), call_id: 1 };
        let second = PersistenceTiming { emitter: Some(emitter), call_id: 2 };
        let mut pending =
            Box::pin(first.run("repo_upsert", std::future::pending::<Result<(), ()>>()));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        second.run("repo_upsert", async { Ok::<_, ()>(()) }).await.unwrap();
        drop(pending);
        let values = (0..4).map(|_| message(rx.try_recv().unwrap())).collect::<Vec<_>>();
        assert_eq!(
            values.iter().map(|v| v["call_id"].as_u64().unwrap()).collect::<Vec<_>>(),
            vec![1, 2, 2, 1]
        );
        assert_eq!(values[3]["outcome"], "cancelled_or_unwound");
        first.write_timing(
            wallet_database::repositories::api_wallet::wallet::ApiWalletWriteTiming {
                writer_wait_ms: 12.0,
                write_action_ms: 3.0,
            },
        );
        let value = message(rx.try_recv().unwrap());
        assert_eq!(value["writer_wait_ms"], 12.0);
        assert_eq!(value["write_action_ms"], 3.0);
    }
}
