use super::*;
use crate::domain::chain::tx_visibility::{EvmLookupContract, evm_transaction_seen};
async fn visible(client: &RpcClient, hash: &str) -> Result<bool, ServiceError> {
    evm_transaction_seen(client, hash, EvmLookupContract::OrdinaryDiagnostic).await
}
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
async fn server(results: Vec<serde_json::Value>) -> (RpcClient, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = RpcClient::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        Some(std::time::Duration::from_secs(1)),
    )
    .unwrap();
    let task = tokio::spawn(async move {
        for mut result in results {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let request = loop {
                let n = stream.read(&mut buffer).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let len = header
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + len {
                        let req: serde_json::Value =
                            serde_json::from_slice(&bytes[end + 4..]).unwrap();
                        let expected = result
                            .get("_method")
                            .and_then(|v| v.as_str())
                            .unwrap_or("eth_getTransactionByHash");
                        assert_eq!(req["method"], expected);
                        break req;
                    }
                }
            };
            if result.get("_ack").and_then(|v| v.as_bool()) == Some(true) {
                let raw =
                    hex::decode(request["params"][0].as_str().unwrap().trim_start_matches("0x"))
                        .unwrap();
                result["result"] = json!(format!("{:#x}", alloy::primitives::keccak256(raw)));
            }
            result.as_object_mut().unwrap().remove("_method");
            result.as_object_mut().unwrap().remove("_ack");
            let body = result
                .get("_raw_body")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| result.to_string());
            let wire = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(wire.as_bytes()).await.unwrap();
        }
    });
    (client, task)
}
#[tokio::test]
async fn visibility_distinguishes_null_error_identity_and_inclusion() {
    let (client, server) = server(vec![
        json!({"result":null}),
        json!({"error":{"code":429,"message":"secret provider body"}}),
        json!({}),
        json!({"result":{"hash":"other"}}),
        json!({"result":{"hash":"0xabc","blockNumber":null}}),
        json!({"result":{"hash":"0xABC","blockNumber":"0x1"}}),
    ])
    .await;
    assert!(!visible(&client, "0xabc").await.unwrap());
    for _ in 0..3 {
        assert!(visible(&client, "0xabc").await.is_err());
    }
    assert!(visible(&client, "0xabc").await.unwrap());
    assert!(visible(&client, "0xabc").await.unwrap());
    server.await.unwrap();
}
#[tokio::test]
async fn observer_is_bounded_and_sends_only_read_requests() {
    let (client, server) = server(vec![json!({"result":null}); 3]).await;
    observe(client, format!("0x{}", "ab".repeat(32)));
    tokio::time::timeout(std::time::Duration::from_secs(4), server).await.unwrap().unwrap();
}
#[test]
fn endpoint_fingerprint_never_exposes_credentials() {
    let url = "https://user:secret@example.test/api/private-key?token=secret";
    let id = rpc_id(url);
    assert_eq!(id.len(), 66);
    assert!(!id.contains("secret"));
    assert_eq!(rpc_id(url), id);
}

#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for Captured {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn signed_submission_records_ack_and_error_without_raw_or_provider_body() {
    use alloy::network::TransactionBuilder as _;
    for fail in [false, true] {
        let (client,server)=server(vec![json!({"_method":"eth_getTransactionCount","result":"0x6"}),json!({"_method":"eth_getBlockByNumber","result":snapshot_block(1_000_000_000)}),json!({"_method":"eth_chainId","result":"0x89"}),if fail{json!({"_method":"eth_sendRawTransaction","error":{"code":-32000,"message":"provider-secret-body"}})}else{json!({"_method":"eth_sendRawTransaction","_ack":true,"result":null})}]).await;
        let chain = EthChain::new(
            wallet_chain_interact::eth::Provider::new(client).unwrap(),
            wallet_types::chain::network::NetworkKind::Mainnet,
            wallet_types::chain::chain::ChainCode::Polygon,
        )
        .unwrap();
        let signer = alloy::signers::local::PrivateKeySigner::from_bytes(
            &alloy::primitives::B256::repeat_byte(7),
        )
        .unwrap();
        let tx = alloy::rpc::types::TransactionRequest::default()
            .with_from(signer.address())
            .with_to(alloy::primitives::Address::repeat_byte(2))
            .with_value(alloy::primitives::U256::from(1))
            .with_gas_limit(21000)
            .with_max_fee_per_gas(2_000_000_000)
            .with_max_priority_fee_per_gas(1_000_000_000);
        let logs = Captured::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let result = send(
            &chain,
            tx,
            &format!("{:x}", alloy::primitives::B256::repeat_byte(7)),
            Some(
                r#"{"quoteId":"123e4567-e89b-12d3-a456-426614174000","quotedAtMs":1793327722000}"#,
            ),
        )
        .await;
        assert_eq!(result.is_err(), fail);
        server.await.unwrap();
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("nonce=6") && text.contains("chain_id=137"), "{text}");
        assert!(
            text.contains("123e4567-e89b-12d3-a456-426614174000") && text.contains("1793327722000")
        );
        assert!(text.contains(if fail { "send_error" } else { "ack_matches=true" }), "{text}");
        assert!(
            !text.contains("provider-secret-body")
                && !text.contains("http://")
                && !text.contains(&"07".repeat(32))
        );
    }
}

