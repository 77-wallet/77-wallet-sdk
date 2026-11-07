use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{error, info};

/// nonce 相关的 metrics
pub struct NonceMetrics {
    /// bootstrap 次数
    pub nonce_bootstrap_total: AtomicU64,
    /// repair 次数
    pub nonce_repair_total: AtomicU64,
    /// reconcile 次数
    pub nonce_reconcile_total: AtomicU64,
    /// nonce 漂移量
    pub nonce_drift: AtomicU64,
    /// nonce 错误次数
    pub nonce_error_total: AtomicU64,
}

impl NonceMetrics {
    pub fn new() -> Self {
        Self {
            nonce_bootstrap_total: AtomicU64::new(0),
            nonce_repair_total: AtomicU64::new(0),
            nonce_reconcile_total: AtomicU64::new(0),
            nonce_drift: AtomicU64::new(0),
            nonce_error_total: AtomicU64::new(0),
        }
    }

    /// 记录 bootstrap 操作
    pub fn record_bootstrap(&self, chain: &str, source: &str) {
        self.nonce_bootstrap_total.fetch_add(1, Ordering::Relaxed);
        info!(chain = %chain, source = %source, nonce_bootstrap_total = %self.nonce_bootstrap_total.load(Ordering::Relaxed), "Nonce bootstrap recorded");
    }

    /// 记录 repair 操作
    pub fn record_repair(&self, error_type: &str) {
        self.nonce_repair_total.fetch_add(1, Ordering::Relaxed);
        info!(error_type = %error_type, nonce_repair_total = %self.nonce_repair_total.load(Ordering::Relaxed), "Nonce repair recorded");
    }

    /// 记录 reconcile 操作
    pub fn record_reconcile(&self, address_type: &str) {
        self.nonce_reconcile_total.fetch_add(1, Ordering::Relaxed);
        info!(address_type = %address_type, nonce_reconcile_total = %self.nonce_reconcile_total.load(Ordering::Relaxed), "Nonce reconcile recorded");
    }

    /// 记录 nonce 漂移
    pub fn record_drift(&self, address: &str, chain: &str, drift: u64) {
        self.nonce_drift.fetch_add(drift, Ordering::Relaxed);
        error!(address = %address, chain = %chain, drift = %drift, nonce_drift = %self.nonce_drift.load(Ordering::Relaxed), "Nonce drift recorded");
    }

    /// 记录 nonce 错误
    pub fn record_error(&self, error_type: &str) {
        self.nonce_error_total.fetch_add(1, Ordering::Relaxed);
        error!(error_type = %error_type, nonce_error_total = %self.nonce_error_total.load(Ordering::Relaxed), "Nonce error recorded");
    }
}

// 全局 metrics 实例
use once_cell::sync::OnceCell;
use std::sync::Arc;

static NONCE_METRICS: OnceCell<Arc<NonceMetrics>> = OnceCell::new();

pub fn get_nonce_metrics() -> Arc<NonceMetrics> {
    NONCE_METRICS.get_or_init(|| Arc::new(NonceMetrics::new())).clone()
}

pub fn init_nonce_metrics() {
    NONCE_METRICS.get_or_init(|| Arc::new(NonceMetrics::new()));
}
