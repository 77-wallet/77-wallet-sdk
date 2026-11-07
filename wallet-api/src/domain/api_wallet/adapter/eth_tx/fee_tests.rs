use super::*;
use rand::RngCore as _;
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

const EXTRA: u64 = 9_007_199_254_740_993;
const TOKEN: &str = "0x0000000000000000000000000000000000000003";
struct Rpc {
    url: String,
    calls: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    extra: Arc<Mutex<Result<U256, &'static str>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Rpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let joined = self.worker.take().unwrap().join();
        if !thread::panicking() {
            joined.unwrap();
        }
    }
}
impl Rpc {
    fn new(native: U256, token: U256, extra: Result<U256, &'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let extra = Arc::new(Mutex::new(extra));
        let extra_state = extra.clone();
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let mut socket = match listener.accept() {
                    Ok((socket, _)) => socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("fake accept: {e}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0u8; 4096];
                let (start, length) = loop {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let length = std::str::from_utf8(&bytes[..end])
                            .unwrap()
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
                while bytes.len() < start + length {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let req: Value = serde_json::from_slice(&bytes[start..start + length]).unwrap();
                let regime = req["params"][0]["data"]
                    == format!(
                        "0x{}",
                        hex::encode(&alloy::primitives::keccak256(b"isGalileo()")[..4])
                    );
                if !regime {
                    recorded.lock().unwrap().push(req.clone());
                }
                let extra = *extra_state.lock().unwrap();
                let additional =
                    !regime && req["method"] == "eth_call" && req["params"][0]["to"] != TOKEN;
                let result = match req["method"].as_str().unwrap() {
                    "eth_sendRawTransaction" => {
                        let raw = hex::decode(
                            req["params"][0].as_str().unwrap().trim_start_matches("0x"),
                        )
                        .unwrap();
                        json!(EthTx::evm_raw_tx_hash_hex(&raw))
                    }
                    "eth_chainId" => json!("0x1"),
                    "eth_getBalance" => json!(format!("{native:#x}")),
                    "eth_call" if regime => json!(format!("0x{:064x}", 1)),
                    "eth_call" if !additional => json!(format!("{token:#066x}")),
                    "eth_call" => json!(format!("{:064x}", extra.unwrap_or_default()))
                        .as_str()
                        .map(|s| json!(format!("0x{s}")))
                        .unwrap(),
                    "eth_estimateGas" => json!("0x5208"),
                    "eth_gasPrice" => json!("0x3"),
                    "eth_getBlockByNumber" => {
                        json!({"number":"0x1","baseFeePerGas":"0x1","gasLimit":"0x100000","gasUsed":"0x0","timestamp":"0x0","difficulty":"0x0","extraData":"0x","transactions":[],"uncles":[],"miner":"0x0000000000000000000000000000000000000000","parentHash":format!("0x{:064x}",0),"sha3Uncles":format!("0x{:064x}",0),"stateRoot":format!("0x{:064x}",0),"transactionsRoot":format!("0x{:064x}",0),"receiptsRoot":format!("0x{:064x}",0),"logsBloom":format!("0x{:0512x}",0),"nonce":"0x0000000000000000","hash":format!("0x{:064x}",1)})
                    }
                    method => panic!("unexpected fake RPC: {method}"),
                };
                let response = if additional && extra.is_err() {
                    json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":-32601,"message":extra.unwrap_err()}})
                } else { json!({"jsonrpc":"2.0","id":req["id"],"result":result}) }.to_string();
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        });
        Self { url, calls, stop, extra, worker: Some(worker) }
    }
}
fn request(code: ChainCode, token: bool, metadata: bool) -> (ApiTransferReq, ChainPrivateKey) {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let hex = hex::encode(bytes);
    let signer: alloy::signers::local::PrivateKeySigner = hex.parse().unwrap();
    let mut base = ApiBaseTransferReq::new(
        &signer.address().to_string(),
        "0x0000000000000000000000000000000000000002",
        "1",
        &code.to_string(),
    );
    base.with_token(token.then(|| TOKEN.to_string()), 18, "TEST");
    if metadata {
        base.metadata = Some(
            json!({"baseFee":"1","priorityFee":"1","maxFeePerGas":"3","gasLimit":"21000"})
                .to_string(),
        );
    }
    (ApiTransferReq { base, password: String::new(), nonce: 7 }, ChainPrivateKey::from(hex))
}

#[tokio::test]
async fn api_evm_fee_raw_includes_l1_native_and_token() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Base, ChainCode::Scroll] {
        for token in [false, true] {
            for metadata in [false, true] {
                let rpc = Rpc::new(
                    U256::from(10u64).pow(U256::from(23)),
                    U256::from(10u64).pow(U256::from(23)),
                    Ok(U256::from(EXTRA)),
                );
                let adapter =
                    EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
                let (req, key) = request(code, token, metadata);
                let quote = adapter.estimate_native_fee_wei(req.base.clone()).await.unwrap();
                let (hash, raw, fee) = adapter.build_transfer_raw(&req, key).await.unwrap();
                let RawTx::Evm(raw, total) = raw else { panic!("expected evm") };
                assert_eq!(hash, EthTx::evm_raw_tx_hash_hex(&raw));
                assert_eq!(quote, total);
                assert_eq!(unit::convert_to_u256(&fee, 18).unwrap(), total);
                if metadata {
                    assert_eq!(total, U256::from(EXTRA) + U256::from(63_000u64));
                }
                assert!(total > U256::from(EXTRA));
                if code == ChainCode::Scroll && !metadata {
                    assert!(
                        total < U256::from(EXTRA + 1_000_000),
                        "API Scroll must also avoid the0.1gwei cap floor"
                    );
                }
                let requests = rpc.calls.lock().unwrap();
                let additional: Vec<_> = requests
                    .iter()
                    .filter(|r| r["method"] == "eth_call" && r["params"][0]["to"] != TOKEN)
                    .collect();
                assert_eq!(additional.len(), 2);
                if token && code == ChainCode::Scroll {
                    let data = additional[0]["params"][0]["data"].as_str().unwrap();
                    assert!(usize::from_str_radix(&data[74..138], 16).unwrap() >= 183);
                }
            }
        }
    }
}

