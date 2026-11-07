use std::future::Future;
use tokio::time::Duration;

use crate::{
    config::runtime_defaults, infrastructure::api_wallet_total_assets_cache,
    response_vo::standard_wallet::account::BalanceInfo,
};

use super::{ApiAccountRepo, ApiAssetsDomain};

pub(super) fn wallet_total_assets_v3_lock_key(
    wallet_address: &str,
    account_id: Option<u32>,
    chain_code: Option<&str>,
) -> String {
    api_wallet_total_assets_cache::wallet_total_assets_key(wallet_address, account_id, chain_code)
}

fn wallet_total_assets_query_key(
    wallet_address: Option<&str>,
    account_id: Option<u32>,
    chain_code: Option<&str>,
) -> String {
    // 允许 wallet_address 为空，保持“全局查询”也能走统一去重/缓存键格式。
    let wallet_part = wallet_address.unwrap_or("none");
    wallet_total_assets_v3_lock_key(wallet_part, account_id, chain_code)
}

fn whole_wallet_count_limit(
    account_id: Option<u32>,
    chain_code: Option<&str>,
    small_wallet_address_threshold: i64,
) -> Option<i64> {
    if account_id.is_none() && chain_code.is_none() {
        Some(small_wallet_address_threshold.saturating_add(1))
    } else {
        None
    }
}

pub(super) fn invalidate_wallet_total_assets_cache(wallet_address: &str) -> usize {
    let defaults = runtime_defaults::api_assets();
    let keys =
        api_wallet_total_assets_cache::mark_wallet_stale(wallet_address, defaults.total_cache_ttl);

    let removed_count = keys.len();
    if removed_count > 0 {
        tracing::info!(
            wallet_address = %wallet_address,
            removed_count,
            "invalidated api wallet total-assets cache"
        );
    }

    removed_count
}

pub(super) fn schedule_wallet_total_assets_refresh(
    ctx: &'static crate::context::Context,
    wallet_address: String,
) -> bool {
    api_wallet_total_assets_cache::schedule_refresh(ctx, wallet_address)
}

fn is_db_pool_timeout_error(err: &str) -> bool {
    err.contains("pool timed out while waiting for an open connection")
}

fn is_v3_timeout_or_pool_timeout(err: &crate::error::service::ServiceError) -> bool {
    matches!(err, crate::error::service::ServiceError::Timeout)
        || is_db_pool_timeout_error(&err.to_string())
}

fn log_db_pool_timeout_metric(err: &str) {
    if is_db_pool_timeout_error(err) {
        tracing::warn!(metric = "db_pool_timeout_count", err = %err, "db pool timeout");
    }
}

// 同 key 聚合请求的统一入口：
// 先命中新鲜缓存，再通过 protected query 合并并发查询；瞬时超时返回陈旧缓存。
async fn load_total_assets_with_cache<F, Fut>(
    cache_key: &str,
    fresh_ttl: Duration,
    query_fn: F,
) -> Result<BalanceInfo, crate::error::service::ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<BalanceInfo, crate::error::service::ServiceError>>,
{
    api_wallet_total_assets_cache::load(cache_key, fresh_ttl, || async move {
        match query_fn().await {
            Ok(balance) => Ok(balance),
            Err(err) => {
                log_db_pool_timeout_metric(&err.to_string());
                Err(err)
            }
        }
    })
    .await
}

