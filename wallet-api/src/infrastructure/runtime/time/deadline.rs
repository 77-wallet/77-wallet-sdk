use std::future::Future;
use tokio::time::{Duration, Timeout, timeout};

/// 为 future 添加超时限制
///
/// 避免直接使用 tokio::time::timeout，确保统一的时间工具使用
///
/// # 示例
/// ```rust
/// use wallet_api::infrastructure::runtime::time::deadline::with_timeout;
///
/// async fn example() {
///     let future = async {
///         // 一些耗时操作
///         tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
///         "完成"
///     };
///     
///     match with_timeout(tokio::time::Duration::from_millis(500), future).await {
///         Ok(result) => println!("操作完成: {}", result),
///         Err(_) => println!("操作超时"),
///     }
/// }
/// ```
pub fn with_timeout<F, T>(duration: Duration, future: F) -> Timeout<F>
where
    F: Future<Output = T>,
{
    timeout(duration, future)
}

/// 计算从现在开始的截止时间
///
/// # 示例
/// ```rust
/// use wallet_api::infrastructure::runtime::time::deadline::deadline_from_now;
///
/// fn example() {
///     let deadline = deadline_from_now(tokio::time::Duration::from_secs(5));
///     println!("截止时间: {:?}", deadline);
/// }
/// ```
pub fn deadline_from_now(duration: Duration) -> tokio::time::Instant {
    tokio::time::Instant::now() + duration
}
