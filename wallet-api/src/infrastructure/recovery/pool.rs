use std::sync::Arc;

use futures::Future;
use tokio::sync::Semaphore;

/// 后台任务池，用于管理异步执行的副作用任务
/// 阶段1重构：使用 tokio::spawn + Semaphore 替代 JoinSet
#[derive(Debug)]
pub struct BackgroundTaskPool {
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl BackgroundTaskPool {
    pub fn new(max_concurrent: usize) -> Self {
        Self { semaphore: Arc::new(Semaphore::new(max_concurrent)), max_concurrent }
    }

    /// 添加任务到后台执行
    pub async fn push<F>(&self, future: F)
    where
        F: Future<Output = Result<(), crate::error::service::ServiceError>> + Send + 'static,
    {
        // 先获取permit再spawn，避免瞬间spawn大量“排队任务”导致内存/调度压力，
        // 也能间接避免数据库连接池被后台任务洪峰压垮。
        let permit = match self.semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(e) => {
                tracing::error!("Failed to acquire semaphore for background task: {:?}", e);
                return;
            }
        };

        let semaphore = self.semaphore.clone();
        tokio::spawn(async move {
            tracing::debug!(
                "Background task started, permits left: {}",
                semaphore.available_permits()
            );

            if let Err(e) = future.await {
                tracing::error!("Background task failed: {:?}", e);
            }

            drop(permit);
            tracing::debug!(
                "Background task finished, permits left: {}",
                semaphore.available_permits()
            );
        });
    }

    #[cfg(test)]
    pub(crate) async fn hold_all_permits_for_test(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.semaphore.clone().acquire_many_owned(self.max_concurrent as u32).await.unwrap()
    }

    /// 获取当前任务数（正在执行的任务数）
    pub fn len(&self) -> usize {
        // 正在执行的任务数 = 总容量 - 可用许可证数
        self.max_concurrent - self.semaphore.available_permits()
    }
}
