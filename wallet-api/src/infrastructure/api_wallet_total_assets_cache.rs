use once_cell::sync::Lazy;
use tokio::time::Duration;

use crate::{
    error::service::ServiceError, infrastructure::protected_query::ProtectedQuery,
    response_vo::standard_wallet::account::BalanceInfo,
};

static WALLET_TOTAL_ASSETS_QUERY: Lazy<ProtectedQuery<BalanceInfo>> =
    Lazy::new(ProtectedQuery::default);
static WALLET_TOTAL_ASSETS_REFRESHING: Lazy<dashmap::DashMap<String, ()>> =
    Lazy::new(dashmap::DashMap::new);

pub(crate) fn wallet_total_assets_key(
    wallet_address: &str,
    account_id: Option<u32>,
    chain_code: Option<&str>,
) -> String {
    let account_part = account_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string());
    let chain_part = chain_code.unwrap_or("none");
    format!("wallet={wallet_address};account_id={account_part};chain_code={chain_part}")
}

pub(crate) fn get_fresh(key: &str, max_age: Duration) -> Option<BalanceInfo> {
    WALLET_TOTAL_ASSETS_QUERY.get_fresh(key, max_age)
}

pub(crate) fn get_stale(key: &str) -> Option<BalanceInfo> {
    WALLET_TOTAL_ASSETS_QUERY.get_stale(key)
}

pub(crate) fn set(key: &str, value: &BalanceInfo) {
    WALLET_TOTAL_ASSETS_QUERY.set(key, value);
}

pub(crate) async fn load<F, Fut>(
    key: &str,
    fresh_ttl: Duration,
    query_fn: F,
) -> Result<BalanceInfo, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<BalanceInfo, ServiceError>>,
{
    WALLET_TOTAL_ASSETS_QUERY.load(key, fresh_ttl, query_fn).await
}

pub(crate) fn mark_wallet_stale(wallet_address: &str, fresh_ttl: Duration) -> Vec<String> {
    WALLET_TOTAL_ASSETS_QUERY.mark_matching_stale(&format!("wallet={wallet_address};"), fresh_ttl)
}

pub(crate) fn mark_key_stale(key: &str, fresh_ttl: Duration) {
    WALLET_TOTAL_ASSETS_QUERY.mark_key_stale(key, fresh_ttl);
}

pub(crate) fn schedule_refresh(
    ctx: &'static crate::context::Context,
    wallet_address: String,
) -> bool {
    let cache_key = wallet_total_assets_key(&wallet_address, None, None);
    if WALLET_TOTAL_ASSETS_REFRESHING.insert(cache_key.clone(), ()).is_some() {
        tracing::debug!(
            cache_key = %cache_key,
            wallet_address = %wallet_address,
            "api wallet total-assets refresh already scheduled"
        );
        return false;
    }

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let result = crate::domain::api_wallet::assets::ApiAssetsDomain::get_api_wallet_assets(
            ctx,
            Some(&wallet_address),
            None,
            None,
        )
        .await;
        match result {
            Ok(_) => {
                tracing::debug!(
                    cache_key = %cache_key,
                    wallet_address = %wallet_address,
                    "api wallet total-assets cache refreshed after balance sync"
                );
            }
            Err(err) => {
                tracing::warn!(
                    cache_key = %cache_key,
                    wallet_address = %wallet_address,
                    err = %err,
                    "api wallet total-assets cache refresh failed after balance sync"
                );
            }
        }
        WALLET_TOTAL_ASSETS_REFRESHING.remove(&cache_key);
    });

    true
}