impl ApiAssetsDomain {
    pub async fn get_api_wallet_assets(
        ctx: &'static Context,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        // 统一从集中默认值读取，避免缓存/阈值参数散落在 service 与 domain 间。
        let defaults = runtime_defaults::api_assets();
        let cache_key = wallet_total_assets_query_key(wallet_address, account_id, chain_code);
        let fresh_ttl = defaults.total_cache_ttl;
        let allow_v2_fallback_large = defaults.allow_v2_fallback_large_wallet;
        let large_wallet_v3_timeout = defaults.large_wallet_v3_timeout;
        let cache_key_for_query = cache_key.clone();
        let small_wallet_address_threshold = defaults.small_wallet_address_threshold;

        load_total_assets_with_cache(&cache_key, fresh_ttl, || async move {
            if let Some(wallet_address) = wallet_address {
                let pool = ctx.api_wallet_pool()?;
                let count_start = std::time::Instant::now();
                let count_limit = whole_wallet_count_limit(
                    account_id,
                    chain_code,
                    small_wallet_address_threshold,
                );
                let address_count = match count_limit {
                    Some(limit) => {
                        ApiAccountRepo::count_by_wallet_address_up_to(
                            &pool,
                            wallet_address,
                            None,
                            None,
                            limit,
                        )
                        .await
                    }
                    None => {
                        ApiAccountRepo::count_by_wallet_address(
                            &pool,
                            wallet_address,
                            account_id,
                            chain_code,
                        )
                        .await
                    }
                }
                .unwrap_or(small_wallet_address_threshold + 1);
                tracing::debug!(
                    metric = "api_assets_wallet_account_count_ms",
                    cache_key = %cache_key_for_query,
                    wallet_address = %wallet_address,
                    account_id = ?account_id,
                    chain_code = ?chain_code,
                    address_count,
                    count_limit = ?count_limit,
                    elapsed_ms = count_start.elapsed().as_millis(),
                    "wallet total assets account count finished"
                );

                if address_count <= small_wallet_address_threshold {
                    tracing::debug!(
                        metric = "api_assets_total_path",
                        cache_key = %cache_key_for_query,
                        wallet_address = %wallet_address,
                        address_count,
                        path = "v2",
                        "wallet total assets using v2 path"
                    );
                    return Self::get_api_wallet_assets_v2(
                        ctx,
                        Some(wallet_address),
                        account_id,
                        chain_code,
                    )
                        .await;
                }

                // 这里已经持有 per-key query lock，避免再走 v3 内部同 key 锁导致自锁超时。
                tracing::debug!(
                    metric = "api_assets_total_path",
                    cache_key = %cache_key_for_query,
                    wallet_address = %wallet_address,
                    address_count,
                    path = "v3",
                    "wallet total assets using v3 path"
                );
                match Self::get_api_wallet_assets_v3_unlocked(
                    ctx,
                    wallet_address,
                    account_id,
                    chain_code,
                    large_wallet_v3_timeout,
                )
                .await
                {
                    Ok(res) => Ok(res),
                    Err(e) => {
                        // 大钱包 v3 失败最常见是超时/连接池耗尽，单独打点便于观察止血效果。
                        if is_v3_timeout_or_pool_timeout(&e) {
                            tracing::warn!(
                                metric = "api_assets_v3_timeout",
                                cache_key = %cache_key_for_query,
                                err = %e,
                                address_count,
                            "wallet total assets v3 timeout or pool-timeout"
                        );
                    }

                        if allow_v2_fallback_large {
                            tracing::warn!(
                                err = ?e,
                                address_count,
                                cache_key = %cache_key_for_query,
                                "get_api_wallet_assets: v3 failed, fallback to v2 (enabled)"
                            );
                            Self::get_api_wallet_assets_v2(
                                ctx,
                                Some(wallet_address),
                                account_id,
                                chain_code,
                            )
                                .await
                        } else {
                            // 默认禁止回退 v2：避免在高压场景回到重 SQL 路径，造成二次放大。
                            tracing::warn!(
                                metric = "api_assets_v2_fallback_blocked",
                                err = %e,
                                address_count,
                                cache_key = %cache_key_for_query,
                                "get_api_wallet_assets: v3 failed, fallback blocked for large wallet"
                            );
                            Err(e)
                        }
                    }
                }
            } else {
                Self::get_api_wallet_assets_v2(ctx, None, account_id, chain_code).await
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::time::{Duration, sleep};

    use crate::{
        error::service::ServiceError, infrastructure::api_wallet_total_assets_cache,
        response_vo::standard_wallet::account::BalanceInfo,
    };

    use super::{load_total_assets_with_cache, whole_wallet_count_limit};

    fn unique_key(prefix: &str) -> String {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        format!("{prefix}-{ts}")
    }

    #[test]
    fn whole_wallet_count_is_bounded_but_filtered_counts_remain_exact() {
        assert_eq!(whole_wallet_count_limit(None, None, 200), Some(201));
        assert_eq!(whole_wallet_count_limit(Some(7), None, 200), None);
        assert_eq!(whole_wallet_count_limit(None, Some("eth"), 200), None);
        assert_eq!(whole_wallet_count_limit(Some(7), Some("eth"), 200), None);
    }

    #[tokio::test]
    async fn total_assets_dedup_allows_single_real_query() {
        let key = unique_key("assets-dedup");
        let hit_count = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..50 {
            let hit_count = hit_count.clone();
            let key = key.clone();
            tasks.push(tokio::spawn(async move {
                load_total_assets_with_cache(&key, Duration::from_secs(5), || async move {
                    hit_count.fetch_add(1, Ordering::SeqCst);
                    sleep(Duration::from_millis(30)).await;
                    Ok(BalanceInfo {
                        amount: 1.0,
                        currency: "USD".to_string(),
                        unit_price: None,
                        fiat_value: Some(1.0),
                    })
                })
                .await
            }));
        }

        for t in tasks {
            let res = t.await.expect("join ok");
            assert!(res.is_ok());
        }

        assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn total_assets_query_propagates_failure_when_refresh_fails() {
        let key = unique_key("assets-fail");

        let err = load_total_assets_with_cache(&key, Duration::from_millis(0), || async {
            Err(ServiceError::Timeout)
        })
        .await
        .expect_err("should propagate failure");

        assert!(matches!(err, ServiceError::Timeout));
    }

    #[tokio::test]
    async fn total_assets_query_preserves_parameter_failure() {
        let key = unique_key("assets-parameter-fail");

        let err = load_total_assets_with_cache(&key, Duration::from_millis(0), || async {
            Err(ServiceError::Parameter("invalid wallet address".to_string()))
        })
        .await
        .expect_err("should propagate failure");

        assert!(matches!(err, ServiceError::Parameter(msg) if msg == "invalid wallet address"));
    }

    #[tokio::test]
    async fn total_assets_query_returns_stale_cache_on_timeout() {
        let key = unique_key("assets-stale-timeout");
        let first = load_total_assets_with_cache(&key, Duration::from_secs(60), || async {
            Ok(BalanceInfo {
                amount: 7.0,
                currency: "USD".to_string(),
                unit_price: None,
                fiat_value: Some(7.0),
            })
        })
        .await
        .expect("initial query ok");
        assert_eq!(first.amount, 7.0);

        api_wallet_total_assets_cache::mark_key_stale(&key, Duration::from_secs(3600));

        let stale = load_total_assets_with_cache(&key, Duration::from_millis(0), || async {
            Err(ServiceError::Timeout)
        })
        .await
        .expect("timeout should return stale cache without age limit");

        assert_eq!(stale.amount, 7.0);
    }

    #[tokio::test]
    async fn invalidate_wallet_total_assets_cache_marks_matching_entry_stale() {
        let key1 = super::wallet_total_assets_v3_lock_key("0xabc", Some(7), Some("ETH"));
        let key2 = super::wallet_total_assets_v3_lock_key("0xabc", None, None);
        let hit_count = Arc::new(AtomicUsize::new(0));

        let _ = load_total_assets_with_cache(&key1, Duration::from_secs(60), {
            let hit_count = hit_count.clone();
            move || async move {
                hit_count.fetch_add(1, Ordering::SeqCst);
                Ok(BalanceInfo {
                    amount: 2.0,
                    currency: "USD".to_string(),
                    unit_price: None,
                    fiat_value: Some(2.0),
                })
            }
        })
        .await
        .expect("first query ok");

        let _ = load_total_assets_with_cache(&key2, Duration::from_secs(60), {
            let hit_count = hit_count.clone();
            move || async move {
                hit_count.fetch_add(1, Ordering::SeqCst);
                Ok(BalanceInfo {
                    amount: 4.0,
                    currency: "USD".to_string(),
                    unit_price: None,
                    fiat_value: Some(4.0),
                })
            }
        })
        .await
        .expect("second query ok");

        assert_eq!(hit_count.load(Ordering::SeqCst), 2);

        let removed = super::invalidate_wallet_total_assets_cache("0xabc");
        assert_eq!(removed, 2);

        let _ = load_total_assets_with_cache(&key1, Duration::from_millis(1), {
            let hit_count = hit_count.clone();
            move || async move {
                hit_count.fetch_add(1, Ordering::SeqCst);
                Ok(BalanceInfo {
                    amount: 5.0,
                    currency: "USD".to_string(),
                    unit_price: None,
                    fiat_value: Some(5.0),
                })
            }
        })
        .await
        .expect("third query ok");

        assert_eq!(hit_count.load(Ordering::SeqCst), 3);
    }
}
use crate::context::Context;