#[tokio::test]
async fn malformed_ack_is_not_queried_or_logged() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client =
        RpcClient::new(&format!("http://{}", listener.local_addr().unwrap()), None, None).unwrap();
    let logs = Captured::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    observe(client, "https://provider.invalid/?token=secret-body".into());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), listener.accept())
            .await
            .is_err()
    );
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("invalid_ack_hash"));
    assert!(!text.contains("https://") && !text.contains("secret-body"));
}

// Freeze the pre-refactor API contract, including the existing null/empty error.
#[tokio::test]
async fn api_visibility_legacy_contract_characterization() {
    use wallet_utils::RetryableError as _;
    let cases = vec![
        (json!({"result":null}), "empty"),
        (json!({"_raw_body":"<not JSON"}), "transport_utils"),
        (json!({"error":{"code":"invalid","message":"not logged"}}), "transport_utils"),
        (json!({}), "empty"),
        (json!({"result":{"hash":"different"}}), "seen"),
        (json!({"result":{}}), "seen"),
        (json!({"result":false}), "seen"),
        (json!({"error":{"code":401,"message":"not logged"}}), "auth"),
        (json!({"error":{"code":429,"message":"not logged"}}), "rpc"),
    ];
    let (client, server) =
        server(cases.iter().flat_map(|(v, _)| [v.clone(), v.clone()]).collect()).await;
    for (_, expected) in cases {
        let params = wallet_transport::types::JsonRpcParams::default()
            .method("eth_getTransactionByHash")
            .params(vec!["0xabc"]);
        let outcome: Result<Option<serde_json::Value>, ServiceError> =
            client.invoke_request(params).await.map_err(Into::into);
        let shared =
            evm_transaction_seen(&client, "0xabc", EvmLookupContract::ApiWallet("[selected RPC]"))
                .await;
        let shared_class = match shared {
            Ok(true) => "seen",
            Ok(false) => "absent",
            Err(ServiceError::Transport(wallet_transport::TransportError::EmptyResult)) => "empty",
            Err(ServiceError::Transport(wallet_transport::TransportError::Utils(_))) => {
                "transport_utils"
            }
            Err(e) if e.is_rpc_auth_unauthorized() => "auth",
            Err(ServiceError::Transport(wallet_transport::TransportError::NodeResponseError(
                _,
            ))) => "rpc",
            Err(_) => "other",
        };
        assert_eq!(shared_class, expected);
        let actual = match outcome {
            Ok(Some(_)) => "seen",
            Ok(None) => "absent",
            Err(ServiceError::Transport(wallet_transport::TransportError::EmptyResult)) => "empty",
            Err(ServiceError::Transport(wallet_transport::TransportError::Utils(_))) => {
                "transport_utils"
            }
            Err(e) if e.is_rpc_auth_unauthorized() => "auth",
            Err(ServiceError::Transport(wallet_transport::TransportError::NodeResponseError(
                _,
            ))) => "rpc",
            Err(e) => panic!("unexpected error class: {}", e.is_delay_retryable()),
        };
        assert_eq!(actual, expected);
    }
    server.await.unwrap();
}

fn snapshot_block(base: u64) -> serde_json::Value {
    let block: alloy::rpc::types::Block = Default::default();
    let mut block = serde_json::to_value(block).unwrap();
    block["baseFeePerGas"] = json!(format!("0x{base:x}"));
    block["number"] = json!("0x1");
    block
}