#[tokio::test]
async fn api_evm_fee_exact_balance_boundary_and_failures() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let fee = U256::from(EXTRA) + U256::from(63_000u64);
    let value = U256::from(1_000_000_000_000_000_000u64);
    for code in [ChainCode::Base, ChainCode::Scroll] {
        for token in [false, true] {
            let required = if token { fee } else { fee + value };
            for shortage in [false, true] {
                let native = required - U256::from(u64::from(shortage));
                let rpc = Rpc::new(native, value, Ok(U256::from(EXTRA)));
                let adapter =
                    EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
                let (req, key) = request(code, token, true);
                let res = adapter.build_transfer_raw(&req, key).await;
                if shortage {
                    assert!(matches!(
                        res.unwrap_err(),
                        ServiceError::Business(BusinessError::Chain(
                            ChainError::InsufficientFeeBalance
                        ))
                    ));
                } else {
                    assert!(res.is_ok(), "{code}, token={token}: {res:?}");
                }
                assert!(
                    !rpc.calls
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|r| r["method"] == "eth_sendRawTransaction")
                );
            }
        }
        for extra in [Err("method not found"), Ok(U256::ZERO)] {
            let rpc = Rpc::new(U256::MAX, U256::MAX, extra);
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, key) = request(code, false, true);
            assert!(adapter.build_transfer_raw(&req, key).await.is_err());
            assert!(
                !rpc.calls.lock().unwrap().iter().any(
                    |r| r["method"] == "eth_chainId" || r["method"] == "eth_sendRawTransaction"
                )
            );
        }
    }
}

