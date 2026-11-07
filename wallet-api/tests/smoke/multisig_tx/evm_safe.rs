//! Manual, read-only Safe address derivation against a configured mainnet RPC.
//! Run one chain at a time with its `SAFE_*_RPC_URL` environment variable.

use wallet_api::domain::chain::adapter::MultisigAdapter;
use wallet_database::entities::{
    chain::ChainWithNode,
    multisig_account::MultisigAccountEntity,
    multisig_member::{MultisigMemberEntities, MultisigMemberEntity},
};
use wallet_types::chain::chain::ChainCode;

async fn check_safe_address(chain_code: &str, rpc_env: &str) {
    let rpc_url = std::env::var(rpc_env).unwrap_or_else(|_| panic!("set {rpc_env} before running"));
    assert!(
        rpc_url.starts_with("https://") || rpc_url.starts_with("http://"),
        "{rpc_env} must be an HTTP(S) RPC URL"
    );

    let chain = ChainCode::try_from(chain_code).expect("supported Safe chain");
    let node = ChainWithNode {
        chain_code: chain_code.to_string(),
        rpc_url,
        network: "mainnet".to_string(),
        ..Default::default()
    };
    let adapter = MultisigAdapter::new(chain, node, None)
        .unwrap_or_else(|_| panic!("{chain_code} Safe adapter initialization failed"));
    let initiator = "0x1111111111111111111111111111111111111111";
    let now = sqlx::types::chrono::Utc::now();
    let account = MultisigAccountEntity {
        id: "manual-safe-derivation".to_string(),
        name: "Manual Safe derivation".to_string(),
        initiator_addr: initiator.to_string(),
        address: String::new(),
        address_type: String::new(),
        authority_addr: String::new(),
        status: 2,
        pay_status: 0,
        owner: 1,
        chain_code: chain_code.to_string(),
        threshold: 1,
        member_num: 1,
        salt: String::new(),
        deploy_hash: String::new(),
        fee_hash: String::new(),
        fee_chain: String::new(),
        is_del: 0,
        created_at: now,
        updated_at: None,
    };
    let members = MultisigMemberEntities(vec![MultisigMemberEntity {
        account_id: account.id.clone(),
        address: initiator.to_string(),
        name: "owner".to_string(),
        confirmed: 1,
        created_at: now,
        ..Default::default()
    }]);

    // This SDK entry point checks chain ID and Safe bytecode, then calls the factory.
    // It does not sign, send, or persist a transaction.
    let result = adapter
        .multisig_address(&account, &members)
        .await
        .unwrap_or_else(|_| panic!("{chain_code} Safe address derivation failed"));
    let address: alloy::primitives::Address =
        result.multisig_address.parse().expect("factory returned an EVM address");
    assert_ne!(address, alloy::primitives::Address::ZERO);
    assert!(!result.salt.is_empty(), "factory returned an empty salt");
}

#[tokio::test]
#[ignore = "requires SAFE_POLYGON_RPC_URL; read-only mainnet RPC"]
async fn polygon_safe_address_derivation_live() {
    check_safe_address("polygon", "SAFE_POLYGON_RPC_URL").await;
}

#[tokio::test]
#[ignore = "requires SAFE_AVALANCHE_RPC_URL; read-only mainnet RPC"]
async fn avalanche_safe_address_derivation_live() {
    check_safe_address("avalanche", "SAFE_AVALANCHE_RPC_URL").await;
}

#[tokio::test]
#[ignore = "requires SAFE_ARBITRUM_RPC_URL; read-only mainnet RPC"]
async fn arbitrum_safe_address_derivation_live() {
    check_safe_address("arbitrum", "SAFE_ARBITRUM_RPC_URL").await;
}

#[tokio::test]
#[ignore = "requires SAFE_BASE_RPC_URL; read-only mainnet RPC"]
async fn base_safe_address_derivation_live() {
    check_safe_address("base", "SAFE_BASE_RPC_URL").await;
}

#[tokio::test]
#[ignore = "requires SAFE_SCROLL_RPC_URL; read-only mainnet RPC"]
async fn scroll_safe_address_derivation_live() {
    check_safe_address("scroll", "SAFE_SCROLL_RPC_URL").await;
}
