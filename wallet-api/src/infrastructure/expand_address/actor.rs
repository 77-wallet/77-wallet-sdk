// actor.rs
#![allow(dead_code, deprecated)]

use tokio::sync::mpsc;

use crate::error::service::ServiceError;

/// ExpandActorMsg - 扩容Actor消息定义
///
/// 🔴 注意：此模块已被deprecated
/// - 不再承担系统推进职责
/// - 不再维护内存状态
/// - 所有状态由 Scanner 管理
/// - 此Actor仅作为消息接收器，不参与系统推进
#[deprecated(
    since = "0.1.0",
    note = "此模块已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
)]
pub enum ExpandActorMsg {
    /// 新扩容任务
    NewExpandTask {
        task_id: String,
        msg: crate::messaging::mqtt::topics::api_wallet::cmd::address_allock::AwmCmdAddrExpandMsg,
        reply: Option<oneshot::Sender<Result<(), ServiceError>>>,
    },

    /// 地址查询状态更新
    AddressInited { indices: Vec<i32> },

    /// 账户创建完成
    AccountCreated { indices: Vec<i32> },

    /// 通知地址已扩容
    NotifyAddressExpanded { batch_id: String },

    /// 任务失败
    JobFailed {
        phase: wallet_database::entities::expand_batch_item::ExpandItemStatus,
        indices: Vec<i32>,
        error: String,
    },

    /// 后端地址同步中
    BackendAddressSyncing,

    /// 后端地址同步完成
    BackendAddressSynced,
}

use tokio::sync::oneshot;

/// ExpandActorHandle - 扩容Actor句柄
///
/// 🔴 注意：此模块已被deprecated
/// - 不再承担系统推进职责
/// - 不再维护内存状态
/// - 所有状态由 Scanner 管理
/// - 此Actor仅作为消息接收器，不参与系统推进
#[deprecated(
    since = "0.1.0",
    note = "此模块已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
)]
pub struct ExpandActorHandle {
    sender: mpsc::Sender<ExpandActorMsg>,
}

impl ExpandActorHandle {
    /// 发送消息给Actor
    ///
    /// 🔴 注意：此方法已被deprecated
    /// - 不再承担系统推进职责
    /// - 所有状态由 Scanner 管理
    #[deprecated(
        since = "0.1.0",
        note = "此方法已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
    )]
    pub async fn send(&self, msg: ExpandActorMsg) -> Result<(), ServiceError> {
        self.sender.send(msg).await.map_err(|e| {
            ServiceError::System(crate::error::system::SystemError::ChannelSendFailed(
                e.to_string(),
            ))
        })
    }
}

/// ExpandActor - 扩容Actor
///
/// 🔴 核心驱动：
/// - 接收来自外部的扩容请求
/// - 处理扩容请求，创建扩容批次
/// - 定期扫描并推进扩容批次状态
/// - 失败时retry+backoff，停留在当前状态
/// - 派生batch状态
/// - recover机制：启动时立即执行一次扫描
///
/// 🔴 注意：此模块已被deprecated
/// - 不再承担系统推进职责
/// - 不再维护内存状态
/// - 所有状态由 Scanner 管理
/// - 此Actor仅作为消息接收器，不参与系统推进
#[deprecated(
    since = "0.1.0",
    note = "此模块已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
)]
pub(crate) struct ExpandActor {
    uid: String,
    chain: String,
}

impl ExpandActor {
    /// 创建新的ExpandActor
    ///
    /// 🔴 注意：此方法已被deprecated
    /// - 不再承担系统推进职责
    /// - 所有状态由 Scanner 管理
    #[deprecated(
        since = "0.1.0",
        note = "此方法已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
    )]
    pub fn new(uid: String, chain: String, _tx: mpsc::Sender<ExpandActorMsg>) -> ExpandActor {
        ExpandActor { uid, chain }
    }

    /// 运行Actor
    ///
    /// 🔴 注意：此方法已被deprecated
    /// - 不再承担系统推进职责
    /// - 所有状态由 Scanner 管理
    #[deprecated(
        since = "0.1.0",
        note = "此方法已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
    )]
    pub(crate) async fn run(
        self,
        mut rx: mpsc::Receiver<ExpandActorMsg>,
    ) -> Result<(), ServiceError> {
        tracing::info!(uid = %self.uid, chain = %self.chain, "expand actor starting");

        // 启动时，先执行一次状态扫描，恢复进度
        // self.recover().await?;

        // 1️⃣ 处理接收到的消息
        while let Some(msg) = rx.recv().await {
            match msg {
                ExpandActorMsg::NewExpandTask { task_id, msg: _, reply } => {
                    // 不再处理新扩容任务，由Scanner管理
                    tracing::info!(task_id = %task_id, uid = %self.uid, chain = %self.chain, "received new expand task, but ignored - all state is managed by Scanner");
                    if let Some(reply) = reply {
                        let _ = reply.send(Ok(()));
                    }
                }
                ExpandActorMsg::AddressInited { indices } => {
                    // 不再处理地址初始化事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, indices = ?indices, "received address inited event, but ignored - all state is managed by Scanner");
                }
                ExpandActorMsg::AccountCreated { indices } => {
                    // 不再处理账户创建事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, indices = ?indices, "received account created event, but ignored - all state is managed by Scanner");
                }
                ExpandActorMsg::NotifyAddressExpanded { batch_id } => {
                    // 不再处理通知事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, batch_id = %batch_id, "received notify address expanded event, but ignored - all state is managed by Scanner");
                }
                ExpandActorMsg::JobFailed { phase, indices, error } => {
                    // 不再处理任务失败事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, phase = ?phase, indices = ?indices, error = %error, "received job failed event, but ignored - all state is managed by Scanner");
                }
                ExpandActorMsg::BackendAddressSyncing => {
                    // 不再处理后端地址同步中事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, "received backend address syncing event, but ignored - all state is managed by Scanner");
                }
                ExpandActorMsg::BackendAddressSynced => {
                    // 不再处理后端地址同步完成事件，由Scanner管理
                    tracing::info!(uid = %self.uid, chain = %self.chain, "received backend address synced event, but ignored - all state is managed by Scanner");
                }
            }
        }

        tracing::info!(uid = %self.uid, chain = %self.chain, "expand actor exiting");

        Ok(())
    }

    /// 处理新扩容任务
    ///
    /// 🔴 注意：此方法已被deprecated
    /// - 不再承担系统推进职责
    /// - 所有状态由 Scanner 管理
    #[deprecated(
        since = "0.1.0",
        note = "此方法已被deprecated，所有状态由Scanner管理，Actor仅作为消息接收器"
    )]
    async fn handle_new_expand(
        &mut self,
        _msg: crate::messaging::mqtt::topics::api_wallet::cmd::address_allock::AwmCmdAddrExpandMsg,
    ) -> Result<(), ServiceError> {
        // 不再处理新扩容任务，由Scanner管理
        tracing::info!(uid = %self.uid, chain = %self.chain, "handling new expand task, but ignored - all state is managed by Scanner");
        Ok(())
    }
}
