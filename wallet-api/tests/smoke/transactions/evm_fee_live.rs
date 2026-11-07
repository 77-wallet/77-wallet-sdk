//! Manually invoked dev-backend/mainnet-RPC smoke test. Never broadcasts.
//! Included from the private EVM fee module to exercise its exact quote entrypoint.

use super::estimate_transfer_fee;
use crate::request::transaction::BaseTransferReq;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, path::Path, time::Duration};
use wallet_chain_interact::eth::{EthChain, Provider};
use wallet_transport::client::RpcClient;
use wallet_transport_backend::{api::BackendApi, response_vo::coin::TokenCurrency};
use wallet_types::chain::{chain::ChainCode, network::NetworkKind};

#[derive(Deserialize)]
struct DevBackendConfig {
    base_url: String,
    client_id: String,
    aw_sec_id: String,
    aes_key: String,
    aes_iv: String,
}

fn dev_backend() -> (BackendApi, String) {
    let config = if let Ok(base_url) = std::env::var("WALLET_BACKEND_TEST_BASE_URL") {
        DevBackendConfig {
            base_url,
            client_id: std::env::var("WALLET_BACKEND_TEST_CLIENT_ID").unwrap(),
            aw_sec_id: std::env::var("WALLET_BACKEND_TEST_AW_SEC_ID").unwrap(),
            aes_key: std::env::var("WALLET_BACKEND_TEST_AES_KEY").unwrap(),
            aes_iv: std::env::var("WALLET_BACKEND_TEST_AES_IV").unwrap(),
        }
    } else {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wallet-transport-backend/tests/backend_test_config.toml");
        wallet_utils::serde_func::toml_from_str::<DevBackendConfig>(
            &std::fs::read_to_string(path)
                .expect("local ignored dev backend test config is required"),
        )
        .expect("dev backend test config must be valid TOML")
    };
    let client_id = config.client_id.clone();
    let headers = HashMap::from([
        ("clientId".to_owned(), config.client_id),
        ("AW-SEC-ID".to_owned(), config.aw_sec_id),
    ]);
    let cryptor = wallet_utils::cbc::AesCbcCryptor::new(&config.aes_key, &config.aes_iv);
    (BackendApi::new(Some(config.base_url), Some(headers), cryptor).unwrap(), client_id)
}

#[tokio::test]
#[ignore = "manual live smoke: dev backend and mainnet RPC, no signing or broadcast"]
async fn added_evm_chains_quote_with_dev_tracker_and_real_nodes() {
    let (backend, client_id) = dev_backend();
    let rpc_token = backend.rpc_token(&client_id).await.expect("dev RPC token request");
    assert!(!rpc_token.is_empty(), "dev RPC token must not be empty");
    for (code, rpc, symbol, expected_chain_id) in [
        (ChainCode::Polygon, "https://apprpc.77wapp.vip/polygon", "POL", 137),
        (ChainCode::Avalanche, "https://apprpc.77wapp.vip/avalanche", "AVAX", 43_114),
        (ChainCode::Arbitrum, "https://apprpc.77wapp.vip/arbitrum", "ETH", 42_161),
        (ChainCode::Base, "https://apprpc.77wapp.vip/base", "ETH", 8_453),
    ] {
        // Require the backend tracker itself to work; SDK fallback must not mask its failure.
        let tracker = backend.gas_oracle(&code.to_string()).await.unwrap();
        assert!(tracker.propose_gas_price.is_some(), "{code}: missing backend propose tier");

        let override_rpc =
            std::env::var(format!("WALLET_SMOKE_{}_RPC", code.to_string().to_uppercase())).ok();
        let (rpc, headers) = match override_rpc {
            Some(rpc) => (rpc, None),
            None => {
                (rpc.to_owned(), Some(HashMap::from([("token".to_owned(), rpc_token.clone())])))
            }
        };
        let client = RpcClient::new(&rpc, headers, Some(Duration::from_secs(20))).unwrap();
        let provider = Provider::new(client).unwrap();
        assert_eq!(provider.chain_id().await.unwrap(), expected_chain_id, "{code}: wrong RPC");
        let chain = EthChain::new(provider, NetworkKind::Mainnet, code).unwrap();
        // A public funded sender may be supplied if an RPC checks gas affordability in eth_estimateGas.
        // No private key is needed: this test estimates only and never sends a transaction.
        let from =
            std::env::var(format!("WALLET_SMOKE_{}_FEE_FROM", code.to_string().to_uppercase()))
                .unwrap_or_else(|_| "0x000000000000000000000000000000000000dEaD".to_owned());
        let mut req = BaseTransferReq::new(
            &from,
            "0x0000000000000000000000000000000000000002",
            "0",
            &code.to_string(),
            symbol,
        );
        req.with_decimals(18);
        let token_currency =
            TokenCurrency::new(&code.to_string(), symbol, symbol, Some(1.0), None, 1.0, 18);
        let quote = estimate_transfer_fee(&chain, &req, &backend, "USD", token_currency)
            .await
            .unwrap_or_else(|error| panic!("{code}: SDK quote failed: {error:?}"));
        let quote: Value = serde_json::from_str(&quote).unwrap();
        assert_eq!(quote["default"], "propose", "{code}");
        let tiers = quote["data"].as_array().expect("quote must include tiers");
        assert_eq!(tiers.len(), 3, "{code}");
        for (tier, kind) in tiers.iter().zip(["safe", "propose", "fast"]) {
            assert_eq!(tier["type"], kind, "{code}");
            let gas_limit = tier["feeSetting"]["gasLimit"].as_u64().unwrap();
            assert!(gas_limit > 0, "{code}");
            let estimated: f64 = tier["estimateFee"]["amount"].as_str().unwrap().parse().unwrap();
            assert!(estimated.is_finite() && estimated > 0.0, "{code} {kind}");
            if code == ChainCode::Base {
                let max_per_gas: f64 =
                    tier["feeSetting"]["maxFeePerGas"].as_str().unwrap().parse().unwrap();
                let execution_cap = gas_limit as f64 * max_per_gas / 1e18;
                let total_max: f64 = tier["maxFee"]["amount"].as_str().unwrap().parse().unwrap();
                assert!(total_max > execution_cap, "Base {kind}: L1 fee must be included");
            }
        }
    }
}
