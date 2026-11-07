use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const WALLET_PASSWORD: &str = "wallet password";

use tokio::sync::RwLock;

pub(crate) static GLOBAL_CACHE: Lazy<Arc<SharedCache>> = Lazy::new(|| {
    let cache = Arc::new(SharedCache::new());
    cache.spawn_cleaner();
    cache
});

#[derive(Debug)]
pub struct SharedCache {
    inner: RwLock<CacheMap>,
}
impl SharedCache {
    pub fn new() -> Self {
        Self { inner: RwLock::new(CacheMap::new()) }
    }

    pub fn spawn_cleaner(self: &std::sync::Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                let sleep_dur = {
                    let mut lock = this.inner.write().await;
                    lock.cleanup_expired();

                    lock.next_expiration_duration()
                };

                match sleep_dur {
                    Some(dur) => tokio::time::sleep(dur).await,
                    None => {
                        // 没有任何带过期的数据，避免 busy-loop
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                }
            }
        });
    }

    pub async fn set_with_expiration<T: serde::Serialize + std::fmt::Debug>(
        &self,
        key: &str,
        value: T,
        expiration: u64,
    ) -> Result<(), crate::error::service::ServiceError> {
        let entry = CacheEntry::new(value, Some(Duration::from_secs(expiration)))?;
        let mut lock = self.inner.write().await;
        lock.set(key.to_string(), entry);
        Ok(())
    }

    pub async fn set<T: serde::Serialize + std::fmt::Debug>(
        &self,
        key: &str,
        value: T,
    ) -> Result<(), crate::error::service::ServiceError> {
        let entry = CacheEntry::new(value, None)?;
        let mut lock = self.inner.write().await;
        lock.set(key.to_string(), entry);
        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut should_delete = false;
        let result = {
            let lock = self.inner.read().await;
            if let Some(entry) = lock.get(key) {
                if entry.is_expired() {
                    should_delete = true;
                    None
                } else {
                    entry.deserialize::<T>().ok()
                }
            } else {
                None
            }
        };

        if should_delete {
            let mut lock = self.inner.write().await;
            lock.delete(key);
        }

        result
    }

    #[allow(dead_code)]
    pub async fn get_raw(&self, key: &str) -> Option<CacheEntry> {
        let lock = self.inner.read().await;
        lock.get(key).cloned()
    }

    #[allow(dead_code)]
    pub async fn delete(&self, key: &str) -> Result<(), crate::error::service::ServiceError> {
        let mut lock = self.inner.write().await;
        lock.delete(key);

        Ok(())
    }
}

#[derive(Debug)]
struct CacheMap {
    data: HashMap<String, CacheEntry>,
    expirations: BinaryHeap<Reverse<(Instant, String)>>, // 小顶堆
}

impl CacheMap {
    fn new() -> Self {
        CacheMap { data: HashMap::new(), expirations: BinaryHeap::new() }
    }

    fn set(&mut self, key: String, entry: CacheEntry) {
        if let Some(expire_at) = entry.instant {
            self.expirations.push(Reverse((expire_at, key.clone())));
        }
        self.data.insert(key, entry);
    }

    fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.data.get(key)
    }

    fn delete(&mut self, key: &str) {
        self.data.remove(key);
    }

    fn cleanup_expired(&mut self) {
        let now = Instant::now();
        while let Some(Reverse((when, key))) = self.expirations.peek().cloned() {
            if when <= now {
                self.expirations.pop();
                if let Some(entry) = self.data.get(&key) {
                    if entry.is_expired() {
                        self.data.remove(&key);
                    }
                }
            } else {
                break;
            }
        }
    }

    fn next_expiration_duration(&self) -> Option<Duration> {
        let now = Instant::now();
        self.expirations.peek().map(
            |Reverse((when, _))| {
                if *when > now { *when - now } else { Duration::from_secs(0) }
            },
        )
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub data: Vec<u8>,
    instant: Option<Instant>,
}

impl CacheEntry {
    fn new<T: serde::Serialize + std::fmt::Debug>(
        data: T,
        expiration: Option<Duration>,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let bytes = wallet_utils::hex_func::bin_encode_bytes(&data)?;

        let instant = expiration.map(|d| Instant::now() + d);

        Ok(Self { data: bytes, instant })
    }

    pub fn is_expired(&self) -> bool {
        self.instant.is_some_and(|instant| instant <= Instant::now())
    }

    pub fn deserialize<T: DeserializeOwned>(
        &self,
    ) -> Result<T, crate::error::service::ServiceError> {
        Ok(wallet_utils::hex_func::bin_decode_bytes(&self.data)?)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_cache() {
        let cache = SharedCache::new();

        let _rs = cache.set_with_expiration("hello", "world", 10).await;
        let rs: Option<String> = cache.get("hello").await;

        println!("{:?}", rs);
    }

    #[tokio::test]
    async fn test_cached() {
        let cache = SharedCache::new();

        let _rs = cache.set_with_expiration("hello", "world", 10).await;

        let rs: Option<String> = cache.get("hello").await;
        println!("{:?}", rs);

        println!("睡眠5秒");
        sleep(std::time::Duration::from_secs(5)).await;

        let rs: Option<String> = cache.get("hello").await;
        println!("再次获取 {:?},是否过期 {}", rs.clone(), rs.is_none());

        println!("睡眠6秒");
        sleep(std::time::Duration::from_secs(5)).await;

        let rs: Option<String> = cache.get("hello").await;
        println!("再次获取 {:?},是否过期 {}", rs.clone(), rs.is_none());
    }
}
