use std::{sync::Arc, time::Instant};

/// 诊断来源
#[derive(Debug, Clone, Copy)]
pub enum DiagnoseSource {
    Advancer,
    ManualAdvance,
    PeriodicScan,
}

/// 诊断阶段（跨交易通用枚举）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnoseStage {
    OrderAck,
    Build,
    Broadcast,
    Recover,
    ResultAck,
    ServiceFeeUpload,
    TxFeeResAck,
    TxExecReceipt,
    Unknown,
}

/// 诊断元数据（不可变）
#[derive(Debug, Clone)]
pub struct DiagnoseMeta {
    pub trade_no: Arc<str>,
    pub source: DiagnoseSource,
    pub stage: DiagnoseStage,
    pub ts: Instant,
}

impl DiagnoseMeta {
    pub fn new(
        trade_no: impl Into<Arc<str>>,
        source: DiagnoseSource,
        stage: DiagnoseStage,
    ) -> Self {
        Self { trade_no: trade_no.into(), source, stage, ts: Instant::now() }
    }
}

/// 诊断事件（泛型载荷）
#[derive(Debug)]
pub enum DiagnoseEvent<T> {
    /// 无推进时的诊断事件
    NoAdvancement { meta: DiagnoseMeta, entity: T },
    /// 周期性扫描时的诊断事件
    PeriodicScan { meta: DiagnoseMeta, entity: T },
    /// 手动触发的诊断事件
    ManualDiagnose { meta: DiagnoseMeta, entity: T, extra: String },
    /// 意图分发失败时的诊断事件
    IntentDispatchFailed { meta: DiagnoseMeta },
}

/// 诊断事件发送器
pub type DiagnoseEventSender<T> = tokio::sync::mpsc::Sender<DiagnoseEvent<T>>;

/// 诊断事件接收器
pub type DiagnoseEventReceiver<T> = tokio::sync::mpsc::Receiver<DiagnoseEvent<T>>;

/// 创建诊断事件通道
pub fn channel<T>(capacity: usize) -> (DiagnoseEventSender<T>, DiagnoseEventReceiver<T>) {
    tokio::sync::mpsc::channel(capacity)
}
