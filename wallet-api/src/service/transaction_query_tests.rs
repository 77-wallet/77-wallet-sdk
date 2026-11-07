use super::TransactionService;
use crate::{config::Config, context::Context, dirs::Dirs};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wallet_database::{
    CoreDbPool,
    entities::{
        api_chain::NodeBindType,
        bill::{BillKind, BillStatus},
        chain::ChainCreateVo,
        node::NodeCreateVo,
    },
    repositories::{bill::BillRepo, chain::ChainRepo, node::NodeRepo},
};

#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn serve_rpc(
    mut socket: tokio::net::TcpStream,
    calls: Arc<AtomicUsize>,
    mode: Arc<AtomicBool>,
    mined: Arc<AtomicUsize>,
) {
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&buf[..end]);
            let length: usize = header
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + length {
                break;
            }
        }
    }
    let body_start = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let request: serde_json::Value = serde_json::from_slice(&buf[body_start..]).unwrap();
    calls.fetch_add(1, Ordering::SeqCst);
    let method = request["method"].as_str().unwrap();
    let hash = request["params"][0].as_str().unwrap_or_default();
    let response = match method {
        "eth_getTransactionReceipt" if mined.load(Ordering::SeqCst) == 3 => {
            std::future::pending::<()>().await;
            unreachable!()
        }

        "eth_getTransactionReceipt" if mode.load(Ordering::SeqCst) => {
            serde_json::json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"fixture upstream unavailable"}})
        }
        "eth_getTransactionReceipt" if mined.load(Ordering::SeqCst) > 0 => {
            serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"transactionHash":hash,"transactionIndex":"0x0","blockNumber":"0x1","blockHash":format!("0x{}","22".repeat(32)),"from":format!("0x{}","11".repeat(20)),"to":format!("0x{}","33".repeat(20)),"status":if mined.load(Ordering::SeqCst)==1 {"0x1"}else{"0x0"},"cumulativeGasUsed":"0x5208","gasUsed":"0x5208","effectiveGasPrice":"0x3b9aca00","logs":[],"logsBloom":format!("0x{}","00".repeat(256)),"type":"0x2"}})
        }
        "eth_getBalance" => {
            serde_json::json!({"jsonrpc":"2.0","id":1,"result":"0xde0b6b3a7640000"})
        }
        "eth_getBlockByHash" => {
            let block: alloy::rpc::types::Block = Default::default();
            let mut block = serde_json::to_value(block).unwrap();
            block["number"] = serde_json::json!("0x1");
            block["timestamp"] =
                serde_json::json!(format!("0x{:x}", chrono::Utc::now().timestamp()));
            serde_json::json!({"jsonrpc":"2.0","id":1,"result":block})
        }
        "eth_getTransactionReceipt" => {
            serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":null})
        }
        _ => panic!("unexpected test RPC method"),
    };
    let body = response.to_string();
    let wire = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = socket.write_all(wire.as_bytes()).await;
}