#[tokio::test]
async fn api_evm_fee_non_l1_chains_keep_metadata_raw_without_new_balance_checks() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [
        ChainCode::Ethereum,
        ChainCode::BnbSmartChain,
        ChainCode::Polygon,
        ChainCode::Avalanche,
        ChainCode::Fantom,
        ChainCode::Arbitrum,
    ] {
        for token in [false, true] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Err("must not call"));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, key) = request(code, token, true);
            assert!(adapter.estimate_native_fee_wei(req.base.clone()).await.is_err());
            assert!(rpc.calls.lock().unwrap().is_empty());
            let (_, raw, fee) = adapter.build_transfer_raw(&req, key).await.unwrap();
            let RawTx::Evm(_, total) = raw else { panic!("expected evm") };
            assert_eq!(total, U256::from(63_000u64));
            assert_eq!(unit::convert_to_u256(&fee, 18).unwrap(), total);
            let calls = rpc.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0]["method"], "eth_chainId");
        }
    }
}

#[tokio::test]
async fn api_evm_fee_l1_quote_remains_balance_free() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Base, ChainCode::Scroll] {
        let rpc = Rpc::new(U256::ZERO, U256::ZERO, Ok(U256::from(EXTRA)));
        let adapter = EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
        let (req, _) = request(code, true, true);
        assert_eq!(
            adapter.estimate_native_fee_wei(req.base).await.unwrap(),
            U256::from(63_000u64) + U256::from(EXTRA)
        );
        assert_eq!(rpc.calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn api_evm_fee_checked_arithmetic_rejects_overflow() {
    let setting = FeeSetting {
        gas_limit: U256::MAX,
        base_fee: U256::ZERO,
        max_priority_fee_per_gas: U256::ZERO,
        max_fee_per_gas: U256::from(2u64),
    };
    assert!(EthTx::checked_total_fee(&setting, U256::ZERO).is_err());
    let setting = FeeSetting { gas_limit: U256::MAX, max_fee_per_gas: U256::from(1u64), ..setting };
    assert!(EthTx::checked_total_fee(&setting, U256::from(1u64)).is_err());
}

#[tokio::test]
async fn api_evm_fee_failed_observation_recovers_and_persisted_raw_is_not_repriced() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Base, ChainCode::Scroll] {
        let rpc = Rpc::new(U256::MAX, U256::MAX, Err("temporarily unavailable"));
        let adapter = EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
        let (req, key) = request(code, true, true);
        assert!(adapter.build_transfer_raw(&req, key.clone()).await.is_err());
        *rpc.extra.lock().unwrap() = Ok(U256::from(EXTRA));
        let (hash, raw, fee) = adapter.build_transfer_raw(&req, key).await.unwrap();
        let persisted = serde_json::to_string(&raw).unwrap();
        rpc.calls.lock().unwrap().clear();
        *rpc.extra.lock().unwrap() = Err("fee observation must not run during rebroadcast");
        for _ in 0..2 {
            let restored: RawTx = serde_json::from_str(&persisted).unwrap();
            let sent = adapter.broadcast_transfer(restored).await.unwrap();
            assert_eq!(sent.tx_hash, hash);
            assert_eq!(sent.fee, fee);
        }
        let calls = rpc.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|r| r["method"] == "eth_sendRawTransaction"));
        assert_eq!(calls[0]["params"], calls[1]["params"]);
    }
}

#[tokio::test]
async fn api_evm_fee_non_l1_no_metadata_keeps_legacy_balance_gate_before_gas() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [
        ChainCode::Ethereum,
        ChainCode::BnbSmartChain,
        ChainCode::Polygon,
        ChainCode::Avalanche,
        ChainCode::Fantom,
        ChainCode::Arbitrum,
    ] {
        for token in [false, true] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Err("must not call"));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, key) = request(code, token, false);
            assert!(matches!(
                adapter.build_transfer_raw(&req, key).await.unwrap_err(),
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(_)))
            ));
            let methods: Vec<String> = rpc
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["method"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(
                methods,
                if token { vec!["eth_call", "eth_call"] } else { vec!["eth_getBalance"] }
            );
        }
    }
}

