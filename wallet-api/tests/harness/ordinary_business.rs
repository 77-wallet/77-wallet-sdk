//! Shared ordinary-wallet fixture: real Context/DAO and scripted localhost JSON-RPC.
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use wallet_api::{Context, manager::WalletManager};
use wallet_database::{
    CoreDbPool,
    entities::{
        api_chain::NodeBindType, asset_token_key::AssetTokenKey, chain::ChainCreateVo,
        coin::CoinData, node::NodeCreateVo,
    },
    repositories::{chain::ChainRepo, coin::CoinRepo, node::NodeRepo},
};

pub const OWNER: &str = "0x0000000000000000000000000000000000000001";
pub const OTHER: &str = "0x0000000000000000000000000000000000000002";
pub const TOKEN: &str = "0x00000000000000000000000000000000000000ab";

pub struct Gate {
    reached: tokio::sync::Notify,
    released: (Mutex<bool>, std::sync::Condvar),
}
impl Gate {
    pub async fn reached(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.reached.notified())
            .await
            .expect("RPC not reached");
    }
    pub fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }
    fn wait(&self) {
        self.reached.notify_one();
        let guard = self.released.0.lock().unwrap();
        let (guard, _) = self
            .released
            .1
            .wait_timeout_while(guard, Duration::from_secs(10), |done| !*done)
            .unwrap();
        assert!(*guard, "RPC gate unreleased");
    }
}
struct Reply {
    stall: bool,
    method: String,
    result: Result<Value, String>,
    gate: Option<Arc<Gate>>,
}
#[derive(Default)]
struct State {
    replies: VecDeque<Reply>,
    calls: Vec<Value>,
    unexpected: Vec<String>,
}
pub struct Rpc {
    pub url: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Rpc {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let mut connections = vec![];
            while !stopped.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("accept {e}"),
                };
                let shared = shared.clone();
                connections.push(thread::spawn(move || {
                    socket.set_nonblocking(false).unwrap();
                    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut bytes=vec![]; let mut buf=[0;4096];
                    let (start, len)=loop { let n=socket.read(&mut buf).unwrap(); assert!(n>0); bytes.extend_from_slice(&buf[..n]); if let Some(end)=bytes.windows(4).position(|w| w==b"\r\n\r\n") { let headers=std::str::from_utf8(&bytes[..end]).unwrap(); let len=headers.lines().find_map(|line| {let (k,v)=line.split_once(':')?; k.eq_ignore_ascii_case("content-length").then(||v.trim().parse::<usize>().unwrap())}).unwrap(); break (end+4,len); } };
                    while bytes.len()<start+len { let n=socket.read(&mut buf).unwrap(); assert!(n>0); bytes.extend_from_slice(&buf[..n]); }
                    let request:Value=serde_json::from_slice(&bytes[start..start+len]).unwrap();
                    let reply={ let mut state=shared.lock().unwrap(); state.calls.push(request.clone()); let method=request["method"].as_str().unwrap();
                        if let Some(pos)=state.replies.iter().position(|r|r.method==method) {state.replies.remove(pos).unwrap()} else {state.unexpected.push(method.into()); Reply {stall:false, method:method.into(),result:Err("unconfigured RPC".into()),gate:None}} };
                    if reply.stall {thread::sleep(Duration::from_secs(35));return;}
                    if let Some(gate)=reply.gate {gate.wait();}
                    let response=match reply.result {Ok(result)=>json!({"jsonrpc":"2.0","id":request["id"],"result":result}),Err(message)=>json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":message}})}.to_string();
                    let _=write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response);
                }));
            }
            for handle in connections {
                handle.join().unwrap();
            }
        });
        Self { url, state, stop, worker: Some(worker) }
    }
    pub fn reply(&self, method: &str, value: Value) {
        self.state.lock().unwrap().replies.push_back(Reply {
            stall: false,
            method: method.into(),
            result: Ok(value),
            gate: None,
        });
    }
    pub fn timeout(&self, method: &str) {
        self.state.lock().unwrap().replies.push_back(Reply {
            stall: true,
            method: method.into(),
            result: Err("transport timeout".into()),
            gate: None,
        });
    }
    pub fn fail(&self, method: &str) {
        self.state.lock().unwrap().replies.push_back(Reply {
            stall: false,
            method: method.into(),
            result: Err("controlled RPC timeout".into()),
            gate: None,
        });
    }
    pub fn hold(&self, method: &str, value: Value) -> Arc<Gate> {
        let gate = Arc::new(Gate {
            reached: tokio::sync::Notify::new(),
            released: (Mutex::new(false), std::sync::Condvar::new()),
        });
        self.state.lock().unwrap().replies.push_back(Reply {
            stall: false,
            method: method.into(),
            result: Ok(value),
            gate: Some(gate.clone()),
        });
        gate
    }
    pub fn calls(&self) -> Vec<Value> {
        self.state.lock().unwrap().calls.clone()
    }
    pub fn then_consumed(&self) {
        let s = self.state.lock().unwrap();
        assert!(s.unexpected.is_empty(), "unconfigured RPC: {:?}", s.unexpected);
        assert!(s.replies.is_empty(), "unused RPC responses: {}", s.replies.len());
        assert!(s.calls.iter().all(|r| r["method"] != "eth_sendRawTransaction"));
    }
}
impl Drop for Rpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

