// facade.rs
#![allow(deprecated)]

/// 决定能不能进入扩容系统
use crate::error::service::ServiceError;

/// ExpandAddressFacade - 扩容地址系统门面
///
/// 🔴 注意：此模块已被deprecated
/// - 不再承担系统推进职责
/// - 不再维护内存状态
/// - 所有状态由 Scanner 管理
/// - 此Facade仅作为向后兼容接口，不参与系统推进
#[deprecated(
    since = "0.1.0",
    note = "此模块已被deprecated，所有状态由Scanner管理，Facade仅作为向后兼容接口"
)]
pub struct ExpandAddressFacade;

impl ExpandAddressFacade {
    // ===== Helper APIs for external use =====

    /// Submit a new expand task to the actor system
    /// 注意：当前实现中，此方法仅用于向后兼容，实际功能由Scanner驱动
    ///
    /// 🔴 注意：此方法已被deprecated
    /// - 不再承担系统推进职责
    /// - 所有状态由 Scanner 管理
    #[deprecated(
        since = "0.1.0",
        note = "此方法已被deprecated，所有状态由Scanner管理，Facade仅作为向后兼容接口"
    )]
    pub async fn submit_expand_task(
        _task_id: String,
        _msg: crate::messaging::mqtt::topics::api_wallet::cmd::address_allock::AwmCmdAddrExpandMsg,
    ) -> Result<(), ServiceError> {
        // 此方法仅用于向后兼容，实际扩容任务由Scanner驱动
        // Scanner会定期扫描并处理所有需要扩容的批次
        tracing::debug!("submit_expand_task called - actual processing is handled by Scanner");
        Ok(())
    }
}