#[tokio::test]
async fn api_evm_fee_non_l1_direct_transfer_keeps_metadata_estimate_and_testnet_path() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Ethereum, ChainCode::Polygon, ChainCode::Avalanche, ChainCode::Arbitrum]
    {
        for network in [NetworkKind::Mainnet, NetworkKind::Testnet] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Err("must not call"));
            let adapter = EthTx::new_for_test(ctx, code, &rpc.url, network, None).unwrap();
            let (req, key) = request(code, true, true);
            let response = adapter.transfer(&req, key).await.unwrap();
            assert_eq!(unit::convert_to_u256(&response.fee, 18).unwrap(), U256::from(42_000u64));
            let methods: Vec<String> = rpc
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["method"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(
                methods,
                match network {
                    NetworkKind::Mainnet => vec!["eth_chainId", "eth_sendRawTransaction"],
                    _ => vec![
                        "eth_estimateGas",
                        "eth_gasPrice",
                        "eth_chainId",
                        "eth_sendRawTransaction"
                    ],
                }
            );
        }
    }
}

#[tokio::test]
async fn api_evm_fee_l1_direct_transfer_without_metadata_keeps_original_amount_gate() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Base, ChainCode::Scroll] {
        for token in [false, true] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Err("must not call"));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, key) = request(code, token, false);
            let result = adapter.transfer(&req, key).await;
            assert!(
                matches!(
                    result,
                    Err(ServiceError::Business(BusinessError::Chain(
                        ChainError::InsufficientBalance(_)
                    )))
                ),
                "original amount gate missing: {code}, token={token}"
            );
            let methods: Vec<String> = rpc
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["method"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(
                methods,
                if token { vec!["eth_call", "eth_call"] } else { vec!["eth_getBalance"] }
            );
        }
    }
}

#[tokio::test]
async fn api_evm_fee_l1_direct_transfer_metadata_preserves_original_bypass() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for code in [ChainCode::Base, ChainCode::Scroll] {
        for token in [false, true] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Err("must not call"));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, key) = request(code, token, true);
            let response = adapter.transfer(&req, key).await.unwrap();
            assert_eq!(unit::convert_to_u256(&response.fee, 18).unwrap(), U256::from(42_000u64));
            let methods: Vec<String> = rpc
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["method"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(methods, vec!["eth_chainId", "eth_sendRawTransaction"]);
        }
    }
}

#[tokio::test]
async fn api_evm_fee_l1_quote_without_metadata_is_isolated_from_raw_gates() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let value = U256::from(1_000_000_000_000_000_000u64);
    for code in [ChainCode::Base, ChainCode::Scroll] {
        for token in [false, true] {
            let rpc = Rpc::new(U256::ZERO, U256::ZERO, Ok(U256::from(EXTRA)));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (mut req, key) = request(code, token, false);
            for metadata in [None, Some("invalid metadata".to_string())] {
                req.base.metadata = metadata;
                let quote = adapter.estimate_native_fee_wei(req.base.clone()).await.unwrap();
                assert!(quote > U256::from(EXTRA));
            }
            assert!(!rpc.calls.lock().unwrap().iter().any(|r| r["method"] == "eth_getBalance"
                || r["method"] == "eth_sendRawTransaction"
                || (r["method"] == "eth_call" && r["params"][0]["to"] == TOKEN)));
            rpc.calls.lock().unwrap().clear();
            req.base.metadata = None;
            assert!(matches!(
                adapter.build_transfer_raw(&req, key.clone()).await.unwrap_err(),
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientBalance(_)))
            ));
            let methods: Vec<String> = rpc
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["method"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(
                methods,
                if token { vec!["eth_call", "eth_call"] } else { vec!["eth_getBalance"] }
            );
            // Amount is funded, but the native execution+L1 reserve is not.
            let rpc =
                Rpc::new(if token { U256::ZERO } else { value }, value, Ok(U256::from(EXTRA)));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            assert!(matches!(
                adapter.build_transfer_raw(&req, key).await.unwrap_err(),
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance))
            ));
            assert!(
                !rpc.calls.lock().unwrap().iter().any(
                    |r| r["method"] == "eth_chainId" || r["method"] == "eth_sendRawTransaction"
                )
            );
        }
    }
}

