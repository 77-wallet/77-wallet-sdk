//! Selected-RPC receipt confirmation; localhost only, no signer or broadcaster.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const HASH: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
const BLOCK: &str = "0x2222222222222222222222222222222222222222222222222222222222222222";
struct Rpc {
    url: String,
    calls: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Rpc {
    fn new(changed: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("fake RPC accept: {e}"),
                };
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0u8; 4096];
                let (start, len) = loop {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let len = std::str::from_utf8(&bytes[..end])
                            .unwrap()
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, len);
                    }
                };
                while bytes.len() < start + len {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let req: Value = serde_json::from_slice(&bytes[start..start + len]).unwrap();
                recorded.lock().unwrap().push(req.clone());
                let is_finalized =
                    req["method"] == "eth_getBlockByNumber" && req["params"][0] == "finalized";
                let value = match req["method"].as_str().unwrap() {
                    "eth_getTransactionReceipt" => {
                        json!({"transactionHash":HASH,"blockHash":BLOCK,"blockNumber":"0x2a","status":"0x1","gasUsed":"0x5208","effectiveGasPrice":"0x1","l1Fee":"0x7"})
                    }
                    "eth_getTransactionByHash" => {
                        json!({"hash":HASH,"from":"sender","nonce":"0x6","blockNumber":"0x2a","blockHash":BLOCK})
                    }
                    "eth_getBlockByNumber" if is_finalized => Value::Null,
                    "eth_getBlockByNumber" => {
                        json!({"number":req["params"][0],"hash":if changed {format!("0x{}","33".repeat(32))} else {BLOCK.into()},"transactions":[HASH],"timestamp":"0x68f40000"})
                    }
                    other => panic!("unexpected method, including any broadcast: {other}"),
                };
                let failed = is_finalized;
                let body = if failed {
                    json!({"error":{"code":-32602,"message":"unsupported finalized tag"}})
                } else {
                    json!({"jsonrpc":"2.0","id":req["id"],"result":value})
                }
                .to_string();
                write!(socket,"HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",if failed {"500 Internal Server Error"} else {"200 OK"},body.len(),body).unwrap();
            }
        });
        Self { url, calls, stop, worker: Some(worker) }
    }
}
impl Drop for Rpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let r = self.worker.take().unwrap().join();
        if !thread::panicking() {
            r.unwrap();
        }
    }
}

#[tokio::test]
async fn replacement_receipt_uses_normal_included_result_without_finalized_tag() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    // A canonical, successful nonce6 receipt. The selected RPC rejects finalized.
    let rpc = Rpc::new(false);
    let adapter =
        EthTx::new_for_test(ctx, ChainCode::Scroll, &rpc.url, NetworkKind::Mainnet, None).unwrap();
    let result = adapter.replacement_receipt(HASH, "sender", 6).await.unwrap().unwrap();
    assert!(result.success);
    assert_eq!(result.block_number, 42);
    assert_eq!(result.transaction_fee, "0.000000000000021007");
    let calls = rpc.calls.lock().unwrap();
    assert!(
        !calls
            .iter()
            .any(|r| r["method"] == "eth_getBlockByNumber" && r["params"][0] == "finalized")
    );
    assert!(!calls.iter().any(|r| r["method"] == "eth_chainId"));
}

#[tokio::test]
async fn replacement_receipt_rejects_changed_canonical_block_on_selected_rpc() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let rpc = Rpc::new(true);
    let adapter =
        EthTx::new_for_test(ctx, ChainCode::Scroll, &rpc.url, NetworkKind::Mainnet, None).unwrap();
    assert!(adapter.replacement_receipt(HASH, "sender", 6).await.unwrap().is_none());
}
