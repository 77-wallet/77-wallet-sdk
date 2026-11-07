use std::{future::Future, sync::Arc, time::Instant};

use dashmap::DashMap;
use tokio::{
    sync::{Mutex, Notify},
    time::Duration,
};

use crate::error::service::ServiceError;

struct CachedValue<T> {
    value: T,
    updated_at: Instant,
}

struct Entry<T> {
    running: bool,
    result: Option<Arc<T>>,
}

struct Flight<T> {
    state: Mutex<Entry<T>>,
    notify: Notify,
}

impl<T> Flight<T> {
    fn new() -> Self {
        Self { state: Mutex::new(Entry { running: false, result: None }), notify: Notify::new() }
    }
}

pub(crate) struct ProtectedQuery<T> {
    cache: DashMap<String, CachedValue<T>>,
    flights: DashMap<String, Arc<Flight<T>>>,
}

impl<T> Default for ProtectedQuery<T> {
    fn default() -> Self {
        Self { cache: DashMap::new(), flights: DashMap::new() }
    }
}

impl<T> ProtectedQuery<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) async fn load<F, Fut>(
        &self,
        key: &str,
        fresh_ttl: Duration,
        query_fn: F,
    ) -> Result<T, ServiceError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ServiceError>>,
    {
        if let Some(cached) = self.get_fresh(key, fresh_ttl) {
            return Ok(cached);
        }

        self.call_singleflight(key, || async move {
            if let Some(cached) = self.get_fresh(key, fresh_ttl) {
                return Ok(cached);
            }

            match query_fn().await {
                Ok(value) => {
                    self.set(key, &value);
                    Ok(value)
                }
                Err(err) => {
                    if is_timeout_or_db_pool_timeout(&err)
                        && let Some(cached) = self.get_stale(key)
                    {
                        tracing::warn!(
                            metric = "protected_query_stale_cache_hit",
                            cache_key = %key,
                            err = %err,
                            "protected query failed, returning stale cached result"
                        );
                        return Ok(cached);
                    }
                    Err(err)
                }
            }
        })
        .await
    }

    pub(crate) fn get_fresh(&self, key: &str, max_age: Duration) -> Option<T> {
        self.cache.get(key).and_then(|entry| {
            if entry.updated_at.elapsed() <= max_age { Some(entry.value.clone()) } else { None }
        })
    }

    pub(crate) fn get_stale(&self, key: &str) -> Option<T> {
        self.cache.get(key).map(|entry| entry.value.clone())
    }

    pub(crate) fn set(&self, key: &str, value: &T) {
        self.cache.insert(
            key.to_string(),
            CachedValue { value: value.clone(), updated_at: Instant::now() },
        );
        // A completed flight memoizes its result independently from the TTL cache.
        // Drop it whenever the cache is replaced so a later stale-cache load cannot
        // resurrect the value that preceded this write.
        self.invalidate_flight(key);
    }

    pub(crate) fn mark_key_stale(&self, key: &str, fresh_ttl: Duration) {
        if let Some(mut entry) = self.cache.get_mut(key) {
            let stale_at = Instant::now()
                .checked_sub(fresh_ttl + Duration::from_millis(1))
                .unwrap_or_else(Instant::now);
            entry.updated_at = stale_at;
        }
    }

    pub(crate) fn mark_matching_stale(&self, prefix: &str, fresh_ttl: Duration) -> Vec<String> {
        let keys: Vec<String> = self
            .cache
            .iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .map(|entry| entry.key().clone())
            .collect();

        for key in &keys {
            self.mark_key_stale(key, fresh_ttl);
            self.invalidate_flight(key);
        }

        keys
    }

    pub(crate) fn mark_stale(&self, key: &str, fresh_ttl: Duration) -> bool {
        let existed = self.cache.contains_key(key);
        self.mark_key_stale(key, fresh_ttl);
        self.invalidate_flight(key);
        existed
    }

    pub(crate) fn invalidate_key(&self, key: &str) -> bool {
        let existed = self.cache.remove(key).is_some();
        self.invalidate_flight(key);
        existed
    }

    pub(crate) fn invalidate_flight(&self, key: &str) -> bool {
        self.flights.remove(key).is_some()
    }

    async fn call_singleflight<F, Fut>(&self, key: &str, query_fn: F) -> Result<T, ServiceError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ServiceError>>,
    {
        let flight =
            self.flights.entry(key.to_string()).or_insert_with(|| Arc::new(Flight::new())).clone();

        loop {
            let mut state = flight.state.lock().await;
            if let Some(result) = state.result.clone() {
                return Ok(result.as_ref().clone());
            }

            if !state.running {
                state.running = true;
                drop(state);

                let result = query_fn().await.map(Arc::new);
                let mut state = flight.state.lock().await;
                state.running = false;
                if let Ok(value) = &result {
                    state.result = Some(value.clone());
                }
                flight.notify.notify_waiters();
                return result.map(|value| value.as_ref().clone());
            }

            let notified = flight.notify.notified();
            drop(state);
            notified.await;
        }
    }
}

pub(crate) fn is_timeout_or_db_pool_timeout(err: &ServiceError) -> bool {
    matches!(err, ServiceError::Timeout) || err.is_transient_db_pool_timeout()
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

    use super::ProtectedQuery;
    use crate::error::service::ServiceError;

    fn unique_key(prefix: &str) -> String {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        format!("{prefix}-{ts}")
    }

    #[tokio::test]
    async fn protected_query_deduplicates_concurrent_loads() {
        let protected = Arc::new(ProtectedQuery::<u32>::default());
        let key = unique_key("protected-dedup");
        let hits = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..32 {
            let protected = protected.clone();
            let key = key.clone();
            let hits = hits.clone();
            tasks.push(tokio::spawn(async move {
                protected
                    .load(&key, Duration::from_secs(5), || async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        sleep(Duration::from_millis(20)).await;
                        Ok(7)
                    })
                    .await
            }));
        }

        for task in tasks {
            assert_eq!(task.await.expect("join ok").expect("query ok"), 7);
        }

        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn protected_query_returns_stale_on_timeout() {
        let protected = ProtectedQuery::<u32>::default();
        let key = unique_key("protected-stale");

        let first = protected
            .load(&key, Duration::from_secs(60), || async { Ok(9) })
            .await
            .expect("initial query ok");
        assert_eq!(first, 9);

        protected.invalidate_flight(&key);
        protected.mark_key_stale(&key, Duration::from_secs(3600));

        let stale = protected
            .load(&key, Duration::from_millis(0), || async { Err(ServiceError::Timeout) })
            .await
            .expect("timeout should use stale cache");
        assert_eq!(stale, 9);
    }

    #[tokio::test]
    async fn protected_query_set_discards_completed_flight_result() {
        // Arrange: populate both the TTL cache and the completed flight result.
        let protected = ProtectedQuery::<u32>::default();
        let key = unique_key("protected-set-flight");
        let initial = protected
            .load(&key, Duration::from_secs(60), || async { Ok(1) })
            .await
            .expect("initial query ok");
        assert_eq!(initial, 1);

        // Act: replace the cached value, then make it stale to force another load.
        protected.set(&key, &2);
        assert_eq!(protected.get_fresh(&key, Duration::from_secs(60)), Some(2));
        protected.mark_key_stale(&key, Duration::from_secs(60));
        let refreshed = protected
            .load(&key, Duration::from_secs(60), || async { Ok(3) })
            .await
            .expect("refresh query ok");

        // Assert: the pre-write flight result must not resurrect value 1.
        assert_eq!(refreshed, 3);
    }
}
