use tokio::time::{Duration, Interval, MissedTickBehavior};
use tracing::debug;

/// 生产级别的 interval 封装
/// 确保所有 interval 都使用 Skip 行为并包含启动屏障
#[must_use = "ProductionInterval must be driven by an actor loop via tick()"]
pub struct ProductionInterval {
    interval: Interval,
    first: bool,
}

impl ProductionInterval {
    /// 创建一个新的生产级 interval
    /// - 设置 MissedTickBehavior::Skip 避免 replay storm
    /// - 自动处理第一次 tick 作为启动屏障
    pub fn new(duration: Duration) -> Self {
        assert!(duration > Duration::ZERO, "ProductionInterval duration must be > 0");

        let mut interval = tokio::time::interval(duration);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        Self { interval, first: true }
    }

    /// 获取下一个 tick
    /// 第一次调用时会自动处理启动屏障，吃掉 tokio 的即时 tick
    pub async fn tick(&mut self) {
        while self.first {
            self.first = false;
            // 吃掉第一次即时 tick，作为启动屏障
            let _ = self.interval.tick().await;
        }
        self.interval.tick().await;
    }
}

/// 创建一个新的生产级 interval
pub fn new_production_interval(duration: Duration) -> ProductionInterval {
    ProductionInterval::new(duration)
}

#[cfg(debug_assertions)]
impl Drop for ProductionInterval {
    fn drop(&mut self) {
        if self.first {
            debug!("ProductionInterval dropped before first tick — may be early shutdown");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_production_interval_barrier() {
        let mut interval = new_production_interval(Duration::from_secs(1));
        let start = std::time::Instant::now();

        interval.tick().await;
        interval.tick().await;

        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_secs(1));
        assert!(elapsed < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_production_interval_basic() {
        let mut interval = new_production_interval(Duration::from_millis(100));

        interval.tick().await;

        let start = std::time::Instant::now();
        interval.tick().await;

        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(50));
        assert!(elapsed < Duration::from_millis(300));
    }

    #[tokio::test]
    async fn test_production_interval_zero_duration() {
        let result = std::panic::catch_unwind(|| {
            let _ = new_production_interval(Duration::ZERO);
        });

        assert!(result.is_err(), "duration zero should panic");
    }
}
