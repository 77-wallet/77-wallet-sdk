pub mod api_trans;
pub(crate) mod api_wallet_asset_query_guard;
pub mod api_wallet_assets_sync;
pub(crate) mod api_wallet_total_assets_cache;
pub mod cache;
pub mod expand_address;
pub mod expand_init;
pub(crate) mod path_guard;
pub mod phrase_package;
pub mod private_key_manager;
pub(crate) mod protected_query;
pub mod recovery;
pub mod system_ready;
pub mod task_queue;

pub mod asset_calc;
pub mod chain_node;
pub mod collector_unconfirm_msg;
pub mod inner_event;
pub mod log;
pub mod mqtt;
pub mod nonce;
pub mod runtime;
pub mod unlock_session;

pub mod chain_rpc_guard;
pub mod process_unconfirm_msg;
pub mod swap_client;

use chrono::{DateTime, NaiveDateTime, Utc};

// time 转换, 默认返回 1970-01-01
pub fn parse_utc_datetime(s: &str) -> DateTime<Utc> {
    match NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        Ok(naive) => DateTime::from_naive_utc_and_offset(naive, Utc),
        Err(_) => DateTime::<Utc>::default(),
    }
}

pub fn parse_utc_with_error(s: &str) -> Result<DateTime<Utc>, crate::error::service::ServiceError> {
    match NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        Ok(naive) => Ok(DateTime::from_naive_utc_and_offset(naive, Utc)),
        Err(e) => {
            let e = crate::error::service::ServiceError::Parameter(format!(
                "convert time error: {}",
                e
            ));
            Err(e)
        }
    }
}