#[tokio::test]
async fn fee_freshness_checks_mainnet_caps_without_changing_user_fees() {
    use super::super::evm_fee::check_before_signing;
    for code in [
        wallet_types::chain::chain::ChainCode::Ethereum,
        wallet_types::chain::chain::ChainCode::Polygon,
        wallet_types::chain::chain::ChainCode::Base,
        wallet_types::chain::chain::ChainCode::Scroll,
        wallet_types::chain::chain::ChainCode::Arbitrum,
        wallet_types::chain::chain::ChainCode::Avalanche,
    ] {
        for cap in [99_u128, 100, 101] {
            let (client, server) = server(vec![
                json!({"_method":"eth_getBlockByNumber","result":snapshot_block(100)}),
            ])
            .await;
            let chain = EthChain::new(
                wallet_chain_interact::eth::Provider::new(client).unwrap(),
                wallet_types::chain::network::NetworkKind::Mainnet,
                code,
            )
            .unwrap();
            let tx = alloy::rpc::types::TransactionRequest {
                max_fee_per_gas: Some(cap),
                max_priority_fee_per_gas: Some(25),
                ..Default::default()
            };
            let result = check_before_signing(&chain, &tx, 6, None).await;
            assert_eq!(result.is_ok(), cap >= 100, "{code} cap{cap}");
            assert_eq!(tx.max_fee_per_gas, Some(cap));
            if let Err(error) = result {
                let (status, _): (i64, String) = error.into();
                assert_eq!(status, 3524);
            }
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn fee_freshness_snapshot_errors_stop_before_signing_and_sanitize_logs() {
    let (client, server) = server(vec![
        json!({"_method":"eth_getTransactionCount","result":"0x6"}),
        json!({"_method":"eth_getBlockByNumber","error":{"code":504,"message":"secret-provider-body"}}),
    ]).await;
    let chain = EthChain::new(
        wallet_chain_interact::eth::Provider::new(client).unwrap(),
        wallet_types::chain::network::NetworkKind::Mainnet,
        wallet_types::chain::chain::ChainCode::Polygon,
    )
    .unwrap();
    let tx = alloy::rpc::types::TransactionRequest {
        from: Some(alloy::primitives::Address::repeat_byte(1)),
        max_fee_per_gas: Some(100),
        ..Default::default()
    };
    let logs = Captured::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    assert!(
        send(&chain, tx, "invalid-test-key", Some(r#"{"quoteId":"malformed-secret-value"}"#))
            .await
            .is_err()
    );
    server.await.unwrap();
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("pre_sign_snapshot_error"));
    assert!(
        !text.contains("secret-provider-body")
            && !text.contains("malformed-secret-value")
            && !text.contains("http://")
    );
}

#[tokio::test]
async fn fee_freshness_bnb_and_testnets_keep_previous_fee_policy() {
    use super::super::evm_fee::check_before_signing;
    for (code, network) in [
        (
            wallet_types::chain::chain::ChainCode::BnbSmartChain,
            wallet_types::chain::network::NetworkKind::Mainnet,
        ),
        (
            wallet_types::chain::chain::ChainCode::Polygon,
            wallet_types::chain::network::NetworkKind::Testnet,
        ),
    ] {
        let (client, server) = server(vec![]).await;
        let chain = EthChain::new(
            wallet_chain_interact::eth::Provider::new(client).unwrap(),
            network,
            code,
        )
        .unwrap();
        assert!(check_before_signing(&chain, &Default::default(), 6, None).await.is_ok());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn fee_freshness_network_timeout_never_authorizes_signing() {
    use super::super::evm_fee::check_before_signing;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = RpcClient::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        None,
        Some(std::time::Duration::from_millis(50)),
    )
    .unwrap();
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        let size = socket.read(&mut bytes).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes[..size]).contains("eth_getBlockByNumber"));
        std::future::pending::<()>().await;
    });
    let chain = EthChain::new(
        wallet_chain_interact::eth::Provider::new(client).unwrap(),
        wallet_types::chain::network::NetworkKind::Mainnet,
        wallet_types::chain::chain::ChainCode::Polygon,
    )
    .unwrap();
    let tx =
        alloy::rpc::types::TransactionRequest { max_fee_per_gas: Some(100), ..Default::default() };
    let error = check_before_signing(&chain, &tx, 6, None)
        .await
        .err()
        .expect("snapshot timeout must fail closed");
    assert!(matches!(
        crate::service::transaction::TransactionService::query_error_kind(&error).0,
        "network"
    ));
    worker.abort();
}

#[tokio::test]
async fn fee_freshness_missing_base_fee_is_not_treated_as_zero() {
    use super::super::evm_fee::check_before_signing;
    let mut block = snapshot_block(100);
    block.as_object_mut().unwrap().remove("baseFeePerGas");
    let (client, server) =
        server(vec![json!({"_method":"eth_getBlockByNumber","result":block})]).await;
    let chain = EthChain::new(
        wallet_chain_interact::eth::Provider::new(client).unwrap(),
        wallet_types::chain::network::NetworkKind::Mainnet,
        wallet_types::chain::chain::ChainCode::Polygon,
    )
    .unwrap();
    let tx =
        alloy::rpc::types::TransactionRequest { max_fee_per_gas: Some(100), ..Default::default() };
    assert!(check_before_signing(&chain, &tx, 6, None).await.is_err());
    server.await.unwrap();
}
