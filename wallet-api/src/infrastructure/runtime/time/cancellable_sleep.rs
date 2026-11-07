use tokio::{
    sync::broadcast::Receiver,
    time::{Duration, sleep},
};

/// 可被 shutdown 信号中断的 sleep
///
/// 用于 actor 中的 jitter sleep 或其他需要可中断的延迟
///
/// # 示例
/// ```rust
/// use tokio::sync::broadcast;
/// use wallet_api::infrastructure::runtime::time::cancellable_sleep::cancellable_sleep;
///
/// async fn example(mut shutdown_rx: broadcast::Receiver<()>) {
///     // 尝试睡眠 500ms，但可以被 shutdown 信号中断
///     let slept = cancellable_sleep(Duration::from_millis(500), &mut shutdown_rx).await;
///     if slept {
///         println!("睡眠完成");
///     } else {
///         println!("被 shutdown 信号中断");
///     }
/// }
/// ```
pub async fn cancellable_sleep(duration: Duration, shutdown_rx: &mut Receiver<()>) -> bool {
    if duration.is_zero() {
        return true;
    }

    tokio::select! {
        biased;

        // shutdown 信号优先处理
        _ = shutdown_rx.recv() => {
            false
        },
        // 正常睡眠完成
        _ = sleep(duration) => {
            true
        },
    }
}