pub struct Scenario {
    pub ctx: &'static Context,
    pub manager: WalletManager,
    pub pool: CoreDbPool,
    pub rpc: Rpc,
    _dir: tempfile::TempDir,
    _tasks: wallet_api::testkit::ordinary_business::TaskGuard,
}
impl Scenario {
    pub async fn given() -> anyhow::Result<Self> {
        Self::given_backend("http://127.0.0.1:9").await
    }
    pub async fn given_backend(backend_url: &str) -> anyhow::Result<Self> {
        unsafe {
            std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
        }
        let rpc = Rpc::new();
        let dir = tempfile::tempdir()?;
        let config_text =             r#"
app_code: "test"
crypto: { aes_key: "1234567890abcdef", aes_iv: "abcdef1234567890" }
backend_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
aggregate_api: { dev_url: "http://127.0.0.1:9", test_url: "http://127.0.0.1:9", prod_url: "http://127.0.0.1:9" }
oss: { access_key_id: "test", access_key_secret: "test", bucket_name: "test", endpoint: "test" }
"#.replace("http://127.0.0.1:9", backend_url);
        let config = wallet_api::config::Config::new(&config_text)?;
        let ctx = Box::leak(Box::new(
            Context::new_without_handles_for_test(
                wallet_api::dirs::Dirs::new(dir.path().to_str().unwrap())?,
                config,
            )
            .await?,
        ));
        let pool = wallet_api::testkit::chain_sync::core_pool(ctx)?;
        let (manager, tasks) = wallet_api::testkit::ordinary_business::manager(ctx);
        let s = Self { ctx, manager, pool, rpc, _dir: dir, _tasks: tasks };
        s.given_chain("eth").await?;
        s.given_wallet("wallet-a", OWNER, "eth").await?;
        Ok(s)
    }
    pub async fn given_chain(&self, code: &str) -> anyhow::Result<()> {
        ChainRepo::add(
            &self.pool,
            ChainCreateVo::new(code, code, &["evm".into()], NodeBindType::ManualUser, "ETH"),
        )
        .await?;
        NodeRepo::upsert(
            &self.pool,
            NodeCreateVo::new(code, "fake RPC", code, &self.rpc.url, None),
        )
        .await?;
        ChainRepo::set_chain_node_with_type(&self.pool, code, code, NodeBindType::ManualUser)
            .await?;
        Ok(())
    }
    pub async fn given_wallet(&self, wallet: &str, owner: &str, code: &str) -> anyhow::Result<()> {
        sqlx::query("INSERT OR IGNORE INTO wallet(name,uid,address,status,is_init,created_at) VALUES ('fixture',?,?,1,1,'2026-01-01T00:00:00Z')").bind(wallet).bind(wallet).execute(self.pool.write_ref()).await?;
        sqlx::query("INSERT INTO account(account_id,name,address,wallet_address,chain_code,status,is_init,created_at) VALUES (1,'fixture',?,?,?,1,1,'2026-01-01T00:00:00Z')").bind(owner).bind(wallet).bind(code).execute(self.pool.write_ref()).await?;
        Ok(())
    }
    pub async fn given_coin(&self, code: &str, token: &str, decimals: u8) -> anyhow::Result<()> {
        let date = "2026-01-01T00:00:00Z".parse()?;
        let symbol = if token.is_empty() { "ETH" } else { "TOKEN" };
        CoinRepo::upsert_multi_coin(
            &self.pool,
            vec![CoinData::new(
                Some(symbol.into()),
                symbol,
                code,
                AssetTokenKey::from_raw(Some(token)),
                Some("1".into()),
                Some("ERC20".into()),
                decimals,
                1,
                1,
                1,
                false,
                date,
                date,
            )],
        )
        .await?;
        Ok(())
    }
    pub async fn given_asset(
        &self,
        owner: &str,
        code: &str,
        token: &str,
        decimals: u8,
        balance: &str,
    ) -> anyhow::Result<()> {
        self.given_coin(code, token, decimals).await?;
        sqlx::query("INSERT INTO assets(name,symbol,decimals,address,chain_code,token_address,status,is_multisig,balance,created_at) VALUES ('fixture',?,?,?,?,?,1,0,?,'2026-01-01T00:00:00Z')").bind(if token.is_empty(){"ETH"}else{"TOKEN"}).bind(decimals).bind(owner).bind(code).bind(token).bind(balance).execute(self.pool.write_ref()).await?;
        Ok(())
    }
    pub async fn given_balance(
        &self,
        owner: &str,
        code: &str,
        token: &str,
        balance: &str,
    ) -> anyhow::Result<()> {
        let result = sqlx::query(
            "UPDATE assets SET balance=? WHERE address=? AND chain_code=? AND token_address=?",
        )
        .bind(balance)
        .bind(owner)
        .bind(code)
        .bind(token)
        .execute(self.pool.write_ref())
        .await?;
        assert_eq!(result.rows_affected(), 1);
        Ok(())
    }
    pub async fn when_refresh(&self) -> anyhow::Result<()> {
        self.manager.sync_assets(OWNER.into(), Some("eth".into()), vec![]).await?;
        Ok(())
    }
    pub async fn then_balance(
        &self,
        owner: &str,
        code: &str,
        token: &str,
        expected: &str,
    ) -> anyhow::Result<()> {
        let rows:Vec<String>=sqlx::query_scalar("SELECT balance FROM assets WHERE address=? AND chain_code=? AND token_address=? COLLATE NOCASE").bind(owner).bind(code).bind(token).fetch_all(self.pool.read_ref()).await?;
        assert_eq!(rows, vec![expected.to_string()]);
        Ok(())
    }
    pub async fn when_add(&self, wallet: &str, code: &str, token: &str) -> anyhow::Result<()> {
        self.manager
            .add_regular_coin(wallet_api::request::coin::AddCoinReq {
                wallet_address: wallet.into(),
                account_id: 1,
                chain_list: wallet_api::response_vo::standard_wallet::chain::ChainList(
                    std::collections::HashMap::from([(code.into(), token.into())]),
                ),
            })
            .await?;
        Ok(())
    }
    pub async fn then_count(&self, expected: i64) -> anyhow::Result<()> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets")
            .fetch_one(self.pool.read_ref())
            .await?;
        assert_eq!(n, expected);
        Ok(())
    }
}
