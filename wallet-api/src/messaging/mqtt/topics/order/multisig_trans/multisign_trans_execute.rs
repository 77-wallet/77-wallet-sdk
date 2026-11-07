use wallet_database::{
    CoreDbPool, entities::multisig_queue::MultisigQueueStatus,
    repositories::multisig_queue::MultisigQueueRepo,
};

use crate::messaging::notify::{FrontendNotifyEvent, event::NotifyEvent};

// 执行多签交易
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiSignTransExecute {
    pub withdraw_id: String,
}

impl MultiSignTransExecute {
    pub(crate) fn _name(&self) -> String {
        "MULTI_SIGN_TRANS_EXECUTE".to_string()
    }
}

// 当某一个参与放执行交易后同步其他参与放交易的状态
impl MultiSignTransExecute {
    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        // 并发可能导致查询不出来结果(事件的先后顺序不一致，导致错误)
        MultisigQueueRepo::update_status(
            &self.withdraw_id,
            MultisigQueueStatus::InConfirmation,
            &core_pool,
        )
        .await?;

        // 发送一个事件去让前端更新全局消息
        let data = NotifyEvent::MultiSignTransExecute;
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub async fn exec(
        &self,
        _msg_id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.exec_with_ctx(_msg_id, ctx).await
    }
}
