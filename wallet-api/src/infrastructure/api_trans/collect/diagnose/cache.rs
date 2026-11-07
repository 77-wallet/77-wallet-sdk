use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::infrastructure::api_trans::collect::diagnose::{
    engine::{DiagnoseResult, diagnose_collect},
    fact_snapshot::fact_mask,
};
use wallet_database::entities::api_collect::ApiCollectEntity;

/// 诊断缓存键
/// 由 trade_no 和 fact_mask 组成，确保当交易事实发生变化时缓存自动失效
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct DiagnoseCacheKey {
    trade_no: String,
    fact_mask: (u64, u8),
}

impl DiagnoseCacheKey {
    pub fn new(collect: &ApiCollectEntity) -> Self {
        Self { trade_no: collect.trade_no.clone(), fact_mask: fact_mask(collect) }
    }
}

/// 缓存条目
#[derive(Debug)]
struct CacheEntry {
    result: DiagnoseResult,
    created_at: Instant,
}

/// 诊断缓存
/// 使用 LRU 策略，TTL 为 5 秒，避免重复的诊断计算
#[derive(Debug)]
pub struct DiagnoseCache {
    cache: Mutex<HashMap<DiagnoseCacheKey, CacheEntry>>,
    capacity: usize,
    ttl: Duration,
}

impl DiagnoseCache {
    /// 创建新的诊断缓存
    /// capacity: 缓存容量
    /// ttl: 缓存条目过期时间
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self { cache: Mutex::new(HashMap::with_capacity(capacity)), capacity, ttl }
    }

    /// 创建默认配置的诊断缓存
    /// 默认容量: 1000
    /// 默认 TTL: 5 秒
    pub fn default() -> Self {
        Self::new(1000, Duration::from_secs(5))
    }

    /// 从缓存中获取诊断结果
    /// 如果缓存中存在有效条目，则返回 Some(result)
    /// 否则返回 None
    pub fn get(&self, collect: &ApiCollectEntity) -> Option<DiagnoseResult> {
        let key = DiagnoseCacheKey::new(collect);
        let mut cache = self.cache.lock().unwrap();

        // 清理过期条目
        self.cleanup_expired(&mut cache);

        // 检查缓存是否包含该键
        if let Some(entry) = cache.get(&key) {
            // 缓存命中
            Some(entry.result.clone())
        } else {
            // 缓存未命中
            None
        }
    }

    /// 将诊断结果存入缓存
    pub fn put(&self, collect: &ApiCollectEntity, result: DiagnoseResult) {
        let key = DiagnoseCacheKey::new(collect);
        let mut cache = self.cache.lock().unwrap();

        // 清理过期条目
        self.cleanup_expired(&mut cache);

        // 如果缓存已满，移除最久未使用的条目
        // 注意：这里使用简化的 LRU 实现，实际生产环境可能需要更复杂的 LRU 结构
        if cache.len() >= self.capacity {
            // 找到最旧的条目
            let oldest_key =
                cache.iter().min_by_key(|(_, entry)| entry.created_at).map(|(key, _)| key.clone());

            if let Some(oldest_key) = oldest_key {
                cache.remove(&oldest_key);
            }
        }

        // 存入新条目
        cache.insert(key, CacheEntry { result, created_at: Instant::now() });
    }

    /// 清理过期的缓存条目
    fn cleanup_expired(&self, cache: &mut HashMap<DiagnoseCacheKey, CacheEntry>) {
        let now = Instant::now();
        cache.retain(|_, entry| now - entry.created_at < self.ttl);
    }

    /// 清空缓存
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// 获取缓存当前大小
    pub fn size(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }
}

/// 诊断缓存包装器
/// 提供更简洁的缓存访问接口
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

    /// 诊断交易，优先使用缓存结果
    pub fn diagnose(&self, collect: &ApiCollectEntity) -> DiagnoseResult {
        // 尝试从缓存获取
        if let Some(result) = self.cache.get(collect) {
            return result;
        }

        // 缓存未命中，执行诊断
        let result = diagnose_collect(collect);

        // 将结果存入缓存
        self.cache.put(collect, result.clone());

        result
    }
}