#[tokio::test]
async fn replacement_raw_rechecks_native_maximum_budget_on_non_l1_chains() {
    use alloy::{
        consensus::{SignableTransaction, TxEip1559, TxEnvelope},
        network::eip2718::Encodable2718,
        signers::{SignerSync, local::PrivateKeySigner},
    };
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    for shortage in [true, false] {
        let signer = PrivateKeySigner::random();
        let (mut req, _) = request(ChainCode::Ethereum, false, false);
        req.base.from = format!("{:#x}", signer.address());
        let old = TxEip1559 {
            chain_id: 1,
            nonce: req.nonce,
            gas_limit: 21000,
            max_fee_per_gas: 100,
            max_priority_fee_per_gas: 10,
            ..Default::default()
        };
        let signature = signer.sign_hash_sync(&old.signature_hash()).unwrap();
        let old: TxEnvelope = old.into_signed(signature).into();
        let bytes = old.encoded_2718();
        req.base.metadata = Some(json!({"collectReplacement":{
            "old_raw":serde_json::to_string(&RawTx::Evm(bytes.clone(), U256::ZERO)).unwrap(),
            "old_hash":EthTx::evm_raw_tx_hash_hex(&bytes), "from_addr":req.base.from,"nonce":req.nonce
        }}).to_string());
        // Latest funds cover value, but not the replacement's bumped maximum fee.
        let native = if shortage { U256::from(1_000_000_000_000_000_000u64) } else { U256::MAX };
        let rpc = Rpc::new(native, U256::ZERO, Ok(U256::ZERO));
        let adapter =
            EthTx::new_for_test(ctx, ChainCode::Ethereum, &rpc.url, NetworkKind::Mainnet, None)
                .unwrap();
        let result = adapter
            .build_transfer_raw(&req, ChainPrivateKey::from(hex::encode(signer.to_bytes())))
            .await;
        if shortage {
            assert!(matches!(
                result.unwrap_err(),
                ServiceError::Business(BusinessError::Chain(ChainError::InsufficientFeeBalance))
            ));
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
        let calls = rpc.calls.lock().unwrap();
        assert!(calls.iter().any(|r| r["method"] == "eth_getBalance"));
        assert!(!calls.iter().any(|r| r["method"] == "eth_sendRawTransaction"));
    }
}

#[tokio::test]
async fn api_base_manual_quote_reserves_l1_margin_without_changing_estimate_or_settings() {
    use wallet_database::{
        entities::{asset_token_key::AssetTokenKey, coin::CoinData},
        repositories::{coin::CoinRepo, exchange_rate::ExchangeRateRepo},
    };
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    let pool = ctx.core_pool().unwrap();
    ExchangeRateRepo::upsert(pool.clone(), "USD", "USD", 1.0).await.unwrap();
    for code in [ChainCode::Base, ChainCode::Scroll, ChainCode::Ethereum] {
        CoinRepo::upsert_multi_coin(
            &pool,
            vec![CoinData {
                chain_code: code.to_string(),
                symbol: "ETH".into(),
                decimals: 18,
                token_address: AssetTokenKey::Native,
                price: Some("0".into()),
                ..Default::default()
            }],
        )
        .await
        .unwrap();
        for token in [false, true] {
            let l1 = U256::from(1_001_u64);
            let rpc = Rpc::new(U256::from(10_u64).pow(U256::from(23)), U256::MAX, Ok(l1));
            let adapter =
                EthTx::new_for_test(ctx, code, &rpc.url, NetworkKind::Mainnet, None).unwrap();
            let (req, _) = request(code, token, true);
            assert!(!req.base.spend_all);
            let quote: Value =
                serde_json::from_str(&adapter.estimate_fee(req.base.clone(), "ETH").await.unwrap())
                    .unwrap();
            let tier = &quote["data"][0];
            let setting: crate::response_vo::EthereumFeeDetails =
                serde_json::from_value(tier["feeSetting"].clone()).unwrap();
            let setting: FeeSetting = setting.try_into().unwrap();
            let extra = if code == ChainCode::Ethereum { U256::ZERO } else { l1 };
            let exact = EthTx::checked_total_fee(&setting, extra).unwrap();
            let expected =
                exact + if code == ChainCode::Base { U256::from(201_u64) } else { U256::ZERO };
            assert_eq!(
                unit::convert_to_u256(tier["maxFee"]["amount"].as_str().unwrap(), 18).unwrap(),
                expected
            );
            assert_eq!(
                unit::convert_to_u256(tier["estimateFee"]["amount"].as_str().unwrap(), 18).unwrap(),
                setting.gas_limit * (setting.base_fee + setting.max_priority_fee_per_gas) + extra
            );
            assert_eq!(setting.gas_limit, U256::from(23_100_u64));
            if code != ChainCode::Scroll {
                assert_eq!(setting.max_fee_per_gas, U256::from(100_000_000_u64));
            }
            assert_eq!(req.base.value, "1");
            assert!(
                !rpc.calls.lock().unwrap().iter().any(|r| r["method"] == "eth_sendRawTransaction")
            );
            if code == ChainCode::Base {
                *rpc.extra.lock().unwrap() = Err("L1 RPC unavailable");
                assert!(adapter.estimate_fee(req.base.clone(), "ETH").await.is_err());
            }
        }
    }
}

#[tokio::test]
async fn api_base_automatic_funding_keeps_single_margin_and_rechecks_live_l1() {
    let ctx = crate::testkit::context::api_trans_test_ctx().await;
    // Existing worker funds ceil((63000 execution + 1100429989 L1) * 1.2).
    let budget = U256::from(1_320_591_587_u64);
    for token in [false, true] {
        for (live_l1, fits) in [(1_281_424_779_u64, true), (1_320_528_588, false)] {
            let (req, key) = request(ChainCode::Base, token, true);
            let value = unit::convert_to_u256(&req.base.value, req.base.decimals).unwrap();
            let balance = budget + if token { U256::ZERO } else { value };
            let rpc = Rpc::new(balance, value, Ok(U256::from(1_100_429_989_u64)));
            let adapter =
                EthTx::new_for_test(ctx, ChainCode::Base, &rpc.url, NetworkKind::Mainnet, None)
                    .unwrap();
            assert_eq!(
                adapter.estimate_native_fee_wei(req.base.clone()).await.unwrap(),
                U256::from(1_100_492_989_u64)
            );
            *rpc.extra.lock().unwrap() = Ok(U256::from(live_l1));
            let result = adapter.build_transfer_raw(&req, key).await;
            if fits {
                let (_, raw, fee) = result.unwrap();
                let RawTx::Evm(_, actual_fee) = raw else { panic!("expected EVM") };
                assert_eq!(actual_fee, U256::from(live_l1 + 63_000));
                assert_eq!(unit::convert_to_u256(&fee, 18).unwrap(), actual_fee);
            } else {
                assert!(matches!(
                    result.unwrap_err(),
                    ServiceError::Business(BusinessError::Chain(
                        ChainError::InsufficientFeeBalance
                    ))
                ));
                assert!(!rpc.calls.lock().unwrap().iter().any(|r| r["method"] == "eth_chainId"));
            }
            assert_eq!(req.base.value, "1");
            assert!(!req.base.spend_all);
            assert!(
                !rpc.calls.lock().unwrap().iter().any(|r| r["method"] == "eth_sendRawTransaction")
            );
        }
    }
}
