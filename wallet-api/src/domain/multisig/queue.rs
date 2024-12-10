use crate::domain::{self, bill::BillDomain, multisig::MultisigDomain};
use std::{collections::HashSet, sync::Arc};
use wallet_database::{
    dao::{multisig_member::MultisigMemberDaoV1, multisig_queue::MultisigQueueDaoV1},
    entities::{
        multisig_queue::{
            MultisigQueueData, MultisigQueueEntity, MultisigQueueStatus, NewMultisigQueueEntity,
        },
        multisig_signatures::NewSignatureEntity,
        wallet::WalletEntity,
    },
    repositories::multisig_queue::MultisigQueueRepo,
};

pub struct MultisigQueueDomain;
impl MultisigQueueDomain {
    pub fn validate_queue(
        queue: &MultisigQueueEntity,
        execute: bool,
    ) -> Result<(), crate::ServiceError> {
        // check queue is expired
        let time = sqlx::types::chrono::Utc::now().timestamp();
        if queue.expiration < time {
            return Err(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::Expired,
            ))?;
        }

        // check status
        if queue.status == MultisigQueueStatus::InConfirmation.to_i8() {
            return Err(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::AlreadyExecuted,
            ))?;
        }

        if queue.status == MultisigQueueStatus::Fail.to_i8() {
            return Err(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::FailedQueue,
            ))?;
        }

        // execute multisig tx need check status
        if execute && queue.status != MultisigQueueStatus::PendingExecution.to_i8() {
            return Err(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::NotPendingExecStatus,
            ))?;
        }

        Ok(())
    }

    pub async fn recover_all_uid_queue_data() -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let uid_list = WalletEntity::uid_list(&*pool)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect();

        let account_ids = MultisigMemberDaoV1::list_by_uids(&uid_list, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?
            .0
            .into_iter()
            .map(|member| member.account_id)
            .collect::<HashSet<String>>();

        let account_ids_vec: Vec<String> = account_ids.into_iter().collect();
        let queue = MultisigQueueDaoV1::list_by_account_ids(&account_ids_vec, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        let raw_time = queue.map(|q| q.created_at.to_string());

        for uid in uid_list {
            Self::recover_queue_data_with_raw_time(&uid, raw_time.clone()).await?;
        }

        Ok(())
    }

    pub async fn recover_all_queue_data(uid: &str) -> Result<(), crate::ServiceError> {
        Self::recover_queue_data_with_raw_time(uid, None).await?;
        Ok(())
    }

    pub async fn recover_queue_data(uid: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let queue = MultisigQueueDaoV1::get_latest(&*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        let raw_time = if let Some(queue) = queue {
            Some(queue.created_at.to_string())
        } else {
            None
        };

        Self::recover_queue_data_with_raw_time(uid, raw_time).await?;

        Ok(())
    }

    pub(crate) async fn recover_queue_data_with_raw_time(
        uid: &str,
        raw_time: Option<String>,
    ) -> Result<(), crate::ServiceError> {
        let backend = crate::manager::Context::get_global_backend_api()?;
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let req =
            wallet_transport_backend::request::FindAddressRawDataReq::new_trans(uid, raw_time);
        let data = backend.address_find_address_raw_data(req).await?;

        let list = data.list;
        for item in list {
            if let Some(raw_data) = item.raw_data {
                if let Ok(multisig_queue_data) = MultisigQueueData::from_string(&raw_data) {
                    let que_id = multisig_queue_data.queue.id.clone();
                    if let Err(e) = Self::insert(pool.clone(), multisig_queue_data).await {
                        tracing::error!("revover queue data error: {} queue_id = {}", e, que_id);
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn insert(
        pool: Arc<sqlx::Pool<sqlx::Sqlite>>,
        data: MultisigQueueData,
    ) -> Result<(), crate::ServiceError> {
        let MultisigQueueData { queue, signatures } = data;
        let id = queue.id.clone();
        let account_id = queue.account_id.clone();

        // 构建交易数据
        let mut params = NewMultisigQueueEntity::from(queue).check_expiration();

        // TODO 后续交给后端处理
        // 如果有交易的hash 去链上判读交易的执行状态
        if !params.tx_hash.is_empty() {
            let tx =
                domain::bill::BillDomain::get_onchain_bill(&params.tx_hash, &params.chain_code)
                    .await?;
            if let Some(tx) = tx {
                if tx.status == 2 {
                    params.status = MultisigQueueStatus::Success;
                } else {
                    params.status = MultisigQueueStatus::Fail;
                }
            }
        }

        for sig in signatures.0 {
            let signature = NewSignatureEntity::try_from(sig)?;
            params = params.with_signatures(signature);
        }

        MultisigQueueRepo::create_queue_with_sign(pool.clone(), &mut params).await?;

        match params.status {
            MultisigQueueStatus::InConfirmation
            | MultisigQueueStatus::Success
            | MultisigQueueStatus::Fail => {}
            _ => {
                let multisig_account =
                    MultisigDomain::account_by_id(&account_id, pool.clone()).await?;

                MultisigQueueRepo::sync_sign_status(
                    &id,
                    &account_id,
                    multisig_account.threshold,
                    pool,
                )
                .await?;
            }
        }

        Ok(())
    }

    // 同步交易的状态
    pub async fn sync_queue_status(queue_id: &str) -> Result<(), crate::ServiceError> {
        // 获取数据库连接池
        let pool = crate::Context::get_global_sqlite_pool()?;

        // 查询队列信息
        let queue = MultisigQueueDaoV1::find_by_id(queue_id, pool.as_ref())
            .await
            .map_err(|e| crate::ServiceError::Database(e.into()))?;

        let Some(queue) = queue else {
            return Ok(());
        };

        // 仅处理状态为 InConfirmation 且有 tx_hash 的队列
        if queue.status != MultisigQueueStatus::InConfirmation.to_i8() || queue.tx_hash.is_empty() {
            return Ok(());
        }

        // 获取链上交易状态
        if let Some(rs) = BillDomain::get_onchain_bill(&queue.tx_hash, &queue.chain_code).await? {
            // 更新状态：2 为成功，否则为失败
            let tx_status = if rs.status == 2 {
                MultisigQueueStatus::Success
            } else {
                MultisigQueueStatus::Fail
            };

            // 更新数据库状态
            MultisigQueueDaoV1::update_status(queue_id, tx_status, pool.as_ref())
                .await
                .map_err(|e| crate::ServiceError::Database(e.into()))?;
        }

        Ok(())
    }
}
