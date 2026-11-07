use tokio::sync::mpsc;

/// ExpandEvent - 扩容系统事件类型
///
/// ExpandEvent is a best-effort wake-up hint.
/// Events may be dropped, duplicated, or delayed.
/// Correctness MUST NOT depend on event delivery.
#[derive(Debug, Clone)]
pub enum ExpandEvent {
    /// 触发一次扫描的事件提示
    ///
    /// - best-effort wake-up signal
    /// - may be dropped / duplicated
    /// - receiver must be idempotent
    /// - MUST NOT carry business semantics
    HintScan,
}

/// 事件发送器类型
pub type ExpandEventSender = mpsc::Sender<ExpandEvent>;

/// 事件接收器类型
pub type ExpandEventReceiver = mpsc::Receiver<ExpandEvent>;

/// 创建事件通道
///
/// bounded channel to prevent unbounded memory growth
/// overflow is acceptable because events are only hints
pub fn channel() -> (ExpandEventSender, ExpandEventReceiver) {
    mpsc::channel(100)
}
