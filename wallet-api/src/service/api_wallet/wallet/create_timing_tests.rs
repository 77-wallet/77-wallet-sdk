use super::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    task::Poll,
};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture(run: impl Future<Output = ()>) -> String {
    let captured = Captured::default();
    let output = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .with_ansi(false)
        .with_writer(move || output.clone())
        .finish();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    tracing::subscriber::with_default(subscriber, || runtime.block_on(run));
    let bytes = captured.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .map(|value| value.trim_matches('"'))
}

#[test]
fn create_timing_preserves_results_and_omits_sensitive_values() {
    // A formatter that includes result/error Debug values would leak these sentinels.
    let logs = capture(async {
        let timing = CreateTiming::new(ApiWalletType::SubAccount);
        let value = timing
            .run("total", async {
                timing.run("derive", async { Ok::<_, &str>("sensitive-output") }).await
            })
            .await;
        assert_eq!(value, Ok("sensitive-output"));
        let failure = timing.run("backend", async { Err::<(), _>("sensitive-error") }).await;
        assert_eq!(failure, Err("sensitive-error"));
    });
    assert!(!logs.contains("sensitive-output"));
    assert!(!logs.contains("sensitive-error"));
    assert_eq!(logs.lines().filter(|l| field(l, "event") == Some("begin")).count(), 3);
    let ends: Vec<_> = logs.lines().filter(|l| field(l, "event") == Some("end")).collect();
    assert_eq!(ends.len(), 3);
    assert_eq!(ends.iter().filter(|l| field(l, "outcome") == Some("ok")).count(), 2);
    assert_eq!(ends.iter().filter(|l| field(l, "outcome") == Some("error")).count(), 1);
    let call_id = field(ends[0], "call_id").unwrap();
    for line in logs.lines() {
        assert_eq!(field(line, "call_id"), Some(call_id));
        assert_eq!(field(line, "wallet_type"), Some("sub_account"));
    }
    for line in ends {
        assert!(field(line, "elapsed_ms").unwrap().parse::<f64>().unwrap().is_finite());
    }
}

#[test]
fn create_timing_correlates_interleaved_calls_without_a_global_current_call() {
    let logs = capture(async {
        let first = CreateTiming::new(ApiWalletType::SubAccount);
        let second = CreateTiming::new(ApiWalletType::Withdrawal);
        let (first_result, second_result) = tokio::join!(
            first.run("total", async {
                tokio::task::yield_now().await;
                Ok::<_, ()>(())
            }),
            second.run("total", async {
                tokio::task::yield_now().await;
                Ok::<_, ()>(())
            }),
        );
        assert_eq!(first_result, Ok(()));
        assert_eq!(second_result, Ok(()));
    });
    let begins: Vec<_> = logs.lines().filter(|l| field(l, "event") == Some("begin")).collect();
    assert_eq!(begins.len(), 2);
    assert_ne!(field(begins[0], "call_id"), field(begins[1], "call_id"));
    for begin in begins {
        let end = logs
            .lines()
            .find(|l| {
                field(l, "event") == Some("end") && field(l, "call_id") == field(begin, "call_id")
            })
            .unwrap();
        assert_eq!(field(end, "wallet_type"), field(begin, "wallet_type"));
        assert_eq!(field(end, "outcome"), Some("ok"));
    }
}

#[test]
fn create_timing_marks_a_dropped_pending_stage_as_interrupted() {
    let logs = capture(async {
        let timing = CreateTiming::new(ApiWalletType::Withdrawal);
        let mut pending = Box::pin(timing.run("total", async {
            timing.run("db_write", std::future::pending::<Result<(), ()>>()).await
        }));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(pending);
    });
    let ends: Vec<_> = logs.lines().filter(|l| field(l, "event") == Some("end")).collect();
    assert_eq!(ends.len(), 2);
    for line in ends {
        assert_eq!(field(line, "outcome"), Some("cancelled_or_unwound"));
    }
    assert!(!logs.contains("outcome=ok"));
}

#[test]
fn creation_notifies_safe_stages_and_preserves_business_errors() {
    let (emitter, mut rx) = crate::messaging::notify::diagnostic::DiagnosticEmitter::test_channel();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let mut timing = CreateTiming::new(ApiWalletType::SubAccount);
        timing.diagnostic = Some(emitter);
        assert_eq!(
            timing.run("http_init_wallet", async { Err::<(), _>("secret-server-response") }).await,
            Err("secret-server-response")
        );
    });
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(serde_json::to_value(event).unwrap());
    }
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["data"]["message"]["event"], "begin");
    assert_eq!(events[1]["data"]["message"]["outcome"], "error");
    assert_eq!(events[0]["data"]["message"]["call_id"], events[1]["data"]["message"]["call_id"]);
    assert!(!serde_json::to_string(&events).unwrap().contains("secret-server-response"));
}

#[test]
fn diagnostic_notifications_correlate_interleaved_calls_and_cancelled_scopes() {
    let (emitter, mut rx) = crate::messaging::notify::diagnostic::DiagnosticEmitter::test_channel();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(async {
        let mut first = CreateTiming::new(ApiWalletType::SubAccount);
        first.diagnostic = Some(emitter.clone());
        let mut second = CreateTiming::new(ApiWalletType::Withdrawal);
        second.diagnostic = Some(emitter);
        let mut pending = Box::pin(first.run("total", std::future::pending::<Result<(), ()>>()));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(second.run("total", async { Ok::<_, ()>(()) }).await, Ok(()));
        drop(pending);
    });
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(serde_json::to_value(e).unwrap()["data"]["message"].clone());
    }
    assert_eq!(events.len(), 4);
    assert_ne!(events[0]["call_id"], events[1]["call_id"]);
    assert_eq!(events[0]["call_id"], events[3]["call_id"]);
    assert_eq!(events[1]["call_id"], events[2]["call_id"]);
    assert_eq!(events[3]["outcome"], "cancelled_or_unwound");
    assert_eq!(events[2]["outcome"], "ok");
}
