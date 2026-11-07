//! Best-effort writer ownership telemetry. No callbacks, awaits or tracing in emission/Drop.
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{OwnedMutexGuard, mpsc::Sender};

#[derive(Debug, Clone)]
pub struct WriterDiagnosticEvent {
    pub pool_id: u64,
    pub lock_id: u64,
    pub op: &'static str,
    pub event: &'static str,
    pub time_ms: u64,
    pub wait_ms: Option<f64>,
    pub held_ms: Option<f64>,
    pub dropped_events: u64,
}
#[derive(Debug)]
pub(super) struct DiagnosticChannel {
    pub sender: Sender<WriterDiagnosticEvent>,
    dropped: AtomicU64,
}
impl DiagnosticChannel {
    pub fn new(sender: Sender<WriterDiagnosticEvent>) -> Self {
        Self { sender, dropped: AtomicU64::new(0) }
    }
}
#[derive(Debug)]
pub(super) struct Probe {
    channel: Option<Arc<DiagnosticChannel>>,
    pool_id: u64,
    lock_id: u64,
    op: &'static str,
    started: Instant,
    waiting: bool,
}
impl Probe {
    pub fn new(
        channel: Option<Arc<DiagnosticChannel>>,
        pool_id: u64,
        lock_id: u64,
        op: &str,
    ) -> Self {
        let probe = Self {
            channel,
            pool_id,
            lock_id,
            op: safe_op(op),
            started: Instant::now(),
            waiting: true,
        };
        probe.emit("wait", None, None);
        probe
    }
    fn emit(&self, event: &'static str, wait_ms: Option<f64>, held_ms: Option<f64>) {
        let Some(channel) = &self.channel else { return };
        let item = WriterDiagnosticEvent {
            pool_id: self.pool_id,
            lock_id: self.lock_id,
            op: self.op,
            event,
            time_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
                as u64,
            wait_ms,
            held_ms,
            dropped_events: channel.dropped.load(Ordering::Relaxed),
        };
        if channel.sender.try_send(item).is_err() {
            channel.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn acquired(mut self, guard: OwnedMutexGuard<()>) -> WriterGuard {
        let acquired = Instant::now();
        let wait_ms = acquired.duration_since(self.started).as_secs_f64() * 1000.0;
        self.waiting = false;
        self.emit("acquired", Some(wait_ms), None);
        WriterGuard { guard: Some(guard), probe: self, acquired, wait_ms }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if self.waiting {
            self.emit("cancelled", Some(self.started.elapsed().as_secs_f64() * 1000.0), None);
        }
    }
}
/// Owns the same Tokio gate as before; cancellation/unwind releases it through Drop.
#[derive(Debug)]
pub struct WriterGuard {
    guard: Option<OwnedMutexGuard<()>>,
    probe: Probe,
    acquired: Instant,
    wait_ms: f64,
}
impl WriterGuard {
    pub(super) fn wait_ms(&self) -> f64 {
        self.wait_ms
    }
}
impl Drop for WriterGuard {
    fn drop(&mut self) {
        let held_ms = self.acquired.elapsed().as_secs_f64() * 1000.0;
        drop(self.guard.take());
        self.probe.emit("released", Some(self.wait_ms), Some(held_ms));
    }
}
// Public callers may pass arbitrary strings. Only known static operation labels leave this crate.
fn safe_op(op: &str) -> &'static str {
    match op {
        "unspecified" => "unspecified",
        "upsert_api_wallet" => "upsert_api_wallet",
        "edit_api_wallet_name" => "edit_api_wallet_name",
        "update_merchant_id" => "update_merchant_id",
        "update_app_id" => "update_app_id",
        "update_wallet_sn" => "update_wallet_sn",
        "update_seed_and_phrase" => "update_seed_and_phrase",
        "unbind_wallet_uid" => "unbind_wallet_uid",
        "clear_binding_address" => "clear_binding_address",
        "clear_binding_relation" => "clear_binding_relation",
        "mark_wallet_init" => "mark_wallet_init",
        "update_api_wallet_import_stage" => "update_api_wallet_import_stage",
        "delete_api_wallets" => "delete_api_wallets",
        "delete_all_api_wallets" => "delete_all_api_wallets",
        "bind_withdraw_relation" => "bind_withdraw_relation",
        "upsert_account_multi" => "upsert_account_multi",
        "mark_as_used" => "mark_as_used",
        "init_account" => "init_account",
        "init_many_accounts" => "init_many_accounts",
        "expand_account" => "expand_account",
        "delete_account" => "delete_account",
        "api_coin_metadata" => "api_coin_metadata",
        "batch_update_balance" => "batch_update_balance",
        "begin_warning_config_refresh" => "begin_warning_config_refresh",
        "delete_api_wallet_data" => "delete_api_wallet_data",
        "device_protection_empty_format" => "device_protection_empty_format",
        "device_protection_file_migration" => "device_protection_file_migration",
        "device_protection_initialization" => "device_protection_initialization",
        "fail_on_terminal_nonce_gap" => "fail_on_terminal_nonce_gap",
        "insert_missing_assets_multi" => "insert_missing_assets_multi",
        "mark_read_for_withdrawal_wallets" => "mark_read_for_withdrawal_wallets",
        "ordinary_default_assets" => "ordinary_default_assets",
        "plan_expand_items" => "plan_expand_items",
        "publish_warning_config" => "publish_warning_config",
        "replace_collect_strategy" => "replace_collect_strategy",
        "replace_withdraw_strategy" => "replace_withdraw_strategy",
        "reserve_signed_withdraw" => "reserve_signed_withdraw",
        "upsert_assets_multi" => "upsert_assets_multi",
        "upsert_assets_multi_update_balance" => "upsert_assets_multi_update_balance",
        _ => "other",
    }
}
