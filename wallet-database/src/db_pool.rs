use sqlx::Sqlite;
use std::sync::atomic::{AtomicU64, Ordering};
mod writer_diagnostics;
use writer_diagnostics::{DiagnosticChannel, Probe};
pub use writer_diagnostics::{WriterDiagnosticEvent, WriterGuard};
static NEXT_POOL_ID: AtomicU64 = AtomicU64::new(1);

pub type DbPool = std::sync::Arc<sqlx::Pool<Sqlite>>;

#[derive(Debug)]
struct SplitDbPool {
    read_pool: DbPool,
    write_pool: DbPool,
    write_gate: std::sync::Arc<tokio::sync::Mutex<()>>,
    pool_id: u64,
    next_lock_id: AtomicU64,
    diagnostics: std::sync::RwLock<Option<std::sync::Arc<DiagnosticChannel>>>,
}

#[derive(Clone, Debug)]
pub struct CoreDbPool(std::sync::Arc<SplitDbPool>);

#[derive(Clone, Debug)]
pub struct TaskDbPool(std::sync::Arc<SplitDbPool>);

#[derive(Clone, Debug)]
pub struct ApiTransactionDbPool(std::sync::Arc<SplitDbPool>);

#[derive(Clone, Debug)]
pub struct ApiWalletDbPool(std::sync::Arc<SplitDbPool>);

#[derive(Clone, Debug)]
pub struct ApiResourceDbPool(std::sync::Arc<SplitDbPool>);

macro_rules! impl_split_pool_wrapper {
    ($name:ident) => {
        impl $name {
            // Backward compatible constructor: read/write share the same pool.
            pub fn new(pool: DbPool) -> Self {
                Self::new_split(pool.clone(), pool)
            }

            pub fn new_split(read_pool: DbPool, write_pool: DbPool) -> Self {
                Self(std::sync::Arc::new(SplitDbPool {
                    read_pool,
                    write_pool,
                    write_gate: std::sync::Arc::new(tokio::sync::Mutex::new(())),
                    pool_id: NEXT_POOL_ID.fetch_add(1, Ordering::Relaxed),
                    next_lock_id: AtomicU64::new(1),
                    diagnostics: std::sync::RwLock::new(None),
                }))
            }

            pub fn as_ref(&self) -> &sqlx::Pool<Sqlite> {
                self.0.read_pool.as_ref()
            }

            pub fn read_ref(&self) -> &sqlx::Pool<Sqlite> {
                self.0.read_pool.as_ref()
            }

            pub fn write_ref(&self) -> &sqlx::Pool<Sqlite> {
                self.0.write_pool.as_ref()
            }

            pub fn read_pool(&self) -> DbPool {
                self.0.read_pool.clone()
            }

            pub fn write_pool(&self) -> DbPool {
                self.0.write_pool.clone()
            }

            pub async fn lock_write(&self) -> WriterGuard {
                self.lock_write_with_metric("unspecified").await
            }

            pub async fn lock_write_with_metric(
                &self,
                op: &str,
            ) -> WriterGuard {
                let channel = self.0.diagnostics.try_read().ok().and_then(|c| c.clone());
                let probe = Probe::new(channel, self.0.pool_id,
                    self.0.next_lock_id.fetch_add(1, Ordering::Relaxed), op);
                let guard = probe.acquired(self.0.write_gate.clone().lock_owned().await);
                let wait_ms = guard.wait_ms();
                if wait_ms >= 1_000.0 {
                    tracing::warn!(
                        metric = "writer_gate_wait_ms",
                        db_pool = stringify!($name),
                        op = %op,
                        value_ms = %wait_ms,
                        "writer gate wait was slow"
                    );
                } else if wait_ms >= 100.0 {
                    tracing::info!(
                        metric = "writer_gate_wait_ms",
                        db_pool = stringify!($name),
                        op = %op,
                        value_ms = %wait_ms,
                        "writer gate wait was noticeable"
                    );
                } else {
                    tracing::debug!(
                        metric = "writer_gate_wait_ms",
                        db_pool = stringify!($name),
                        op = %op,
                        value_ms = %wait_ms,
                        "writer gate acquired"
                    );
                }
                guard
            }

            // Compatibility helper used by legacy transaction code paths.
            pub fn into_inner(&self) -> DbPool {
                self.0.write_pool.clone()
            }
        }
    };
}

impl_split_pool_wrapper!(CoreDbPool);
impl_split_pool_wrapper!(TaskDbPool);
impl_split_pool_wrapper!(ApiTransactionDbPool);
impl_split_pool_wrapper!(ApiWalletDbPool);
impl_split_pool_wrapper!(ApiResourceDbPool);

#[cfg(test)]
#[path = "db_pool/writer_diagnostics_tests.rs"]
mod writer_diagnostics_tests;

impl ApiWalletDbPool {
    /// Install once at Context initialization. Delivery never waits on the consumer.
    pub fn set_writer_diagnostics(&self, sender: tokio::sync::mpsc::Sender<WriterDiagnosticEvent>) {
        if let Ok(mut channel) = self.0.diagnostics.write() {
            *channel = Some(std::sync::Arc::new(DiagnosticChannel::new(sender)));
        }
    }
}
