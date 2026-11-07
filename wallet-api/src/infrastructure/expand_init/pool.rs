// expand_init/pool.rs

use futures::Future;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// 后台任务池，用于管理异步执行的INIT任务
/// 使用 tokio::spawn + Semaphore 实现并发控制
#[derive(Debug)]
pub struct BackgroundTaskPool {
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl BackgroundTaskPool {
    /// 创建新的后台任务池
    /// 参数：max_concurrent - 最大并发数
    /// 返回：BackgroundTaskPool - 任务池实例
    pub fn new(max_concurrent: usize) -> Self {
        Self { semaphore: Arc::new(Semaphore::new(max_concurrent)), max_concurrent }
    }

    /// 添加任务到后台执行
    /// 参数：future - 要执行的异步任务
    /// 注意：任务的返回类型必须是Result<(), ServiceError>
    pub async fn push<F>(&self, future: F)
    where
        F: Future<Output = Result<(), crate::error::service::ServiceError>> + Send + 'static,
    {
        let semaphore = self.semaphore.clone();

        // 使用 tokio::spawn 执行任务，配合 Semaphore 控制并发
        tokio::spawn(async move {
            // 克隆信号量，用于获取可用许可证数量
            let semaphore_clone = semaphore.clone();

            // 尝试获取信号量，如果超过并发限制则等待
            let _permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(e) => {
                    tracing::error!(
                        "INIT_POOL: Failed to acquire semaphore for background task: {:?}",
                        e
                    );
                    return;
                }
            };

            tracing::debug!(
                "INIT_POOL: Background task started, permits left: {}",
                semaphore_clone.available_permits()
            );

            // 执行任务
            if let Err(e) = future.await {
                tracing::error!("INIT_POOL: Background task failed: {:?}", e);
            }

            tracing::debug!(
                "INIT_POOL: Background task finished, permits left: {}",
                semaphore_clone.available_permits() + 1
            );
        });
    }

    /// 获取当前任务数（正在执行的任务数）
    /// 返回：usize - 正在执行的任务数
    pub fn len(&self) -> usize {
        // 正在执行的任务数 = 总容量 - 可用许可证数
        self.max_concurrent - self.semaphore.available_permits()
    }
}

/// INIT任务池，用于并发执行INIT任务
/// 最大并发数：20
pub static INIT_POOL: Lazy<BackgroundTaskPool> = Lazy::new(|| BackgroundTaskPool::new(20));
