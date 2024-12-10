use wallet_database::{
    dao::multisig_queue::MultisigQueueDaoV1, entities::multisig_queue::fail_reason,
};

// 取消多签交易
use super::MultiSignTransCancel;

impl MultiSignTransCancel {
    pub async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;

        MultisigQueueDaoV1::update_fail(&self.withdraw_id, fail_reason::CANCEL, pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?;

        Ok(())
    }
}
