//! Forward only typed, sanitized database writer events into existing App DEBUG notifications.
use super::diagnostic::DiagnosticEmitter;
use wallet_database::{ApiWalletDbPool, db_pool::WriterDiagnosticEvent};

pub(crate) fn start(pool: &ApiWalletDbPool, emitter: DiagnosticEmitter) {
    let (sender, receiver) = tokio::sync::mpsc::channel(1024);
    pool.set_writer_diagnostics(sender);
    // Detached receiver owns no Context/pool; ends when the pool/guards drop their senders.
    let _task = tokio::spawn(forward(receiver, emitter));
}

async fn forward(
    mut receiver: tokio::sync::mpsc::Receiver<WriterDiagnosticEvent>,
    emitter: DiagnosticEmitter,
) {
    while let Some(item) = receiver.recv().await {
        emitter.emit(serde_json::json!({"component":"db_writer", "db_pool":"ApiWalletDbPool",
            "pool_id":item.pool_id,"lock_id":item.lock_id,"op":item.op,"event":item.event,
            "lock_time_ms":item.time_ms,"wait_ms":item.wait_ms,"held_ms":item.held_ms,
            "dropped_events":item.dropped_events}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_pool_events_reach_app_with_owner_identity_and_without_raw_operation() {
        let raw = std::sync::Arc::new(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap());
        let pool = ApiWalletDbPool::new(raw);
        let (emitter, mut rx) = DiagnosticEmitter::test_channel();
        start(&pool, emitter);
        let held = pool.lock_write_with_metric("sensitive-operation-sentinel").await;
        drop(held);
        let mut messages = Vec::new();
        for expected in ["wait", "acquired", "released"] {
            let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
                .await
                .expect("writer event must reach existing App channel")
                .unwrap();
            let value = serde_json::to_value(event).unwrap();
            assert_eq!(value["event"], "DEBUG");
            let message = value["data"]["message"].clone();
            assert_eq!(message["event"], expected);
            assert_eq!(message["component"], "db_writer");
            assert_eq!(message["op"], "other");
            assert_eq!(message["sn"], "test-device");
            assert!(message["lock_time_ms"].is_u64());
            assert!(!message.to_string().contains("sensitive"));
            messages.push(message);
        }
        assert_eq!(messages[0]["lock_id"], messages[2]["lock_id"]);
        assert_eq!(messages[0]["pool_id"], messages[2]["pool_id"]);
        assert!(messages[1]["wait_ms"].is_number());
        assert!(messages[2]["held_ms"].is_number());
    }
    #[tokio::test]
    async fn forwarder_finishes_when_pool_sender_is_gone() {
        let (emitter, rx) = DiagnosticEmitter::test_channel();
        drop(rx);
        let (tx, receiver) = tokio::sync::mpsc::channel(1);
        let task = tokio::spawn(forward(receiver, emitter));
        tx.try_send(WriterDiagnosticEvent {
            pool_id: 1,
            lock_id: 2,
            op: "upsert_api_wallet",
            event: "released",
            time_ms: 3,
            wait_ms: Some(1.0),
            held_ms: Some(2.0),
            dropped_events: 4,
        })
        .unwrap();
        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap();
    }
}
