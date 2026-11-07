#![allow(unused_imports)]

pub mod cache;
pub mod engine;
pub mod fact_snapshot;
pub mod stuck_monitor;

use wallet_database::entities::api_fee::ApiFeeEntity;

pub use cache::CachedDiagnoser;
pub use engine::{DiagnoseResult, diagnose_fee};
pub use stuck_monitor::{DiagnoseDecision, FeeStuckMonitor, maybe_log_stuck};

pub use crate::infrastructure::api_trans::diagnose_common::event::{
    DiagnoseMeta, DiagnoseSource, DiagnoseStage,
};

pub type DiagnoseEvent =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEvent<ApiFeeEntity>;
pub type DiagnoseEventSender =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventSender<ApiFeeEntity>;
pub type DiagnoseEventReceiver =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventReceiver<ApiFeeEntity>;

pub fn channel(capacity: usize) -> (DiagnoseEventSender, DiagnoseEventReceiver) {
    crate::infrastructure::api_trans::diagnose_common::event::channel(capacity)
}
