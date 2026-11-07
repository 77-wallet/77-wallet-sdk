#![allow(unused_variables)]
pub mod account;
pub mod address_book;
pub mod address_query_state;
pub mod announcement;
pub mod api_account;
pub mod api_asset_sync_state;
pub mod api_assets;
pub mod api_business_notification;
pub mod api_chain;
pub mod api_coin;
pub mod api_collect;
pub mod api_collect_strategy;
pub mod api_collect_strategy_chain_config;
pub mod api_fee;
pub mod api_nonce;
pub mod api_resource_delegation;
pub mod api_resource_gate;
pub mod api_resource_operation;
pub mod api_resource_type;
pub mod api_trade_type;
pub mod api_tron_account_activation;
pub mod api_wallet;
pub mod api_withdraw;
pub mod api_withdraw_strategy;
pub mod api_withdraw_strategy_chain_config;
pub mod asset_query_state;
pub mod asset_token_key;
pub mod assets;
pub mod bill;
pub mod chain;
pub mod coin;
pub mod config;
pub mod device;
pub mod exchange_rate;
pub mod expand_batch;
pub mod expand_batch_item;
pub mod expand_notify_state;
pub mod multisig_account;
pub mod multisig_member;
pub mod multisig_queue;
pub mod multisig_signatures;
pub mod node;
pub mod permission;
pub mod permission_user;
pub mod stake;
pub mod system_notification;
pub mod task_queue;
pub mod wallet;

// 是否过期
fn has_expiration(timestamp: i64, chain_code: wallet_types::chain::chain::ChainCode) -> bool {
    match chain_code {
        // 1天
        wallet_types::chain::chain::ChainCode::Bitcoin => {
            timestamp < chrono::Utc::now().timestamp() - 86400
        }
        // 10分钟
        wallet_types::chain::chain::ChainCode::Solana => {
            timestamp < chrono::Utc::now().timestamp() - (10 * 60)
        }
        // 30分钟
        wallet_types::chain::chain::ChainCode::Ethereum => {
            timestamp < chrono::Utc::now().timestamp() - (30 * 60)
        }
        // 30分钟
        wallet_types::chain::chain::ChainCode::Tron => {
            timestamp < chrono::Utc::now().timestamp() - (30 * 60)
        }
        // 30分钟
        wallet_types::chain::chain::ChainCode::BnbSmartChain => {
            timestamp < chrono::Utc::now().timestamp() - (30 * 60)
        }
        _ => {
            // 默认 给到30分钟
            timestamp < chrono::Utc::now().timestamp() - (30 * 60)
        }
    }
}
pub mod api_nonce_claim;