struct Fixture {
    _dir: tempfile::TempDir,
    ctx: &'static Context,
    calls: Arc<AtomicUsize>,
    rpc_error: Arc<AtomicBool>,
    mined: Arc<AtomicUsize>,
    rpc: tokio::task::JoinHandle<()>,
    stop: tokio::sync::broadcast::Sender<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.rpc.abort();
        let _ = self.stop.send(());
    }
}
impl Fixture {
    fn new(error: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Self>>> {
        Box::pin(async move {
            let dir = tempfile::tempdir().unwrap();
            let config = Config::new(r#"
app_code: test
crypto: { aes_key: '1234567890abcdef', aes_iv: 'abcdef1234567890' }
backend_api: { dev_url: 'http://127.0.0.1:9', test_url: 'http://127.0.0.1:9', prod_url: 'http://127.0.0.1:9' }
aggregate_api: { dev_url: 'http://127.0.0.1:9', test_url: 'http://127.0.0.1:9', prod_url: 'http://127.0.0.1:9' }
"#).unwrap();
            let ctx = Box::leak(Box::new(
                Context::new_without_handles_for_test(
                    Dirs::new(dir.path().to_str().unwrap()).unwrap(),
                    config,
                )
                .await
                .unwrap(),
            ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let calls = Arc::new(AtomicUsize::new(0));
            let rpc_error = Arc::new(AtomicBool::new(error));
            let mined = Arc::new(AtomicUsize::new(0));
            let server_mined = mined.clone();
            let (stop, _) = tokio::sync::broadcast::channel(1);
            let stopping = stop.clone();
            let observed = calls.clone();
            let mode = rpc_error.clone();
            let rpc = tokio::spawn(async move {
                loop {
                    let (socket, _) = listener.accept().await.unwrap();
                    let mut shutdown = stopping.subscribe();
                    let mined = server_mined.clone();
                    let calls = observed.clone();
                    let mode = mode.clone();
                    tokio::spawn(async move {
                        tokio::select! { _=shutdown.recv()=>{}, _=serve_rpc(socket,calls,mode,mined)=>{} }
                    });
                }
            });
            let pool = ctx.core_pool().unwrap();
            ChainRepo::add(
                &pool,
                ChainCreateVo::new("Polygon", "polygon", &[], NodeBindType::AutoLocal, "POL"),
            )
            .await
            .unwrap();
            NodeRepo::upsert(
                &pool,
                NodeCreateVo {
                    node_id: "query-fixture".into(),
                    name: "fake".into(),
                    chain_code: "polygon".into(),
                    rpc_url: url.clone(),
                    network: Config::feature_chain_network().as_str().into(),
                    status: 1,
                    is_local: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            ChainRepo::set_chain_node_with_type(
                &pool,
                "polygon",
                "query-fixture",
                NodeBindType::ManualUser,
            )
            .await
            .unwrap();
            Self { _dir: dir, ctx, calls, rpc_error, mined, rpc, stop }
        })
    }
    fn pool(&self) -> CoreDbPool {
        self.ctx.core_pool().unwrap()
    }
    async fn bill(&self, hash: &str, expired: bool) -> wallet_database::entities::bill::BillEntity {
        let new = BillRepo::build_bill(
            hash.into(),
            "0x1111111111111111111111111111111111111111".into(),
            "0x2222222222222222222222222222222222222222".into(),
            1.1,
            "polygon".into(),
            "POL".into(),
            false,
            BillKind::Transfer,
            String::new(),
        );
        BillRepo::create(new, &self.pool()).await.unwrap();
        if expired {
            sqlx::query(
                "UPDATE bill SET transaction_time = datetime('now', '-2 hours') WHERE hash = ?",
            )
            .bind(hash)
            .execute(self.pool().write_ref())
            .await
            .unwrap();
        }
        BillRepo::get_by_hash_and_owner(
            hash,
            "0x1111111111111111111111111111111111111111",
            &self.pool(),
        )
        .await
        .unwrap()
    }
    async fn status(&self, bill: &wallet_database::entities::bill::BillEntity) -> i8 {
        BillRepo::find_by_id(&bill.id.to_string(), &self.pool()).await.unwrap().status
    }
}

#[tokio::test]
async fn transaction_query_tests_rpc_error_is_visible_and_does_not_expire_bill() {
    let f = Fixture::new(true).await;
    let bill = f.bill("query-rpc-error", true).await;
    let logs = Captured::default();
    let output = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || output.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let result = TransactionService::new(f.ctx).query_tx_result(vec![bill.id.to_string()]).await;
    assert!(result.is_err(), "RPC error must not become successful empty result");
    assert_eq!(f.status(&bill).await, BillStatus::Pending.to_i8());
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let diagnostic = text
        .lines()
        .find(|l| l.contains("ordinary_transaction_query") && l.contains("stage=\"receipt\""))
        .expect("safe receipt-stage diagnostic");
    assert!(
        diagnostic.contains("chain=polygon") && diagnostic.contains("rpc_code=-32000"),
        "{diagnostic}"
    );
    assert!(
        !diagnostic.contains("http://")
            && !diagnostic.contains("upstream unavailable")
            && !diagnostic.contains("11111111")
    );
}
#[tokio::test]
async fn transaction_query_tests_missing_id_is_visible() {
    let f = Fixture::new(false).await;
    assert!(TransactionService::new(f.ctx).query_tx_result(vec!["999999".into()]).await.is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn transaction_query_tests_mixed_batch_continues_and_retry_reads_expired_state() {
    let f = Fixture::new(false).await;
    let bill = f.bill("query-expired", true).await;
    let service = TransactionService::new(f.ctx);
    assert!(service.query_tx_result(vec!["999999".into(), bill.id.to_string()]).await.is_err());
    assert_eq!(f.status(&bill).await, BillStatus::Pending.to_i8());
    let result = service.query_tx_result(vec![bill.id.to_string()]).await.unwrap();
    assert_eq!(result[0].status, BillStatus::Pending.to_i8());
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn transaction_query_tests_null_receipt_preserves_fresh_pending() {
    let f = Fixture::new(false).await;
    let bill = f.bill("query-pending", false).await;
    let result =
        TransactionService::new(f.ctx).query_tx_result(vec![bill.id.to_string()]).await.unwrap();
    assert_eq!(result[0].status, BillStatus::Pending.to_i8());
    assert_eq!(f.status(&bill).await, BillStatus::Pending.to_i8());
}
#[tokio::test]
async fn transaction_query_tests_expiry_remains_pending_and_queryable() {
    let f = Fixture::new(false).await;
    let bill = f.bill("query-expiry-response", true).await;
    let result =
        TransactionService::new(f.ctx).query_tx_result(vec![bill.id.to_string()]).await.unwrap();
    assert_eq!(result[0].status, BillStatus::Pending.to_i8());
    assert_eq!(f.status(&bill).await, BillStatus::Pending.to_i8());
}

#[tokio::test]
async fn transaction_query_tests_recovery_retries_receipt_without_false_failure() {
    let f = Fixture::new(true).await;
    let bill = f.bill("query-recovery", false).await;
    let service = TransactionService::new(f.ctx);
    assert!(service.query_tx_result(vec![bill.id.to_string()]).await.is_err());
    assert_eq!(f.status(&bill).await, BillStatus::Pending.to_i8());
    f.rpc_error.store(false, Ordering::SeqCst);
    let result = service.query_tx_result(vec![bill.id.to_string()]).await.unwrap();
    assert_eq!(result[0].status, BillStatus::Pending.to_i8());
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn transaction_query_tests_terminal_success_and_empty_batch_remain_successful() {
    let f = Fixture::new(false).await;
    let bill = f.bill("query-success", false).await;
    sqlx::query("UPDATE bill SET status = 2 WHERE id = ?")
        .bind(bill.id)
        .execute(f.pool().write_ref())
        .await
        .unwrap();
    let service = TransactionService::new(f.ctx);
    let result = service.query_tx_result(vec![bill.id.to_string()]).await.unwrap();
    assert_eq!(result[0].status, BillStatus::Success.to_i8());
    assert!(service.query_tx_result(vec![]).await.unwrap().is_empty());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn transaction_query_tests_late_receipt_resolves_expired_pending_success_or_failure() {
    for mined in [1, 2] {
        let f = Fixture::new(false).await;
        wallet_database::repositories::coin::CoinRepo::upsert_multi_coin(
            &f.pool(),
            vec![wallet_database::entities::coin::CoinData {
                symbol: "POL".into(),
                chain_code: "polygon".into(),
                decimals: 18,
                ..Default::default()
            }],
        )
        .await
        .unwrap();
        let bill = f.bill(&format!("0x{}", "aa".repeat(32)), true).await;
        let service = TransactionService::new(f.ctx);
        assert_eq!(service.query_tx_result(vec![bill.id.to_string()]).await.unwrap()[0].status, 1);
        f.mined.store(mined, Ordering::SeqCst);
        let result = service.query_tx_result(vec![bill.id.to_string()]).await.unwrap();
        assert_eq!(result[0].status, if mined == 1 { 2 } else { 3 });
        assert_eq!(f.status(&bill).await, result[0].status);
    }
}

#[tokio::test]
async fn transaction_query_tests_multisig_and_non_transfer_expiry_keep_existing_policy() {
    for (multisig, kind) in [(1_i64, 1_i64), (0, 2)] {
        let f = Fixture::new(false).await;
        let bill = f.bill("other-bill-kind", true).await;
        sqlx::query("UPDATE bill SET is_multisig=?,tx_kind=? WHERE id=?")
            .bind(multisig)
            .bind(kind)
            .bind(bill.id)
            .execute(f.pool().write_ref())
            .await
            .unwrap();
        let result = TransactionService::new(f.ctx)
            .query_tx_result(vec![bill.id.to_string()])
            .await
            .unwrap();
        assert_eq!(result[0].status, 3);
        assert_eq!(f.status(&bill).await, 3);
    }
}

async fn seed_observation_coin(f: &Fixture) {
    wallet_database::repositories::coin::CoinRepo::upsert_multi_coin(
        &f.pool(),
        vec![wallet_database::entities::coin::CoinData {
            symbol: "POL".into(),
            chain_code: "polygon".into(),
            decimals: 18,
            ..Default::default()
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn transaction_query_tests_background_keeps_retrying_and_rediscovers_on_restart() {
    let f = Fixture::new(true).await;
    seed_observation_coin(&f).await;
    let bill = f.bill(&format!("0x{:064x}", 101), true).await;
    let service = TransactionService::new(f.ctx);
    let mut cursor = 0;
    for _ in 0..5 {
        service.observe_pending_ordinary_page(&mut cursor).await.unwrap();
        assert_eq!(f.status(&bill).await, 1);
    }
    assert!(f.calls.load(Ordering::SeqCst) >= 5);
    f.rpc_error.store(false, Ordering::SeqCst);
    f.mined.store(1, Ordering::SeqCst);
    // A new observer has no in-memory history; durable Pending is sufficient.
    let mut restarted_cursor = 0;
    service.observe_pending_ordinary_page(&mut restarted_cursor).await.unwrap();
    assert_eq!(f.status(&bill).await, 2);
    let before = f.calls.load(Ordering::SeqCst);
    service.observe_pending_ordinary_page(&mut restarted_cursor).await.unwrap();
    assert_eq!(before, f.calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn transaction_query_tests_background_timeouts_do_not_exhaust_and_later_revert_is_saved() {
    let f = Fixture::new(false).await;
    seed_observation_coin(&f).await;
    let bill = f.bill(&format!("0x{:064x}", 102), true).await;
    let service = TransactionService::new(f.ctx);
    f.mined.store(3, Ordering::SeqCst);
    let mut cursor = 0;
    for _ in 0..5 {
        service
            .observe_pending_ordinary_page_with_timeout(
                &mut cursor,
                std::time::Duration::from_millis(20),
            )
            .await
            .unwrap();
        assert_eq!(f.status(&bill).await, 1);
    }
    assert!(f.calls.load(Ordering::SeqCst) >= 5);
    f.mined.store(2, Ordering::SeqCst);
    service.observe_pending_ordinary_page(&mut cursor).await.unwrap();
    assert_eq!(f.status(&bill).await, 3);
}

#[tokio::test]
async fn transaction_query_tests_background_pagination_does_not_starve_later_bills() {
    let f = Fixture::new(false).await;
    seed_observation_coin(&f).await;
    for n in 0..32 {
        let bill = f.bill(&format!("background-page-{n}"), true).await;
        // Excluded kinds must neither be queried nor expired by this observer.
        sqlx::query("UPDATE bill SET chain_code = 'btc' WHERE id = ?")
            .bind(bill.id)
            .execute(f.pool().write_ref())
            .await
            .unwrap();
    }
    let later = f.bill(&format!("0x{:064x}", 103), true).await;
    let multi = f.bill("background-multisig", true).await;
    sqlx::query("UPDATE bill SET is_multisig = 1 WHERE id = ?")
        .bind(multi.id)
        .execute(f.pool().write_ref())
        .await
        .unwrap();
    let other = f.bill("background-other-kind", true).await;
    sqlx::query("UPDATE bill SET tx_kind = 2 WHERE id = ?")
        .bind(other.id)
        .execute(f.pool().write_ref())
        .await
        .unwrap();
    f.mined.store(1, Ordering::SeqCst);
    let service = TransactionService::new(f.ctx);
    let mut cursor = 0;
    service.observe_pending_ordinary_page(&mut cursor).await.unwrap();
    assert!(cursor > 0);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    service.observe_pending_ordinary_page(&mut cursor).await.unwrap();
    assert_eq!(f.status(&later).await, 2);
    assert_eq!(f.status(&multi).await, 1);
    assert_eq!(f.status(&other).await, 1);
    assert_eq!(cursor, 0);
}

#[tokio::test]
async fn transaction_query_tests_background_loop_recovers_without_ui_and_cancels_inflight() {
    let f = Fixture::new(true).await;
    seed_observation_coin(&f).await;
    let bill = f.bill(&format!("0x{:064x}", 104), true).await;
    let (stop, rx) = tokio::sync::broadcast::channel(1);
    let service = TransactionService::new(f.ctx);
    let observer = tokio::spawn(async move {
        service.run_ordinary_receipt_observer(rx, std::time::Duration::from_millis(20)).await;
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while f.calls.load(Ordering::SeqCst) < 5 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        f.rpc_error.store(false, Ordering::SeqCst);
        f.mined.store(1, Ordering::SeqCst);
        while f.status(&bill).await != 2 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let blocked = f.bill(&format!("0x{:064x}", 105), true).await;
    f.mined.store(3, Ordering::SeqCst);
    let before = f.calls.load(Ordering::SeqCst);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f.calls.load(Ordering::SeqCst) == before {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(250), observer).await.unwrap().unwrap();
    assert_eq!(f.status(&blocked).await, 1);
    // Cancellation releases any observation ticket and keeps the durable bill.
    f.mined.store(1, Ordering::SeqCst);
    TransactionService::new(f.ctx).observe_pending_ordinary_page(&mut 0).await.unwrap();
    assert_eq!(f.status(&blocked).await, 2);
}
