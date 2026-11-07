use wallet_database::entities::api_collect::ApiCollectEntity;

pub use crate::infrastructure::api_trans::diagnose_common::event::{
    DiagnoseMeta, DiagnoseSource, DiagnoseStage,
};

pub type DiagnoseEvent =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEvent<ApiCollectEntity>;
pub type DiagnoseEventSender =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventSender<ApiCollectEntity>;
pub type DiagnoseEventReceiver =
    crate::infrastructure::api_trans::diagnose_common::event::DiagnoseEventReceiver<
        ApiCollectEntity,
    >;

pub fn channel(capacity: usize) -> (DiagnoseEventSender, DiagnoseEventReceiver) {
    crate::infrastructure::api_trans::diagnose_common::event::channel(capacity)
}
