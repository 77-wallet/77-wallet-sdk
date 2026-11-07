use crate::infrastructure::api_trans::diagnose_common::event::DiagnoseStage;
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

/// 最大冷却映射大小
const MAX_COOLDOWN_SIZE: usize = 20_000;

// 硬上限冷却映射大小
const HARD_MAX_COOLDOWN_SIZE: usize = 50_000;

lazy_static::lazy_static! {
    /// 卡住诊断的冷却时间映射（trade_no + stage -> last_ts）
    pub static ref STUCK_COOLDOWN_MAP: dashmap::DashMap<std::sync::Arc<str>, Instant> = dashmap::DashMap::new();
    pub static ref COOLDOWN_KEY_QUEUE: Mutex<VecDeque<(std::sync::Arc<str>, Instant)>> = Mutex::new(VecDeque::new());
}

/// 全局诊断速率限制（每秒）
const GLOBAL_DIAGNOSE_QPS: u64 = 100;

/// 每阶段最小保障额度（每秒）
const MIN_STAGE_QPS: u64 = 5;

/// 速率限制桶
pub struct RateLimitBucket {
    epoch: u64,
    count: u64,
}

// 按阶段分桶的速率限制
lazy_static::lazy_static! {
    pub static ref STAGE_RATE_LIMITS: Mutex<[RateLimitBucket; 8]> = Mutex::new([
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
        RateLimitBucket { epoch: 0, count: 0 },
    ]);
    pub static ref GLOBAL_RATE_LIMIT: Mutex<RateLimitBucket> = Mutex::new(RateLimitBucket { epoch: 0, count: 0 });
}

/// 获取当前时间戳（秒）
fn now_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn stage_index(stage: DiagnoseStage) -> usize {
    match stage {
        DiagnoseStage::OrderAck => 0,
        DiagnoseStage::Build => 1,
        DiagnoseStage::Broadcast => 2,
        DiagnoseStage::Recover => 3,
        DiagnoseStage::ResultAck => 4,
        DiagnoseStage::ServiceFeeUpload => 5,
        DiagnoseStage::TxFeeResAck => 6,
        DiagnoseStage::TxExecReceipt => 7,
        DiagnoseStage::Unknown => 0,
    }
}

/// 检查全局速率限制
fn check_global_rate_limit() -> bool {
    let current_epoch = now_epoch();
    let mut bucket = GLOBAL_RATE_LIMIT.lock().unwrap();

    if bucket.epoch != current_epoch {
        bucket.epoch = current_epoch;
        bucket.count = 0;
    }

    let count = bucket.count;
    bucket.count += 1;
    count < GLOBAL_DIAGNOSE_QPS
}

/// 检查阶段速率限制
fn check_stage_rate_limit(stage: DiagnoseStage) -> bool {
    let current_epoch = now_epoch();
    let mut buckets = STAGE_RATE_LIMITS.lock().unwrap();

    let bucket = &mut buckets[stage_index(stage)];

    if bucket.epoch != current_epoch {
        bucket.epoch = current_epoch;
        bucket.count = 0;
    }

    let count = bucket.count;
    bucket.count += 1;
    count < MIN_STAGE_QPS
}

/// 检查速率限制（先全局再分阶段）
pub fn check_rate_limit(stage: DiagnoseStage) -> bool {
    if !check_global_rate_limit() {
        return false;
    }
    check_stage_rate_limit(stage)
}

fn cooldown_key(trade_no: &str, stage: DiagnoseStage) -> std::sync::Arc<str> {
    let mut key = String::with_capacity(64);
    key.push_str(trade_no);
    key.push('-');
    key.push_str(&format!("{stage:?}"));
    std::sync::Arc::<str>::from(key.into_boxed_str())
}

/// 检查是否应该诊断卡住
/// 包含冷却时间检查和自动清理
pub fn should_diagnose(trade_no: &str, stage: DiagnoseStage, cooldown: Duration) -> bool {
    let key = cooldown_key(trade_no, stage);
    let now = Instant::now();

    let mut queue = COOLDOWN_KEY_QUEUE.lock().unwrap();

    // 清理过期项（O(1) 渐进式 GC）
    if STUCK_COOLDOWN_MAP.len() > MAX_COOLDOWN_SIZE {
        let mut removed = 0;
        while removed < 50 && !queue.is_empty() {
            if let Some((old_key, old_ts)) = queue.pop_front() {
                if let Some(entry) = STUCK_COOLDOWN_MAP.get(&old_key) {
                    if *entry == old_ts {
                        STUCK_COOLDOWN_MAP.remove(&old_key);
                        removed += 1;
                    }
                }
            }
        }
    }

    // 硬上限清理
    if queue.len() > HARD_MAX_COOLDOWN_SIZE {
        let remove_count = queue.len() / 10;
        for _ in 0..remove_count {
            if let Some((old_key, old_ts)) = queue.pop_front() {
                if let Some(entry) = STUCK_COOLDOWN_MAP.get(&old_key) {
                    if *entry == old_ts {
                        STUCK_COOLDOWN_MAP.remove(&old_key);
                    }
                }
            }
        }
    }

    // 检查冷却
    if let Some(entry) = STUCK_COOLDOWN_MAP.get(&key) {
        if now.duration_since(*entry) < cooldown {
            return false;
        }
    }

    STUCK_COOLDOWN_MAP.insert(key.clone(), now);
    queue.push_back((key, now));
    true
}
