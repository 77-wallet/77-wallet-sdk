#![allow(unused_imports)]

pub mod cache;
pub mod engine;
pub mod fact_snapshot;
pub mod stuck_monitor;

use wallet_database::entities::api_withdraw::ApiWithdrawEntity;

pub use cache::CachedDiagnoser;
pub use engine::{DiagnoseResult, diagnose_withdraw};
pub use stuck_monitor::{DiagnoseDecision, WithdrawStuckMonitor, maybe_log_stuck};

pub use crate::infrastructure::api_trans::diagnose_common::event::{
    DiagnoseMeta, DiagnoseSource, DiagnoseStage,
};

pub type DiagnoseEvent =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEvent<ApiWithdrawEntity>;
pub type DiagnoseEventSender =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventSender<
        ApiWithdrawEntity,
    >;
pub type DiagnoseEventReceiver =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventReceiver<
        ApiWithdrawEntity,
    >;

pub fn channel(capacity: usize) -> (DiagnoseEventSender, DiagnoseEventReceiver) {
    crate::infrastructure::api_trans::diagnose_common::event::channel(capacity)
}
