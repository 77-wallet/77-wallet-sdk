use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

/// SPAWN_GUARD metrics 管理
/// 用于监控 spawn 保护的状态，提供可观测性
pub struct SpawnGuardMetrics {
    /// 最大 permit 数量
    max_permits: usize,
    /// 被拒绝的 spawn 次数
    rejected_count: AtomicU64,
    /// 进入饱和状态的时间
    enter_saturation_time: Option<Instant>,
    /// 总饱和时间（纳秒）
    total_saturation_time: AtomicU64,
}

impl SpawnGuardMetrics {
    /// 创建一个新的 SpawnGuardMetrics
    /// - max_permits: 最大 permit 数量
    pub fn new(max_permits: usize) -> Self {
        Self {
            max_permits,
            rejected_count: AtomicU64::new(0),
            enter_saturation_time: None,
            total_saturation_time: AtomicU64::new(0),
        }
    }

    /// 记录一次 spawn 尝试
    /// - available: 当前可用 permit 数量
    /// - granted: 是否成功获取 permit
    pub fn record_spawn_attempt(&mut self, available: usize, granted: bool) {
        if !granted {
            self.rejected_count.fetch_add(1, Ordering::Relaxed);
        }

        // 处理饱和状态
        if available == 0 {
            if self.enter_saturation_time.is_none() {
                self.enter_saturation_time = Some(Instant::now());
            }
        } else {
            if let Some(enter_time) = self.enter_saturation_time.take() {
                let saturation_duration = Instant::now().duration_since(enter_time);
                self.total_saturation_time
                    .fetch_add(saturation_duration.as_nanos() as u64, Ordering::Relaxed);
            }
        }
    }

    /// 获取当前 inflight worker 数量
    pub fn inflight_workers(&self, available: usize) -> usize {
        self.max_permits - available
    }

    /// 获取被拒绝的 spawn 次数
    pub fn rejected_count(&self) -> u64 {
        self.rejected_count.load(Ordering::Relaxed)
    }

    /// 获取总饱和时间（秒）
    pub fn total_saturation_time_seconds(&self) -> f64 {
        let nanos = self.total_saturation_time.load(Ordering::Relaxed);
        nanos as f64 / 1_000_000_000.0
    }
}

/// Loop Latency 监控
/// 用于记录 actor loop 的执行延迟，作为移动端 thermal 报警的早期指标
pub struct LoopLatencyMetrics {
    /// 最后一次 loop 开始时间
    last_loop_start: Option<Instant>,
    /// 总执行时间（纳秒）
    total_execution_time: AtomicU64,
    /// 执行次数
    execution_count: AtomicU64,
}

impl LoopLatencyMetrics {
    /// 创建一个新的 LoopLatencyMetrics
    pub fn new() -> Self {
        Self {
            last_loop_start: None,
            total_execution_time: AtomicU64::new(0),
            execution_count: AtomicU64::new(0),
        }
    }

    /// 记录 loop 开始
    pub fn record_loop_start(&mut self) {
        self.last_loop_start = Some(Instant::now());
    }

    /// 记录 loop 结束
    pub fn record_loop_end(&mut self) {
        if let Some(start_time) = self.last_loop_start.take() {
            let execution_time = Instant::now().duration_since(start_time);
            self.total_execution_time
                .fetch_add(execution_time.as_nanos() as u64, Ordering::Relaxed);
            self.execution_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 获取平均 loop 执行时间（毫秒）
    pub fn average_loop_latency_ms(&self) -> f64 {
        let total_nanos = self.total_execution_time.load(Ordering::Relaxed);
        let count = self.execution_count.load(Ordering::Relaxed);

        if count == 0 { 0.0 } else { (total_nanos as f64 / count as f64) / 1_000_000.0 }
    }
}
