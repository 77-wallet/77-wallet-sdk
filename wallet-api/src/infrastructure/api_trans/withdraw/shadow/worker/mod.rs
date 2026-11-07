// withdraw/shadow/worker/mod.rs
mod shadow_withdraw_worker;
mod side_effect_worker;

pub use shadow_withdraw_worker::ShadowWithdrawWorker;
pub use side_effect_worker::SideEffectWorker;

/// ShadowWithdrawWorker 命令
#[derive(Debug, Clone)]
pub enum ShadowWithdrawCommand {
    /// 写入审计展示用手续费预估快照
    EstimateFee(String),
    /// 评估资源闸门
    EvalResourceGate(String),
    /// 调用幂等的平台代理申请接口
    ApplyPlatformDelegate(String),
    /// 构建交易
    BuildTx(String),
    /// 广播交易
    Broadcast(String),
    /// 恢复交易
    Recover(String),
    /// 执行资源代理任务
    ExecuteResourceDelegation(String),
}

/// SideEffectWorker 命令
#[derive(Debug, Clone)]
pub enum SideEffectCommand {
    /// 发送交易 ACK
    SendTxAck(String),
    /// 发送交易结果 ACK
    SendTxResAck(String),
    /// 上传交易执行回执
    UploadTxExecReceipt(String),
    /// 发送资源任务结果 ACK
    SendResourceResultAck(String),
    /// 发送资源任务 ACK
    SendResourceTaskAck(String),
    /// 上传资源任务交易执行回执
    UploadResourceTxExecReceipt(String),
}
