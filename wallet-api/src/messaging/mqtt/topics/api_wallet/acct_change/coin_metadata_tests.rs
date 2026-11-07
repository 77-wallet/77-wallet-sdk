use super::*;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use wallet_types::chain::{chain::ChainCode, network::NetworkKind};

fn rpc(fail_decimals: bool) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let task = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(4);
        while Instant::now() < until {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0u8; 4096];
            let (header_len, length) = loop {
                let n = stream.read(&mut buf).unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..i]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    break (i + 4, len);
                }
            };
            while bytes.len() < header_len + length {
                let n = stream.read(&mut buf).unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let req: Value =
                serde_json::from_slice(&bytes[header_len..header_len + length]).unwrap();
            let data = req["params"][0]["data"]
                .as_str()
                .or_else(|| req["params"][0]["input"].as_str())
                .unwrap_or("")
                .to_string();
            recorded.lock().unwrap().push(data.clone());
            let result = if data.starts_with("0x313ce567") && !fail_decimals {
                json!({"jsonrpc":"2.0","id":req["id"],"result":format!("0x{:064x}",6)})
            } else {
                json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":-32602,"message":"unexpected metadata call or injected decimals failure"}})
            };
            let body = result.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    (url, calls, task)
}

#[tokio::test]
async fn api_acct_change_coin_uses_notification_symbol_and_only_queries_decimals() {
    // Arrange: unknown token; the fake rejects symbol()/name() calls.
    let (url, calls, server) = rpc(false);
    let adapter = crate::domain::chain::adapter::TransactionAdapter::new(
        ChainCode::Polygon,
        &url,
        None,
        NetworkKind::Mainnet,
    )
    .unwrap();
    // Act: runtime notification supplies USDT, independent of catalog membership.
    let coin = ApiWalletAcctChange::build_acct_change_coin(
        &adapter,
        "polygon",
        "0x0000000000000000000000000000000000000003",
        "USDT",
        None,
    )
    .await
    .unwrap();
    // Assert: no on-chain name/symbol can overwrite the notification.
    assert_eq!(
        (coin.symbol.as_str(), coin.name.as_deref(), coin.decimals),
        ("USDT", Some("USDT"), 6)
    );
    // A token absent from the backend must still be persistable (price is NOT NULL).
    let dir = tempfile::tempdir().unwrap();
    let pool =
        wallet_database::SqliteContext::new(dir.path().to_str().unwrap(), Some("api_wallet.db"))
            .await
            .unwrap()
            .into_api_wallet_db_pool()
            .unwrap();
    ApiCoinRepo::upsert_multi_coin(&pool, vec![coin]).await.unwrap();
    let saved = ApiCoinRepo::coin_list(&pool).await.unwrap();
    assert_eq!(
        (saved[0].symbol.as_str(), saved[0].name.as_str(), saved[0].price.as_str()),
        ("USDT", "USDT", "0")
    );
    server.join().unwrap();
    assert_eq!(*calls.lock().unwrap(), vec!["0x313ce567"]);
}

#[tokio::test]
async fn api_acct_change_coin_does_not_guess_precision_on_rpc_failure() {
    let (url, _, server) = rpc(true);
    let adapter = crate::domain::chain::adapter::TransactionAdapter::new(
        ChainCode::Polygon,
        &url,
        None,
        NetworkKind::Mainnet,
    )
    .unwrap();
    let result = ApiWalletAcctChange::build_acct_change_coin(
        &adapter,
        "polygon",
        "0x0000000000000000000000000000000000000003",
        "USDT",
        Some("1".into()),
    )
    .await;
    assert!(result.is_err());
    server.join().unwrap();
}
