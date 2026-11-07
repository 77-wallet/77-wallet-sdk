use serde_json::{Value, json};
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
use wallet_api::domain::chain::adapter::TransactionAdapter;
use wallet_types::chain::{chain::ChainCode, network::NetworkKind};

const USDG: &str = "2u1tszSeqZ3qBWF3uNGPFc8TzMk2tdiwknnRMWGWjGWH";

type Calls = Arc<Mutex<Vec<(String, String)>>>;

pub(super) struct TokenMetadataScenario {
    adapter: TransactionAdapter,
    calls: Calls,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl TokenMetadataScenario {
    pub(super) fn given_usdg_mint() -> Self {
        // Public mainnet account bytes captured on 2026-09-16. Tests never
        // contact mainnet, initialize a wallet or write application state.
        let account = json!({
            "owner": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
            "data": [include_str!("usdg_mint.base64").trim(), "base64"],
            "executable": false, "lamports": 1, "rentEpoch": 0, "space": 866
        });
        Self::given_account(account)
    }

    pub(super) fn given_missing_mint() -> Self {
        Self::given_account(Value::Null)
    }

    fn given_account(account: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                socket.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 4096];
                let (body_start, length) = loop {
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0, "incomplete HTTP headers");
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < body_start + length {
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0, "incomplete HTTP body");
                    bytes.extend_from_slice(&buf[..count]);
                }
                let request: Value =
                    serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
                let method = request["method"].as_str().unwrap();
                let address = request["params"][0].as_str().unwrap();
                recorded.lock().unwrap().push((method.into(), address.into()));
                let value = match (method, address) {
                    ("getTokenSupply", USDG) => json!({
                        "amount": "1000000", "decimals": 6,
                        "uiAmount": 1.0, "uiAmountString": "1"
                    }),
                    ("getAccountInfo", USDG) => account.clone(),
                    ("getAccountInfo", _) => Value::Null,
                    _ => panic!("unexpected RPC method: {method}"),
                };
                let response = json!({
                    "jsonrpc": "2.0", "id": request["id"],
                    "result": {"context": {"slot": 1}, "value": value}
                })
                .to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        Self {
            adapter: TransactionAdapter::new(ChainCode::Solana, &url, None, NetworkKind::Mainnet)
                .unwrap(),
            calls,
            stop,
            worker: Some(worker),
        }
    }

    pub(super) async fn when_read_token_info(
        &self,
    ) -> Result<(u8, String, String), wallet_chain_interact::Error> {
        let decimals = self.adapter.decimals(USDG).await?;
        let symbol = self.adapter.token_symbol(USDG).await?;
        let name = self.adapter.token_name(USDG).await?;
        Ok((decimals, symbol, name))
    }

    pub(super) fn then_only_mint_and_supply_were_requested(&self) {
        assert_eq!(
            *self.calls.lock().unwrap(),
            vec![
                ("getTokenSupply".into(), USDG.into()),
                ("getAccountInfo".into(), USDG.into()),
                ("getAccountInfo".into(), USDG.into()),
            ]
        );
    }

    pub(super) fn then_failed_at_mint_lookup(&self) {
        assert_eq!(
            *self.calls.lock().unwrap(),
            vec![("getTokenSupply".into(), USDG.into()), ("getAccountInfo".into(), USDG.into()),]
        );
    }
}

impl Drop for TokenMetadataScenario {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let joined = self.worker.take().unwrap().join();
        if !thread::panicking() {
            joined.unwrap();
        }
    }
}
