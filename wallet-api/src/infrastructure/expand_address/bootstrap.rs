// bootstrap.rs
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use crate::{
    error::service::ServiceError,
    infrastructure::expand_address::{event::channel, scanner::ExpandScanner},
};

pub(crate) struct ExpandBootstrap;

static EXPAND_SCANNER_STARTED: AtomicBool = AtomicBool::new(false);

impl ExpandBootstrap {
    pub async fn start_after_first_wallet_unlock(
        ctx: &'static crate::context::Context,
    ) -> Result<(), ServiceError> {
        if EXPAND_SCANNER_STARTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            tracing::info!("ExpandScanner already started, skip");
            return Ok(());
        }

        // Done batches are recovered by the scanner's existing Notify worker.
        // Startup must not depend on network reporting or duplicate its side effects.
        Self::start_scanner(ctx).await?;
        Ok(())
    }

    /// 启动ExpandScanner，作为系统的唯一核心驱动
    /// 每30秒执行一次扫描
    pub async fn start_scanner(ctx: &'static crate::context::Context) -> Result<(), ServiceError> {
        tracing::info!("开始启动ExpandScanner作为唯一核心驱动");

        // 创建事件通道
        // bounded channel to prevent unbounded memory growth
        // overflow is acceptable because events are only hints
        let (event_tx, event_rx) = channel();

        // 将事件发射器保存到全局上下文中，以便其他组件触发事件
        ctx.set_expand_event_tx(Some(event_tx)).await;

        // 创建并启动Scanner
        // 扫描间隔：6秒
        // 单轮扫描上限：5000个items
        let scanner = ExpandScanner::new(ctx, Duration::from_secs(6), 20000, Some(event_rx));

        // 在后台启动扫描器
        tokio::spawn(async move {
            scanner.start().await;
        });

        tracing::info!("ExpandScanner已成功启动，支持事件驱动");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EXPAND_SCANNER_STARTED;
    use std::sync::atomic::Ordering;

    #[test]
    fn expand_scanner_started_flag_is_atomic_and_single_use() {
        EXPAND_SCANNER_STARTED.store(false, Ordering::Release);
        assert!(
            EXPAND_SCANNER_STARTED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
        assert!(
            EXPAND_SCANNER_STARTED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        );
        EXPAND_SCANNER_STARTED.store(false, Ordering::Release);
    }
}
