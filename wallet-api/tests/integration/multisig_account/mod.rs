mod support;

use serial_test::serial;
use support::{EvmDeployScenario, FakeEvmRpc};
use wallet_chain_interact::eth::operations::MultisigPayloadOpt;
use wallet_database::entities::multisig_queue::MultisigQueueStatus;

fn polygon_token_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("eth_call", serde_json::json!("0x989680")),
        ("eth_call", serde_json::json!("0x989680")),
        ("eth_call", serde_json::json!("0x0")),
        ("eth_call", serde_json::json!(format!("0x{}", "aa".repeat(32)))),
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0x89")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]
}

fn avalanche_token_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = polygon_token_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0xa86a");
    responses
}

fn arbitrum_token_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = polygon_token_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0xa4b1");
    responses
}

fn base_token_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = polygon_token_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0x2105");
    responses
}

fn scroll_token_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = polygon_token_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0x82750");
    responses
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_receipt_confirms_queue_bill_and_token_balance() {
    // Given a broadcast Scroll token Safe transfer and a successful receipt.
    let mut responses = scroll_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_call", serde_json::json!("0x895440")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync reads the successful receipt and token balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and token balance updates to 9.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_token_asset_balance(9.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Scroll token Safe transfer with a failed receipt.
    let mut responses = scroll_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_call", serde_json::json!("0x989680")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync observes failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then both records fail and token balance remains 10.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_token_asset_balance(10.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Scroll token Safe transfer with no receipt yet.
    let mut responses = scroll_token_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation remains pending without token balance refresh.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_token_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Scroll Safe holding the test ERC-20 token.
    let rpc = FakeEvmRpc::new_json(scroll_token_broadcast_responses());
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service signs the proposal and broadcasts its execution.
    let proposal_hash = scenario.when_create_token_queue().await.expect("create token queue");
    scenario.then_token_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_token_queue().await.expect("execute token queue");

    // Then token identity, chain hash, and pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&chain_hash).await;
    let repeat_error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(repeat_error.contains("already been executed"), "{repeat_error}");
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_service_insufficient_balance_leaves_no_queue() {
    // Given a Scroll Safe without spendable test ERC-20 tokens.
    let rpc = FakeEvmRpc::new(&[("eth_call", "0x0"), ("eth_call", "0x0")]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service attempts a token transfer proposal.
    let error = scenario.when_create_token_queue().await.unwrap_err();

    // Then no Safe nonce is read and no queue is created.
    assert!(error.contains("token balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Scroll token queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When execution checks the initiator's ETH gas balance.
    let error = scenario.when_execute_token_queue().await.unwrap_err();

    // Then the signed queue stays ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn scroll_token_safe_service_retries_rejected_broadcast_once() {
    // Given a signed token queue whose first Scroll broadcast is rejected.
    let mut responses = scroll_token_broadcast_responses();
    responses.last_mut().unwrap().1 = serde_json::json!("__rpc_rejected__");
    responses.extend([
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0x82750")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]);
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When the first broadcast fails, the signed queue stays ready.
    let error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(error.contains("transaction rejected by fake RPC"), "{error}");
    scenario.then_native_queue_remains_ready().await;

    // Then retry broadcasts successfully and saves one pending token bill.
    let hash = scenario.when_execute_token_queue().await.expect("retry token queue");
    scenario.then_token_queue_execution_persisted(&hash).await;
    let calls = rpc.finish();
    assert_eq!(calls.iter().filter(|call| call.as_str() == "eth_sendRawTransaction").count(), 2);
}

#[tokio::test]
#[serial]
async fn base_token_safe_receipt_confirms_queue_bill_and_token_balance() {
    // Given a broadcast Base token Safe transfer and a successful receipt.
    let mut responses = base_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_call", serde_json::json!("0x895440")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync reads the successful receipt and token balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and the token balance updates to 9.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_token_asset_balance(9.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn base_token_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Base token Safe transfer with a failed receipt.
    let mut responses = base_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_call", serde_json::json!("0x989680")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync observes failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then both records fail and the token balance remains 10.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_token_asset_balance(10.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn base_token_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Base token Safe transfer with no receipt yet.
    let mut responses = base_token_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation remains pending without a token balance refresh.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_token_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn base_token_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Base Safe holding the test ERC-20 token.
    let rpc = FakeEvmRpc::new_json(base_token_broadcast_responses());
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service signs the proposal and broadcasts its execution.
    let proposal_hash = scenario.when_create_token_queue().await.expect("create token queue");
    scenario.then_token_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_token_queue().await.expect("execute token queue");

    // Then the token identity, chain hash, and pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&chain_hash).await;
    let repeat_error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(repeat_error.contains("already been executed"), "{repeat_error}");
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn base_token_safe_service_insufficient_balance_leaves_no_queue() {
    // Given a Base Safe without spendable test ERC-20 tokens.
    let rpc = FakeEvmRpc::new(&[("eth_call", "0x0"), ("eth_call", "0x0")]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service attempts a token transfer proposal.
    let error = scenario.when_create_token_queue().await.unwrap_err();

    // Then no Safe nonce is read and no queue is created.
    assert!(error.contains("token balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn base_token_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Base token queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When execution checks the initiator's ETH gas balance.
    let error = scenario.when_execute_token_queue().await.unwrap_err();

    // Then the signed queue stays ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn base_token_safe_service_retries_rejected_broadcast_once() {
    // Given a signed token queue whose first Base broadcast is rejected.
    let mut responses = base_token_broadcast_responses();
    responses.last_mut().unwrap().1 = serde_json::json!("__rpc_rejected__");
    responses.extend([
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0x2105")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]);
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When the first broadcast fails, the signed queue stays ready.
    let error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(error.contains("transaction rejected by fake RPC"), "{error}");
    scenario.then_native_queue_remains_ready().await;

    // Then a retry broadcasts successfully and saves one pending token bill.
    let hash = scenario.when_execute_token_queue().await.expect("retry token queue");
    scenario.then_token_queue_execution_persisted(&hash).await;
    let calls = rpc.finish();
    assert_eq!(calls.iter().filter(|call| call.as_str() == "eth_sendRawTransaction").count(), 2);
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_receipt_confirms_queue_bill_and_token_balance() {
    // Given a broadcast Arbitrum token Safe transfer and a successful receipt.
    let mut responses = arbitrum_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_call", serde_json::json!("0x895440")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync reads the successful receipt and token balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and the token balance updates to 9.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_token_asset_balance(9.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Arbitrum token Safe transfer with a failed receipt.
    let mut responses = arbitrum_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_call", serde_json::json!("0x989680")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync observes failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then both records fail and the token balance remains 10.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_token_asset_balance(10.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Arbitrum token Safe transfer with no receipt yet.
    let mut responses = arbitrum_token_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation remains pending without a token balance refresh.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_token_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Arbitrum Safe holding the test ERC-20 token.
    let rpc = FakeEvmRpc::new_json(arbitrum_token_broadcast_responses());
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service signs the proposal and broadcasts its execution.
    let proposal_hash = scenario.when_create_token_queue().await.expect("create token queue");
    scenario.then_token_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_token_queue().await.expect("execute token queue");

    // Then the token identity, chain hash, and pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&chain_hash).await;
    let repeat_error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(repeat_error.contains("already been executed"), "{repeat_error}");
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_service_insufficient_balance_leaves_no_queue() {
    // Given an Arbitrum Safe with no spendable test ERC-20 tokens.
    let rpc = FakeEvmRpc::new(&[("eth_call", "0x0"), ("eth_call", "0x0")]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When public Service attempts a token transfer proposal.
    let error = scenario.when_create_token_queue().await.unwrap_err();

    // Then no Safe nonce is read and no queue is created.
    assert!(error.contains("token balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Arbitrum token queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When execution checks the initiator's ETH gas balance.
    let error = scenario.when_execute_token_queue().await.unwrap_err();

    // Then the signed queue remains ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn arbitrum_token_safe_service_retries_rejected_broadcast_once() {
    // Given a signed token queue whose first Arbitrum broadcast is rejected.
    let mut responses = arbitrum_token_broadcast_responses();
    responses.last_mut().unwrap().1 = serde_json::json!("__rpc_rejected__");
    responses.extend([
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0xa4b1")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]);
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When the first broadcast fails, the signed queue stays ready.
    let error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(error.contains("transaction rejected by fake RPC"), "{error}");
    scenario.then_native_queue_remains_ready().await;

    // Then a retry broadcasts successfully and saves one pending token bill.
    let hash = scenario.when_execute_token_queue().await.expect("retry token queue");
    scenario.then_token_queue_execution_persisted(&hash).await;
    let calls = rpc.finish();
    assert_eq!(calls.iter().filter(|call| call.as_str() == "eth_sendRawTransaction").count(), 2);
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_receipt_confirms_queue_bill_and_token_balance() {
    // Given a broadcast Avalanche token Safe transfer and a successful receipt.
    let mut responses = avalanche_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_call", serde_json::json!("0x895440")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync reads the successful receipt and token balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the bill and queue succeed and the cached token balance is refreshed.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_token_asset_balance(9.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Avalanche token transfer with an explicitly failed receipt.
    let mut responses = avalanche_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_call", serde_json::json!("0x989680")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync observes the failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then both records fail and the token balance remains 10.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_token_asset_balance(10.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Avalanche token transfer with no readable receipt yet.
    let mut responses = avalanche_token_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When public bill sync cannot establish an execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation and the cached token balance remain untouched.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_token_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Avalanche Safe holding the test ERC-20 token.
    let rpc = FakeEvmRpc::new_json(avalanche_token_broadcast_responses());
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When the public Service signs the proposal and broadcasts its execution.
    let proposal_hash = scenario.when_create_token_queue().await.expect("create token queue");
    scenario.then_token_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_token_queue().await.expect("execute token queue");

    // Then the queue and pending bill retain the token identity on Avalanche.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&chain_hash).await;
    let repeat_error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(repeat_error.contains("already been executed"), "{repeat_error}");
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_service_insufficient_balance_leaves_no_queue() {
    // Given an Avalanche Safe with no spendable test ERC-20 tokens.
    let rpc = FakeEvmRpc::new(&[("eth_call", "0x0"), ("eth_call", "0x0")]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When the public Service attempts to create a token proposal.
    let error = scenario.when_create_token_queue().await.unwrap_err();

    // Then no Safe nonce is read and no queue is created.
    assert!(error.contains("token balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Avalanche token queue and no initiator AVAX for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When execution checks the initiator's gas funding.
    let error = scenario.when_execute_token_queue().await.unwrap_err();

    // Then the signed queue stays ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn avalanche_token_safe_service_retries_rejected_broadcast_once() {
    // Given a signed token queue whose first RPC broadcast is rejected.
    let mut responses = avalanche_token_broadcast_responses();
    responses.last_mut().unwrap().1 = serde_json::json!("__rpc_rejected__");
    responses.extend([
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0xa86a")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]);
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");

    // When the first broadcast fails, the queue remains ready for retry.
    let error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(error.contains("transaction rejected by fake RPC"), "{error}");
    scenario.then_native_queue_remains_ready().await;

    // Then a retry broadcasts once successfully and saves one pending bill.
    let hash = scenario.when_execute_token_queue().await.expect("retry token queue");
    scenario.then_token_queue_execution_persisted(&hash).await;
    let calls = rpc.finish();
    assert_eq!(calls.iter().filter(|call| call.as_str() == "eth_sendRawTransaction").count(), 2);
}

fn avalanche_native_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_call", serde_json::json!("0x0")),
        ("eth_call", serde_json::json!(format!("0x{}", "aa".repeat(32)))),
        ("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")),
        ("eth_getTransactionCount", serde_json::json!("0x0")),
        ("eth_chainId", serde_json::json!("0xa86a")),
        ("eth_sendRawTransaction", serde_json::json!(format!("0x{}", "bb".repeat(32)))),
    ]
}

fn arbitrum_native_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = avalanche_native_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0xa4b1");
    responses
}

fn base_native_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = avalanche_native_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0x2105");
    responses
}

fn scroll_native_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = avalanche_native_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0x82750");
    responses
}

fn polygon_native_broadcast_responses() -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = avalanche_native_broadcast_responses();
    let chain_id = responses.iter_mut().find(|(method, _)| *method == "eth_chainId").unwrap();
    chain_id.1 = serde_json::json!("0x89");
    responses
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_receipt_confirms_queue_bill_and_pol_balance() {
    // Given a broadcast Polygon POL Safe transfer and a successful receipt.
    let mut responses = polygon_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_getBalance", serde_json::json!("0xc7d713b49da0000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create POL queue");
    scenario.when_execute_native_queue().await.expect("broadcast POL queue");

    // When public bill sync reads the successful receipt and Safe POL balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and native balance updates to 0.9 POL.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_native_asset_balance(0.9).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Polygon POL Safe transfer with a failed receipt.
    let mut responses = polygon_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create POL queue");
    scenario.when_execute_native_queue().await.expect("broadcast POL queue");

    // When public bill sync observes failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill fail while the Safe still holds 1 POL.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_native_asset_balance(1.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Polygon POL Safe transfer with no readable receipt.
    let mut responses = polygon_native_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create POL queue");
    scenario.when_execute_native_queue().await.expect("broadcast POL queue");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation and cached POL balance remain unchanged.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_native_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_receipt_confirms_queue_bill_and_eth_balance() {
    // Given a broadcast Scroll ETH Safe transfer and a successful receipt.
    let mut responses = scroll_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_getBalance", serde_json::json!("0xc7d713b49da0000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync reads the successful receipt and Safe ETH balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and native balance updates to 0.9 ETH.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_native_asset_balance(0.9).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Scroll ETH Safe transfer with a failed receipt.
    let mut responses = scroll_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync observes the failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill fail while the Safe still holds 1 ETH.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_native_asset_balance(1.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Scroll ETH Safe transfer with no readable receipt.
    let mut responses = scroll_native_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation and cached ETH balance remain unchanged.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_native_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn base_native_safe_receipt_confirms_queue_bill_and_eth_balance() {
    // Given a broadcast Base ETH Safe transfer and a successful receipt.
    let mut responses = base_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_getBalance", serde_json::json!("0xc7d713b49da0000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync reads the successful receipt and Safe ETH balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and the native balance updates to 0.9 ETH.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_native_asset_balance(0.9).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn base_native_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Base ETH Safe transfer with a failed receipt.
    let mut responses = base_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync observes the failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill fail while the Safe holds 1 ETH.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_native_asset_balance(1.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn base_native_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Base ETH Safe transfer with no readable receipt.
    let mut responses = base_native_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation and the cached ETH balance remain unchanged.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_native_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_receipt_confirms_queue_bill_and_eth_balance() {
    // Given a broadcast Arbitrum ETH Safe transfer and a successful receipt.
    let mut responses = arbitrum_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_getBalance", serde_json::json!("0xc7d713b49da0000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync reads the receipt and refreshed Safe ETH balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and native ETH balance is updated.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_native_asset_balance(0.9).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast Arbitrum ETH Safe transfer with a failed receipt.
    let mut responses = arbitrum_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync observes the failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill fail while the Safe still holds 1 ETH.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_native_asset_balance(1.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast Arbitrum ETH Safe transfer with no readable receipt.
    let mut responses = arbitrum_native_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH queue");
    scenario.when_execute_native_queue().await.expect("broadcast ETH queue");

    // When public bill sync cannot establish the execution outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation stays pending without refreshing cached ETH balance.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_native_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_receipt_confirms_queue_bill_and_avax_balance() {
    // Given a broadcast AVAX Safe transfer and a successful receipt.
    let mut responses = avalanche_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_getBalance", serde_json::json!("0xc7d713b49da0000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create AVAX queue");
    scenario.when_execute_native_queue().await.expect("broadcast AVAX queue");

    // When public bill sync reads the receipt and refreshed Safe balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill succeed and native AVAX balance is updated.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_native_asset_balance(0.9).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast AVAX Safe transfer with an explicitly failed receipt.
    let mut responses = avalanche_native_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_getBalance", serde_json::json!("0xde0b6b3a7640000")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create AVAX queue");
    scenario.when_execute_native_queue().await.expect("broadcast AVAX queue");

    // When public bill sync observes the failed execution.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then the queue and bill fail, while the native balance stays at 1 AVAX.
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_native_asset_balance(1.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast AVAX Safe transfer whose receipt is unavailable.
    let mut responses = avalanche_native_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create AVAX queue");
    scenario.when_execute_native_queue().await.expect("broadcast AVAX queue");

    // When public bill sync cannot establish the transaction outcome.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then confirmation stays pending, without a balance refresh.
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_native_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_receipt_confirms_queue_and_bill() {
    // Given a broadcast ERC-20 Safe transfer and a successful Polygon receipt.
    let mut responses = polygon_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(true));
    responses.push(("eth_call", serde_json::json!("0x895440")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When the public bill sync reads the receipt and refreshed token balance.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);

    // Then both records settle successfully without enabling ordinary transfers.
    scenario.then_queue_execution_status(2, 4).await;
    scenario.then_token_asset_balance(9.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_failed_receipt_marks_queue_and_bill_failed() {
    // Given a broadcast token transfer with an explicitly failed receipt.
    let mut responses = polygon_token_broadcast_responses();
    responses.extend(polygon_execution_receipt(false));
    responses.push(("eth_call", serde_json::json!("0x989680")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When bill sync observes failed execution, both records settle as failed.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);
    scenario.then_queue_execution_status(3, 5).await;
    scenario.then_token_asset_balance(10.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_call"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_missing_receipt_keeps_confirmation_pending() {
    // Given a broadcast token transfer whose receipt is temporarily unavailable.
    let mut responses = polygon_token_broadcast_responses();
    responses.push(("eth_getTransactionReceipt", serde_json::json!("unavailable")));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create token queue");
    scenario.when_execute_token_queue().await.expect("broadcast token transfer");

    // When bill sync cannot establish an outcome, it preserves confirmation.
    assert_eq!(scenario.when_sync_queue_execution().await.unwrap(), 1);
    scenario.then_queue_execution_status(1, 3).await;
    scenario.then_token_asset_balance(0.0).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getTransactionReceipt"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_service_executes_signed_queue_with_token_bill() {
    // Given a signed ERC-20 queue on an on-chain Polygon Safe.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create signed token queue");

    // When the public Service broadcasts the threshold-signed token transfer.
    let hash = scenario.when_execute_token_queue().await.expect("execute token Safe queue");

    // Then the queue and pending bill retain the ERC-20 identity.
    assert_eq!(hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&hash).await;
    let repeat_error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(repeat_error.contains("already been executed"), "{repeat_error}");
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_service_retries_rejected_broadcast_once() {
    // Given a signed token queue, with an RPC that rejects the first broadcast.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        ("eth_sendRawTransaction", "__rpc_rejected__"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create signed token queue");

    // When the first broadcast fails, the queue stays ready without a bill.
    let error = scenario.when_execute_token_queue().await.unwrap_err();
    assert!(error.contains("transaction rejected by fake RPC"), "{error}");
    scenario.then_native_queue_remains_ready().await;

    // When retried, the accepted transaction creates one durable bill.
    let hash = scenario.when_execute_token_queue().await.expect("retry token Safe queue");
    assert_eq!(hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_token_queue_execution_persisted(&hash).await;
    let calls = rpc.finish();
    assert_eq!(calls.iter().filter(|call| call.as_str() == "eth_sendRawTransaction").count(), 2);
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_service_execution_gas_shortage_keeps_queue_ready() {
    // Given a signed ERC-20 queue but no initiator balance for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;
    scenario.when_create_token_queue().await.expect("create signed token queue");

    // When execution checks gas funding.
    let error = scenario.when_execute_token_queue().await.unwrap_err();

    // Then there is no broadcast or bill, and the queue can be retried.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_service_creates_signed_queue() {
    // Given an on-chain Polygon Safe holding the test ERC-20 token.
    let rpc = FakeEvmRpc::new(&[
        ("eth_call", "0x989680"),
        ("eth_call", "0x989680"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When the public Service creates a token transfer proposal.
    let hash = scenario.when_create_token_queue().await.expect("create token Safe queue");

    // Then the token payload and initial signature are durable.
    assert_eq!(hash, format!("0x{}", "aa".repeat(32)));
    scenario.then_token_queue_is_persisted(&hash).await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call", "eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn polygon_token_safe_service_insufficient_balance_leaves_no_queue() {
    // Given an on-chain Polygon Safe with no spendable test ERC-20 tokens.
    let rpc = FakeEvmRpc::new(&[("eth_call", "0x0"), ("eth_call", "0x0")]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_token_safe().await;

    // When the public Service attempts the token transfer proposal.
    let error = scenario.when_create_token_queue().await.unwrap_err();

    // Then it rejects the transfer before Safe nonce lookup or queue creation.
    assert!(error.contains("token balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_service_executes_signed_queue() {
    // Given an on-chain Safe with a signed native queue ready to execute.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create signed queue");

    // When the public Service broadcasts the threshold-signed transaction.
    let hash = scenario.when_execute_native_queue().await.expect("execute Safe queue");

    // Then the queue and pending bill share the returned chain transaction hash.
    assert_eq!(hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_two_of_two_waits_for_second_signature() {
    // Given an on-chain 2-of-2 Safe with only one local signer.
    let mut responses = polygon_native_broadcast_responses();
    responses.truncate(3);
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.given_two_of_two_safe(false).await;

    // When the local owner creates a signed transfer proposal.
    scenario.when_create_native_queue().await.expect("create proposal");

    // Then one signature cannot broadcast and no chain hash is recorded.
    scenario.then_native_queue_has_signatures(1, MultisigQueueStatus::HasSignature).await;
    assert!(scenario.when_execute_native_queue().await.is_err());
    scenario.then_native_queue_has_signatures(1, MultisigQueueStatus::HasSignature).await;
    assert!(!rpc.finish().contains(&"eth_sendRawTransaction".to_string()));
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_two_of_two_broadcasts_with_both_signatures() {
    // Given an on-chain 2-of-2 Safe with both owners held locally.
    let rpc = FakeEvmRpc::new_json(polygon_native_broadcast_responses());
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.given_two_of_two_safe(true).await;

    // When both owners sign and the public Service executes the proposal.
    scenario.when_create_native_queue().await.expect("create signed proposal");
    scenario.then_native_queue_has_signatures(2, MultisigQueueStatus::PendingExecution).await;
    let hash = scenario.when_execute_native_queue().await.expect("broadcast 2-of-2 Safe transfer");

    // Then the broadcast hash is stored with the queue and bill.
    assert_eq!(hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_service_execution_gas_shortage_keeps_queue_ready() {
    // Given a signed Polygon native queue but an unfunded initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create signed queue");

    // When execution checks the initiator's gas balance.
    let error = scenario.when_execute_native_queue().await.unwrap_err();

    // Then no transaction is broadcast and the queue remains ready.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish(), ["eth_getBalance", "eth_call", "eth_call", "eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_service_creates_signed_queue() {
    // Given an on-chain Polygon Safe with a local signer and native asset.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When the public Service creates a native transfer queue.
    let hash = scenario.when_create_native_queue().await.expect("create Safe queue");

    // Then the hash, raw payload, and initial signature persist together.
    assert_eq!(hash, format!("0x{}", "aa".repeat(32)));
    scenario.then_native_queue_is_persisted(&hash).await;
    assert_eq!(rpc.finish(), ["eth_getBalance", "eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_service_insufficient_balance_leaves_no_queue() {
    // Given an on-chain Polygon Safe with no spendable native balance.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When the public Service attempts to create the transfer queue.
    let error = scenario.when_create_native_queue().await.unwrap_err();

    // Then it rejects the transfer before reading nonce or writing the queue.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_proposal_signature_recovers_signer() {
    // Given a native Polygon proposal built from the Safe nonce and transaction hash.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_failed_deployment_with_address().await;
    let (hash, payload) = scenario.when_build_native_proposal("0.1").await.unwrap();

    // When the adapter signs that proposal with an ephemeral test key.
    let (signature, public_key) = scenario.when_sign_native_proposal(&payload).await.unwrap();

    // Then the signature recovers the signer for the proposal hash without another RPC.
    let bytes = hex::decode(signature).expect("hex signature");
    assert_eq!(bytes.len(), 65);
    let recovery_id = secp256k1::ecdsa::RecoveryId::from_i32(i32::from(bytes[64]) - 27).unwrap();
    let recoverable =
        secp256k1::ecdsa::RecoverableSignature::from_compact(&bytes[..64], recovery_id).unwrap();
    let digest: [u8; 32] = hex::decode(hash.trim_start_matches("0x")).unwrap().try_into().unwrap();
    let message = secp256k1::Message::from_digest_slice(&digest).unwrap();
    let recovered = secp256k1::Secp256k1::new().recover_ecdsa(&message, &recoverable).unwrap();
    assert_eq!(recovered.serialize_uncompressed().as_slice(), public_key);
    assert_eq!(rpc.finish(), ["eth_getBalance", "eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_proposal_rejects_malformed_signature_payload() {
    // Given a Polygon Safe and malformed proposal data.
    let rpc = FakeEvmRpc::new(&[]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;

    // When signing is attempted, decoding fails before any RPC.
    assert!(scenario.when_sign_native_proposal("not-a-safe-payload").await.is_err());
    assert!(rpc.finish().is_empty());
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_proposal_builds_signable_payload() {
    // Given a funded Polygon Safe and its local fake RPC.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_failed_deployment_with_address().await;

    // When a native transfer proposal is built.
    let (hash, payload) = scenario.when_build_native_proposal("0.1").await.unwrap();

    // Then its hash and signable payload are returned after nonce lookup.
    assert_eq!(hash, format!("0x{}", "aa".repeat(32)));
    let signable: MultisigPayloadOpt = payload.parse().expect("decode Safe proposal payload");
    assert_eq!(signable.sign_message, hash);
    assert!(signable.input_data.starts_with("0x"));
    assert!(signable.input_data.len() > 100);
    assert_eq!(rpc.finish(), ["eth_getBalance", "eth_call", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn polygon_native_safe_proposal_rejects_insufficient_balance() {
    // Given an unfunded Polygon Safe.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_failed_deployment_with_address().await;

    // When a native transfer proposal is built.
    let error = scenario.when_build_native_proposal("0.1").await.unwrap_err();

    // Then the adapter does not query the Safe nonce or transaction hash.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn polygon_paid_service_broadcast_persists_hash_and_pending_status() {
    // Given a paid Polygon Safe and a Core wallet containing its initiator signer.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;

    // When the Service deploys through the signed adapter path.
    scenario.when_service_deploys().await.expect("paid Safe deployment");

    // Then the broadcast hash and pending state are durable.
    scenario.then_service_deployment_pending(&format!("0x{}", "aa".repeat(32))).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
    scenario.then_stop_syncing().await;
}

#[tokio::test]
#[serial]
async fn polygon_paid_service_insufficient_balance_keeps_deployment_retryable() {
    // Given a paid Polygon Safe with a signable Core wallet but no gas balance.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;

    // When the Service tries to deploy, the adapter rejects gas funding.
    let error = scenario.when_service_deploys().await.unwrap_err();

    // Then the saved Safe identity remains retryable without a broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(
        rpc.finish(),
        [
            "eth_chainId",
            "eth_getCode",
            "eth_getCode",
            "eth_chainId",
            "eth_getCode",
            "eth_getCode",
            "eth_getBalance"
        ]
    );
}

#[tokio::test]
#[serial]
async fn avalanche_paid_service_broadcast_persists_hash_and_pending_status() {
    // Given a paid Avalanche Safe and a local Core wallet initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0xa86a"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;

    // When the Service broadcasts the signed Safe deployment.
    scenario.when_service_deploys().await.expect("Avalanche paid Safe deployment");

    // Then the hash and pending state are durable.
    scenario.then_service_deployment_pending(&format!("0x{}", "aa".repeat(32))).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
    scenario.then_stop_syncing().await;
}

#[tokio::test]
#[serial]
async fn avalanche_paid_service_insufficient_balance_keeps_deployment_retryable() {
    // Given a paid Avalanche Safe with no initiator gas balance.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;

    // When deployment checks the initiator's fee balance.
    let error = scenario.when_service_deploys().await.unwrap_err();

    // Then no transaction is broadcast and the stored identity stays retryable.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn arbitrum_paid_service_broadcast_persists_hash_and_pending_status() {
    // Given a paid Arbitrum Safe and a local Core wallet initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa4b1"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0xa4b1"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0xa4b1"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When the public Service broadcasts the signed Safe deployment.
    scenario.when_service_deploys().await.expect("Arbitrum paid Safe deployment");

    // Then the chain hash and pending deployment status are durable.
    scenario.then_service_deployment_pending(&format!("0x{}", "aa".repeat(32))).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
    scenario.then_stop_syncing().await;
}

#[tokio::test]
#[serial]
async fn arbitrum_paid_service_insufficient_balance_keeps_deployment_retryable() {
    // Given a paid Arbitrum Safe with no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa4b1"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0xa4b1"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When deployment checks the initiator's gas balance.
    let error = scenario.when_service_deploys().await.unwrap_err();

    // Then no transaction is broadcast and the Safe identity remains retryable.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn base_paid_service_broadcast_persists_hash_and_pending_status() {
    // Given a paid Base Safe and a local Core wallet initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x2105"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x2105"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x2105"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When public Service broadcasts the signed Safe deployment.
    scenario.when_service_deploys().await.expect("Base paid Safe deployment");

    // Then the chain hash and pending deployment status are durable.
    scenario.then_service_deployment_pending(&format!("0x{}", "aa".repeat(32))).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
    scenario.then_stop_syncing().await;
}

#[tokio::test]
#[serial]
async fn base_paid_service_insufficient_balance_keeps_deployment_retryable() {
    // Given a paid Base Safe without initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x2105"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x2105"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When deployment checks the initiator's gas balance.
    let error = scenario.when_service_deploys().await.unwrap_err();

    // Then no transaction is broadcast and the Safe identity remains retryable.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn scroll_paid_service_broadcast_persists_hash_and_pending_status() {
    // Given a paid Scroll Safe and a local Core wallet initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x82750"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x82750"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x82750"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When public Service broadcasts the signed Safe deployment.
    scenario.when_service_deploys().await.expect("Scroll paid Safe deployment");

    // Then the chain hash and pending deployment status are durable.
    scenario.then_service_deployment_pending(&format!("0x{}", "aa".repeat(32))).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
    scenario.then_stop_syncing().await;
}

#[tokio::test]
#[serial]
async fn scroll_paid_service_insufficient_balance_keeps_deployment_retryable() {
    // Given a paid Scroll Safe without initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x82750"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_chainId", "0x82750"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_paid_deployment().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When deployment checks the initiator's gas balance.
    let error = scenario.when_service_deploys().await.unwrap_err();

    // Then no transaction is broadcast and the Safe identity remains retryable.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Scroll Safe with a local signer and ETH asset.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x82750"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When public Service creates and executes the signed ETH queue.
    let proposal_hash = scenario.when_create_native_queue().await.expect("create ETH Safe queue");
    scenario.then_native_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_native_queue().await.expect("execute ETH Safe queue");

    // Then the chain hash and native pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&chain_hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_service_insufficient_eth_leaves_no_queue() {
    // Given an on-chain Scroll Safe without spendable ETH.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When queue creation checks the Safe's native ETH balance.
    let error = scenario.when_create_native_queue().await.unwrap_err();

    // Then no nonce is queried and no queue is saved.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn scroll_native_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Scroll ETH queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH Safe queue");

    // When execution checks the initiator's gas balance.
    let error = scenario.when_execute_native_queue().await.unwrap_err();

    // Then the signed queue remains ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn base_native_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Base Safe with a local signer and ETH asset.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x2105"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When public Service creates and executes the signed ETH queue.
    let proposal_hash = scenario.when_create_native_queue().await.expect("create ETH Safe queue");
    scenario.then_native_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_native_queue().await.expect("execute ETH Safe queue");

    // Then the chain hash and native pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&chain_hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn base_native_safe_service_insufficient_eth_leaves_no_queue() {
    // Given an on-chain Base Safe without spendable ETH.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When queue creation checks the Safe's native ETH balance.
    let error = scenario.when_create_native_queue().await.unwrap_err();

    // Then no nonce is queried and no queue is saved.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn base_native_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Base ETH queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH Safe queue");

    // When execution checks the initiator's gas balance.
    let error = scenario.when_execute_native_queue().await.unwrap_err();

    // Then the signed queue remains ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Arbitrum Safe with a local signer and ETH asset.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0xa4b1"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.then_native_coin_symbol("ETH").await;

    // When public Service creates and executes the signed ETH queue.
    let proposal_hash = scenario.when_create_native_queue().await.expect("create ETH Safe queue");
    scenario.then_native_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_native_queue().await.expect("execute ETH Safe queue");

    // Then the chain hash and native pending bill are durable.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&chain_hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_service_insufficient_eth_leaves_no_queue() {
    // Given an on-chain Arbitrum Safe without spendable ETH.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When queue creation checks the Safe's native ETH balance.
    let error = scenario.when_create_native_queue().await.unwrap_err();

    // Then no nonce is queried and no queue is saved.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn arbitrum_native_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed Arbitrum ETH queue and no initiator ETH for gas.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create ETH Safe queue");

    // When execution checks the initiator's gas balance.
    let error = scenario.when_execute_native_queue().await.unwrap_err();

    // Then the signed queue remains ready without a bill or broadcast.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_service_creates_and_executes_signed_queue() {
    // Given an on-chain Avalanche Safe with a local signer and AVAX asset.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0xa86a"),
        (
            "eth_sendRawTransaction",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When the public Service creates and executes the signed AVAX queue.
    let proposal_hash = scenario.when_create_native_queue().await.expect("create AVAX Safe queue");
    scenario.then_native_queue_is_persisted(&proposal_hash).await;
    let chain_hash = scenario.when_execute_native_queue().await.expect("execute AVAX Safe queue");

    // Then the broadcast hash and native bill are persisted for confirmation.
    assert_eq!(chain_hash, format!("0x{}", "bb".repeat(32)));
    scenario.then_native_queue_execution_persisted(&chain_hash).await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_sendRawTransaction"));
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_service_insufficient_avax_leaves_no_queue() {
    // Given an on-chain Avalanche Safe without spendable AVAX.
    let rpc = FakeEvmRpc::new(&[("eth_getBalance", "0x0")]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;

    // When queue creation checks the Safe's native balance.
    let error = scenario.when_create_native_queue().await.unwrap_err();

    // Then no nonce is queried and no queue is saved.
    assert!(error.contains("main coin balance is insufficient"), "{error}");
    scenario.then_no_native_queue_exists().await;
    assert_eq!(rpc.finish(), ["eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn avalanche_native_safe_service_insufficient_gas_keeps_queue_ready() {
    // Given a signed AVAX queue but an unfunded initiator.
    let rpc = FakeEvmRpc::new(&[
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_call", "0x0"),
        ("eth_call", "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;
    scenario.given_signable_onchain_native_safe().await;
    scenario.when_create_native_queue().await.expect("create AVAX Safe queue");

    // When execution checks gas funding, it stops before broadcast.
    let error = scenario.when_execute_native_queue().await.unwrap_err();
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_native_queue_remains_ready().await;
    assert_eq!(rpc.finish().last().map(String::as_str), Some("eth_getBalance"));
}

#[tokio::test]
#[serial]
async fn polygon_paid_safe_adapter_signs_and_broadcasts_with_fake_rpc() {
    // Given a paid Polygon Safe with a saved address and sufficient gas balance.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0xde0b6b3a7640000"),
        ("eth_getTransactionCount", "0x0"),
        ("eth_chainId", "0x89"),
        (
            "eth_sendRawTransaction",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_failed_deployment_with_address().await;

    // When the adapter signs and submits a deployment transaction.
    let (hash, _) = scenario.when_broadcast_deployment_with_test_signer().await.unwrap();

    // Then the RPC returned hash is passed back and adapter calls do not mutate account state.
    assert_eq!(hash, format!("0x{}", "aa".repeat(32)));
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(
        rpc.finish(),
        [
            "eth_chainId",
            "eth_getCode",
            "eth_getCode",
            "eth_getBalance",
            "eth_getTransactionCount",
            "eth_chainId",
            "eth_sendRawTransaction"
        ]
    );
}

#[tokio::test]
#[serial]
async fn polygon_paid_safe_adapter_rejects_insufficient_gas_before_broadcast() {
    // Given a paid Polygon Safe with too little gas balance.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_getBalance", "0x0"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_failed_deployment_with_address().await;

    // When the adapter tries to deploy.
    let error = scenario.when_broadcast_deployment_with_test_signer().await.unwrap_err();

    // Then it stops before nonce lookup and broadcast, leaving local state unchanged.
    assert!(error.contains("Insufficient balance for fees"), "{error}");
    scenario.then_stored_address_is_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode", "eth_getBalance"]);
}

#[tokio::test]
#[serial]
async fn evm_safe_pending_receipt_does_not_rebroadcast_deployment() {
    for (chain_code, chain_name, chain_id) in [
        ("eth", "Ethereum", None),
        ("bnb", "BNB", None),
        ("polygon", "Polygon", Some("0x89")),
        ("avalanche", "Avalanche", Some("0xa86a")),
        ("arbitrum", "Arbitrum", Some("0xa4b1")),
        ("base", "Base", Some("0x2105")),
        ("scroll", "Scroll", Some("0x82750")),
    ] {
        // Given an already broadcast Safe deployment with no available receipt.
        let mut responses = Vec::new();
        if let Some(chain_id) = chain_id {
            responses.extend([
                ("eth_chainId", chain_id),
                ("eth_getCode", "0x6001"),
                ("eth_getCode", "0x6002"),
            ]);
        }
        responses.push(("eth_getTransactionReceipt", "unavailable"));
        let rpc = FakeEvmRpc::new(&responses);
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;
        scenario.given_paid_pending_deployment_with_hash().await;

        // When the user retries before a receipt can be read.
        scenario.when_retry_pending_deployment().await.expect("retain pending deployment");

        // Then the original transaction and Safe identity stay pending with no broadcast.
        scenario.then_pending_deployment_is_unchanged().await;
        scenario.then_stop_syncing().await;
        let methods = rpc.finish();
        assert_eq!(methods.last().map(String::as_str), Some("eth_getTransactionReceipt"));
        assert_eq!(methods.len(), if chain_id.is_some() { 4 } else { 1 });
    }
}

#[tokio::test]
#[serial]
async fn legacy_evm_safe_paid_retry_reuses_stored_address() {
    for (chain_code, chain_name) in [("eth", "Ethereum"), ("bnb", "BNB")] {
        // Given a paid Safe account whose previous attempt persisted an address and salt.
        let rpc = FakeEvmRpc::new(&[]);
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;
        scenario.given_paid_failed_deployment_with_address().await;

        // When the retry reaches the unavailable initiator signer.
        let error = scenario.when_retry_deployment_without_signer().await;

        // Then no new address is derived and the original Safe identity remains.
        assert!(error.contains("Account not found"), "{chain_code}: {error}");
        scenario.then_stored_address_is_unchanged().await;
        assert!(rpc.finish().is_empty());
    }
}

#[tokio::test]
#[serial]
async fn new_evm_chains_paid_retry_reuses_stored_safe_address_before_signer_lookup() {
    for (chain_code, chain_name, chain_id) in [
        ("polygon", "Polygon", "0x89"),
        ("avalanche", "Avalanche", "0xa86a"),
        ("arbitrum", "Arbitrum", "0xa4b1"),
        ("base", "Base", "0x2105"),
        ("scroll", "Scroll", "0x82750"),
    ] {
        // Given a paid Safe account whose previous deployment saved its address.
        let rpc = FakeEvmRpc::new(&[
            ("eth_chainId", chain_id),
            ("eth_getCode", "0x6001"),
            ("eth_getCode", "0x6002"),
        ]);
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;
        scenario.given_paid_failed_deployment_with_address().await;

        // When retrying without an available initiator signer.
        let error = scenario.when_retry_deployment_without_signer().await;

        // Then derivation is skipped and the stored Safe identity is retained.
        assert!(error.contains("Account not found"), "{chain_code}: {error}");
        scenario.then_stored_address_is_unchanged().await;
        assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
    }
}

fn polygon_deployment_receipt(success: bool) -> Vec<(&'static str, serde_json::Value)> {
    let hash = format!("0x{}", "11".repeat(32));
    let zero_hash = format!("0x{}", "00".repeat(32));
    let zero_bloom = format!("0x{}", "00".repeat(256));
    let receipt = serde_json::json!({
        "type": "0x0", "status": if success { "0x1" } else { "0x0" },
        "cumulativeGasUsed": "0x5208", "logsBloom": zero_bloom,
        "logs": [], "transactionHash": hash, "blockHash": hash,
        "blockNumber": "0x1", "transactionIndex": "0x0",
        "gasUsed": "0x5208", "effectiveGasPrice": "0x1",
        "from": "0x1111111111111111111111111111111111111111",
        "to": "0x2222222222222222222222222222222222222222",
        "contractAddress": null
    });
    let block = serde_json::json!({
        "hash": hash, "parentHash": zero_hash, "sha3Uncles": zero_hash,
        "miner": "0x0000000000000000000000000000000000000000",
        "stateRoot": zero_hash, "transactionsRoot": zero_hash,
        "receiptsRoot": zero_hash, "logsBloom": format!("0x{}", "00".repeat(256)),
        "difficulty": "0x0", "number": "0x1", "gasLimit": "0x100000",
        "gasUsed": "0x5208", "timestamp": "0x1", "extraData": "0x",
        "transactions": [], "uncles": []
    });
    vec![("eth_getTransactionReceipt", receipt), ("eth_getBlockByHash", block)]
}

fn polygon_execution_receipt(success: bool) -> Vec<(&'static str, serde_json::Value)> {
    let mut responses = polygon_deployment_receipt(success);
    responses[0].1["transactionHash"] = serde_json::json!(format!("0x{}", "bb".repeat(32)));
    responses
}

#[tokio::test]
#[serial]
async fn polygon_failed_receipt_allows_retry_to_reach_signer_lookup() {
    // Given a paid deployment with a confirmed failed receipt.
    let mut responses = vec![
        ("eth_chainId", serde_json::json!("0x89")),
        ("eth_getCode", serde_json::json!("0x6001")),
        ("eth_getCode", serde_json::json!("0x6002")),
    ];
    responses.extend(polygon_deployment_receipt(false));
    let rpc = FakeEvmRpc::new_json(responses);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_paid_pending_deployment_with_hash().await;
    scenario.then_stop_syncing().await;

    // When retrying without a signer after the failed receipt is read.
    let error = scenario.when_retry_pending_deployment().await.unwrap_err();

    // Then retry reaches signer lookup, retaining the original Safe identity.
    assert!(error.contains("Account not found"), "{error}");
    scenario.then_failed_deployment_with_hash_is_unchanged().await;
    assert_eq!(
        rpc.finish(),
        [
            "eth_chainId",
            "eth_getCode",
            "eth_getCode",
            "eth_getTransactionReceipt",
            "eth_getBlockByHash"
        ]
    );
}

#[tokio::test]
#[serial]
async fn polygon_successful_deployment_receipt_marks_account_on_chain() {
    let rpc = FakeEvmRpc::new_json(polygon_deployment_receipt(true));
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_pending_deployment().await;
    assert_eq!(scenario.when_sync_deployment().await, 3);
    assert_eq!(rpc.finish(), ["eth_getTransactionReceipt", "eth_getBlockByHash"]);
}

#[tokio::test]
#[serial]
async fn polygon_failed_deployment_receipt_marks_account_failed() {
    let rpc = FakeEvmRpc::new_json(polygon_deployment_receipt(false));
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_pending_deployment().await;
    assert_eq!(scenario.when_sync_deployment().await, 4);
    assert_eq!(rpc.finish(), ["eth_getTransactionReceipt", "eth_getBlockByHash"]);
}

#[tokio::test]
#[serial]
async fn new_evm_chains_successful_deployment_receipt_marks_account_on_chain() {
    for (chain_code, chain_name) in [
        ("avalanche", "Avalanche"),
        ("arbitrum", "Arbitrum"),
        ("base", "Base"),
        ("scroll", "Scroll"),
    ] {
        // Given a pending Safe deployment with a successful chain receipt.
        let rpc = FakeEvmRpc::new_json(polygon_deployment_receipt(true));
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;
        scenario.given_pending_deployment().await;

        // When public account listing synchronizes the deployment receipt.
        let status = scenario.when_sync_deployment().await;

        // Then the account is on chain and no extra RPC is made.
        assert_eq!(status, 3, "{chain_code}");
        assert_eq!(rpc.finish(), ["eth_getTransactionReceipt", "eth_getBlockByHash"]);
    }
}

#[tokio::test]
#[serial]
async fn new_evm_chains_failed_deployment_receipt_marks_account_failed() {
    for (chain_code, chain_name) in [
        ("avalanche", "Avalanche"),
        ("arbitrum", "Arbitrum"),
        ("base", "Base"),
        ("scroll", "Scroll"),
    ] {
        // Given a pending Safe deployment with an explicitly failed receipt.
        let rpc = FakeEvmRpc::new_json(polygon_deployment_receipt(false));
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;
        scenario.given_pending_deployment().await;

        // When public account listing synchronizes the deployment receipt.
        let status = scenario.when_sync_deployment().await;

        // Then the account is failed and no extra RPC is made.
        assert_eq!(status, 4, "{chain_code}");
        assert_eq!(rpc.finish(), ["eth_getTransactionReceipt", "eth_getBlockByHash"]);
    }
}

#[tokio::test]
#[serial]
async fn polygon_pending_deployment_queries_safe_receipt_and_keeps_status_on_rpc_error() {
    // Given a pending Polygon Safe deployment and an unavailable receipt response.
    let rpc = FakeEvmRpc::new(&[("eth_getTransactionReceipt", "unavailable")]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;
    scenario.given_pending_deployment().await;

    // When the account list synchronizes deployment status.
    let status = scenario.when_sync_deployment().await;

    // Then it queries through the Safe adapter and preserves the pending DB status.
    assert_eq!(status, 5);
    scenario.then_stop_syncing().await;
    assert_eq!(rpc.finish(), ["eth_getTransactionReceipt"]);
}

#[tokio::test]
#[serial]
async fn wrong_polygon_rpc_stops_before_service_fee_and_keeps_account_unchanged() {
    // Given an unpaid Polygon multisig account and an RPC for Ethereum.
    let rpc = FakeEvmRpc::new(&[("eth_chainId", "0x1")]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;

    // When deployment starts, the Safe preflight rejects the RPC.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then no service fee is attempted and the account's durable state is unchanged.
    assert!(error.contains("expected 137"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId"]);
}

#[tokio::test]
#[serial]
async fn valid_polygon_rpc_reaches_service_fee_payer_gate_without_mutating_account() {
    // Given an unpaid Polygon multisig account with valid Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x89"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
    ]);
    let scenario = EvmDeployScenario::given("polygon", "Polygon", &rpc.url).await;

    // When deployment starts without a service fee payer.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then preflight completed before the payer validation, with no account writes.
    assert!(error.contains("PayerNeed"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}

#[tokio::test]
#[serial]
async fn wrong_avalanche_rpc_stops_before_service_fee_and_keeps_account_unchanged() {
    // Given an unpaid Avalanche multisig account and an RPC for Ethereum.
    let rpc = FakeEvmRpc::new(&[("eth_chainId", "0x1")]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;

    // When deployment starts, the Safe preflight rejects the RPC.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then no service fee is attempted and the account's durable state is unchanged.
    assert!(error.contains("expected 43114"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId"]);
}

#[tokio::test]
#[serial]
async fn valid_avalanche_rpc_reaches_service_fee_payer_gate_without_mutating_account() {
    // Given an unpaid Avalanche multisig account with valid Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;

    // When deployment starts without a service fee payer.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then preflight completed before the payer validation, with no account writes.
    assert!(error.contains("PayerNeed"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}

#[tokio::test]
#[serial]
async fn avalanche_address_derivation_checks_safe_before_factory_call() {
    // Given an Avalanche account and an RPC with both Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
        ("eth_call", "0x0000000000000000000000002222222222222222222222222222222222222222"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;

    // When the adapter derives the address from the Safe factory.
    let (address, salt) = scenario.when_derive_address().await.unwrap();

    // Then the factory call follows preflight and the account remains unchanged.
    assert_eq!(address.to_lowercase(), "0x2222222222222222222222222222222222222222");
    assert!(!salt.is_empty());
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode", "eth_call"]);
}

#[tokio::test]
#[serial]
async fn avalanche_address_derivation_stops_when_safe_singleton_is_missing() {
    // Given an Avalanche account with no Safe singleton code at the configured RPC.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa86a"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x"),
    ]);
    let scenario = EvmDeployScenario::given("avalanche", "Avalanche", &rpc.url).await;

    // When address derivation begins, preflight rejects the missing contract.
    let error = scenario.when_derive_address().await.unwrap_err();

    // Then no factory call or account mutation occurs.
    assert!(error.contains("Safe singleton"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}

#[tokio::test]
#[serial]
async fn eth_l2_safe_address_derivation_checks_contracts_before_factory_call() {
    for (chain_code, chain_name, chain_id) in [
        ("arbitrum", "Arbitrum", "0xa4b1"),
        ("base", "Base", "0x2105"),
        ("scroll", "Scroll", "0x82750"),
    ] {
        // Given a chain RPC with both canonical Safe contracts deployed.
        let rpc = FakeEvmRpc::new(&[
            ("eth_chainId", chain_id),
            ("eth_getCode", "0x6001"),
            ("eth_getCode", "0x6002"),
            ("eth_call", "0x0000000000000000000000002222222222222222222222222222222222222222"),
        ]);
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;

        // When the adapter derives the deterministic Safe address.
        let (address, salt) = scenario
            .when_derive_address()
            .await
            .unwrap_or_else(|error| panic!("{chain_code}: {error}"));

        // Then preflight precedes the factory call and the account is unchanged.
        assert_eq!(address.to_lowercase(), "0x2222222222222222222222222222222222222222");
        assert!(!salt.is_empty(), "{chain_code}");
        scenario.then_account_unchanged().await;
        assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode", "eth_call"]);
    }
}

#[tokio::test]
#[serial]
async fn eth_l2_safe_address_derivation_stops_without_singleton() {
    for (chain_code, chain_name, chain_id) in [
        ("arbitrum", "Arbitrum", "0xa4b1"),
        ("base", "Base", "0x2105"),
        ("scroll", "Scroll", "0x82750"),
    ] {
        // Given a chain RPC where the Safe singleton has no bytecode.
        let rpc = FakeEvmRpc::new(&[
            ("eth_chainId", chain_id),
            ("eth_getCode", "0x6001"),
            ("eth_getCode", "0x"),
        ]);
        let scenario = EvmDeployScenario::given(chain_code, chain_name, &rpc.url).await;

        // When address derivation performs contract preflight.
        let error = scenario.when_derive_address().await.unwrap_err();

        // Then no factory call or account mutation occurs.
        assert!(error.contains("Safe singleton"), "{chain_code}: {error}");
        scenario.then_account_unchanged().await;
        assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
    }
}

#[tokio::test]
#[serial]
async fn wrong_arbitrum_rpc_stops_before_service_fee_and_keeps_account_unchanged() {
    // Given an unpaid Arbitrum multisig account and an RPC for Ethereum.
    let rpc = FakeEvmRpc::new(&[("eth_chainId", "0x1")]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;

    // When deployment starts, Safe preflight rejects the RPC.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then no service fee is attempted and durable account state is unchanged.
    assert!(error.contains("expected 42161"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId"]);
}

#[tokio::test]
#[serial]
async fn valid_arbitrum_rpc_reaches_service_fee_payer_gate_without_mutating_account() {
    // Given an unpaid Arbitrum multisig account and both Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0xa4b1"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
    ]);
    let scenario = EvmDeployScenario::given("arbitrum", "Arbitrum", &rpc.url).await;

    // When deployment starts without a service fee payer.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then preflight completed before payer validation and no account write occurred.
    assert!(error.contains("PayerNeed"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}

#[tokio::test]
#[serial]
async fn wrong_base_rpc_stops_before_service_fee_and_keeps_account_unchanged() {
    // Given an unpaid Base multisig account and an RPC for Ethereum.
    let rpc = FakeEvmRpc::new(&[("eth_chainId", "0x1")]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;

    // When deployment starts, Safe preflight rejects the RPC.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then no service fee is attempted and durable account state is unchanged.
    assert!(error.contains("expected 8453"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId"]);
}

#[tokio::test]
#[serial]
async fn valid_base_rpc_reaches_service_fee_payer_gate_without_mutating_account() {
    // Given an unpaid Base multisig account and both Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x2105"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
    ]);
    let scenario = EvmDeployScenario::given("base", "Base", &rpc.url).await;

    // When deployment starts without a service fee payer.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then preflight completed before payer validation and no account write occurred.
    assert!(error.contains("PayerNeed"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}

#[tokio::test]
#[serial]
async fn wrong_scroll_rpc_stops_before_service_fee_and_keeps_account_unchanged() {
    // Given an unpaid Scroll multisig account and an RPC for Ethereum.
    let rpc = FakeEvmRpc::new(&[("eth_chainId", "0x1")]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;

    // When deployment starts, Safe preflight rejects the RPC.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then no service fee is attempted and durable account state is unchanged.
    assert!(error.contains("expected 534352"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId"]);
}

#[tokio::test]
#[serial]
async fn valid_scroll_rpc_reaches_service_fee_payer_gate_without_mutating_account() {
    // Given an unpaid Scroll multisig account and both Safe contracts.
    let rpc = FakeEvmRpc::new(&[
        ("eth_chainId", "0x82750"),
        ("eth_getCode", "0x6001"),
        ("eth_getCode", "0x6002"),
    ]);
    let scenario = EvmDeployScenario::given("scroll", "Scroll", &rpc.url).await;

    // When deployment starts without a service fee payer.
    let error = scenario.when_deploy_without_payer().await.unwrap_err();

    // Then preflight completed before payer validation and no account write occurred.
    assert!(error.contains("PayerNeed"), "{error}");
    scenario.then_account_unchanged().await;
    assert_eq!(rpc.finish(), ["eth_chainId", "eth_getCode", "eth_getCode"]);
}
