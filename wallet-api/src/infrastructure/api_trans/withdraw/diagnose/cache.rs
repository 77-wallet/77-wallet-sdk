use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use wallet_database::entities::api_withdraw::ApiWithdrawEntity;

use crate::infrastructure::api_trans::withdraw::diagnose::{
    engine::{DiagnoseResult, diagnose_withdraw},
    fact_snapshot::fact_mask,
};

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct DiagnoseCacheKey {
    trade_no: String,
    fact_mask: (u64, u8),
}

impl DiagnoseCacheKey {
    pub fn new(withdraw: &ApiWithdrawEntity) -> Self {
        Self { trade_no: withdraw.trade_no.clone(), fact_mask: fact_mask(withdraw) }
    }
}

#[derive(Debug)]
struct CacheEntry {
    result: DiagnoseResult,
    created_at: Instant,
}

#[derive(Debug)]
pub struct DiagnoseCache {
    cache: Mutex<HashMap<DiagnoseCacheKey, CacheEntry>>,
    capacity: usize,
    ttl: Duration,
}

impl DiagnoseCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self { cache: Mutex::new(HashMap::with_capacity(capacity)), capacity, ttl }
    }

    pub fn default() -> Self {
        Self::new(1000, Duration::from_secs(5))
    }

    pub fn get(&self, withdraw: &ApiWithdrawEntity) -> Option<DiagnoseResult> {
        let key = DiagnoseCacheKey::new(withdraw);
        let mut cache = self.cache.lock().unwrap();
        self.cleanup_expired(&mut cache);
        cache.get(&key).map(|entry| entry.result.clone())
    }

    pub fn put(&self, withdraw: &ApiWithdrawEntity, result: DiagnoseResult) {
        let key = DiagnoseCacheKey::new(withdraw);
        let mut cache = self.cache.lock().unwrap();
        self.cleanup_expired(&mut cache);

        if cache.len() >= self.capacity {
            let oldest_key =
                cache.iter().min_by_key(|(_, entry)| entry.created_at).map(|(k, _)| k.clone());
            if let Some(oldest_key) = oldest_key {
                cache.remove(&oldest_key);
            }
        }

        cache.insert(key, CacheEntry { result, created_at: Instant::now() });
    }

    fn cleanup_expired(&self, cache: &mut HashMap<DiagnoseCacheKey, CacheEntry>) {
        let now = Instant::now();
        cache.retain(|_, entry| now - entry.created_at < self.ttl);
    }
}

#[derive(Debug)]
pub struct CachedDiagnoser {
    cache: DiagnoseCache,
}

impl CachedDiagnoser {
    pub fn new(cache: DiagnoseCache) -> Self {
        Self { cache }
    }

    pub fn default() -> Self {
        Self::new(DiagnoseCache::default())
    }

    pub fn diagnose(&self, withdraw: &ApiWithdrawEntity) -> DiagnoseResult {
        if let Some(result) = self.cache.get(withdraw) {
            return result;
        }

        let result = diagnose_withdraw(withdraw);
        self.cache.put(withdraw, result.clone());
        result
    }
}
