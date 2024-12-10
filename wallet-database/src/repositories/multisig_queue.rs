use super::{ResourcesRepo, TransactionTrait};
use crate::{
    dao::{
        multisig_account::MultisigAccountDaoV1, multisig_member::MultisigMemberDaoV1,
        multisig_queue::MultisigQueueDaoV1, multisig_signatures::MultisigSignatureDaoV1,
    },
    entities::{
        multisig_account::MultisigAccountEntity,
        multisig_member::MultisigMemberEntities,
        multisig_queue::{
            fail_reason::SIGN_FAILED, MemberSignedResult, MultisigQueueData, MultisigQueueEntity,
            MultisigQueueStatus, MultisigQueueWithAccountEntity, NewMultisigQueueEntity,
        },
        multisig_signatures::{
            MultisigSignatureEntities, MultisigSignatureStatus, NewSignatureEntity,
        },
    },
    pagination::Pagination,
};
use sqlx::{Pool, Sqlite};
use std::sync::Arc;

pub struct MultisigQueueRepo {
    repo: ResourcesRepo,
}
impl MultisigQueueRepo {
    pub fn new(db_pool: crate::DbPool) -> Self {
        Self {
            repo: ResourcesRepo::new(db_pool),
        }
    }
}

impl MultisigQueueRepo {
    pub async fn create_queue_with_sign(
        pool: Arc<Pool<Sqlite>>,
        params: &mut NewMultisigQueueEntity,
    ) -> Result<MultisigQueueEntity, crate::Error> {
        // get database transaction
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // create multisig queue
        let res = MultisigQueueDaoV1::create_queue(params, tx.as_mut()).await?;

        // if signatures is not empty insert signatures
        if !params.signatures.is_empty() {
            for signature in &mut params.signatures {
                signature.queue_id = res.id.clone();

                let exists = MultisigSignatureDaoV1::find_by_address_and_queue_id(
                    &res.id,
                    &signature.address,
                    tx.as_mut(),
                )
                .await?;
                match exists {
                    Some(_) => {
                        MultisigSignatureDaoV1::update_status(signature, tx.as_mut()).await?
                    }
                    None => {
                        MultisigSignatureDaoV1::create_signature(signature, tx.as_mut()).await?
                    }
                }
            }
        }

        tx.commit()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        Ok(res)
    }

    pub async fn multisig_account(
        &self,
        address: &str,
    ) -> Result<Option<MultisigAccountEntity>, crate::Error> {
        let pool = self.repo.pool_ref();
        let conditions = vec![("address", address)];
        Ok(MultisigAccountDaoV1::find_by_conditions(conditions, pool.as_ref()).await?)
    }

    pub async fn find_by_id_with_account(
        &self,
        id: &str,
    ) -> Result<Option<MultisigQueueWithAccountEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::find_by_id_with_account(id, &*self.repo.db_pool).await?)
    }

    pub async fn queue_list(
        &self,
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigQueueWithAccountEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::queue_list(
            from,
            chain_code,
            status,
            page,
            page_size,
            self.repo.pool(),
        )
        .await?)
    }

    pub async fn queue_list_info(
        &self,
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<MultisigQueueWithAccountEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::queue_list(
            from,
            chain_code,
            status,
            page,
            page_size,
            self.repo.pool(),
        )
        .await?)
    }

    pub async fn get_signed_count(&self, id: &str) -> Result<i64, crate::Error> {
        Ok(MultisigSignatureDaoV1::get_signed_count(id, &*self.repo.db_pool).await?)
    }

    pub async fn member_signed_result(
        account_id: &str,
        queue_id: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MemberSignedResult>, crate::Error> {
        let mut result = vec![];

        let mut member = MultisigMemberDaoV1::find_records_by_id(account_id, pool.as_ref()).await?;

        for item in member.0.iter_mut() {
            let mut sign_result = MemberSignedResult::new(&item.name, &item.address, item.is_self);

            // 获取签名的结果
            let sign = MultisigSignatureDaoV1::find_by_address_and_queue_id(
                queue_id,
                &item.address,
                pool.as_ref(),
            )
            .await?;
            if let Some(sign) = sign {
                sign_result.singed = sign.status;
                sign_result.signature = sign.signature;
            }
            result.push(sign_result);
        }
        Ok(result)
    }

    // get multisig queue by id
    pub async fn queue_by_id(
        &self,
        queue_id: &str,
    ) -> Result<Option<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::find_by_id(queue_id, &*self.repo.db_pool).await?)
    }

    pub async fn create_or_update_sign(
        &self,
        params: &NewSignatureEntity,
    ) -> Result<(), crate::Error> {
        Ok(
            MultisigSignatureDaoV1::create_or_update(params, self.repo.get_db_pool().clone())
                .await?,
        )
    }

    pub async fn sync_sign_status(
        queue_id: &str,
        account_id: &str,
        threshold: i32,
        pool: crate::DbPool,
    ) -> Result<(), crate::Error> {
        // fetch all member sign result
        let signature_result =
            MultisigQueueRepo::member_signed_result(account_id, queue_id, pool.clone()).await?;

        // first check self sign completed
        let mut status = MultisigQueueStatus::HasSignature;
        let mut remain_num = 0;
        let mut apporved_num = 0;

        for sign in signature_result {
            if sign.is_self == 1 && sign.singed == MultisigSignatureStatus::UnSigned.to_i8() {
                status = MultisigQueueStatus::PendingSignature;
            }

            // 剩余未签名的数量
            if sign.singed == MultisigSignatureStatus::UnSigned.to_i8() {
                remain_num += 1;
            }

            // 已经同意签名的数量
            if sign.singed == MultisigSignatureStatus::Approved.to_i8() {
                apporved_num += 1;
            }
        }

        if apporved_num >= threshold {
            status = MultisigQueueStatus::PendingExecution;
        } else {
            // 如果剩余的未签名数量 + 同意签名的数量 < 阈值 则这个交易队列失败
            if remain_num + apporved_num < threshold {
                return Ok(
                    MultisigQueueDaoV1::update_fail(queue_id, SIGN_FAILED, pool.as_ref()).await?,
                );
            }
        }

        MultisigQueueDaoV1::update_status(queue_id, status, pool.as_ref()).await?;

        Ok(())
    }

    pub async fn self_member_account_id(
        &self,
        id: &str,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::get_self_by_id(id, &*self.repo.db_pool).await?)
    }

    pub async fn get_signed_list(
        &self,
        queue_id: &str,
    ) -> Result<MultisigSignatureEntities, crate::Error> {
        Ok(MultisigSignatureDaoV1::get_signed_list(queue_id, &*self.repo.db_pool).await?)
    }

    pub async fn update_queue_status(
        &self,
        queue_id: &str,
        status: MultisigQueueStatus,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_status(queue_id, status, &*self.repo.db_pool).await?)
    }

    pub async fn update_status_and_hash(
        &self,
        queue_id: &str,
        status: MultisigQueueStatus,
        tx_hash: &str,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_status_and_tx_hash(
            queue_id,
            status,
            tx_hash,
            &*self.repo.db_pool,
        )
        .await?)
    }

    pub async fn multisig_queue_data(
        queue_id: &str,
        pool: crate::DbPool,
    ) -> Result<MultisigQueueData, crate::Error> {
        let queue = MultisigQueueDaoV1::find_by_id(queue_id, pool.as_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;

        let signatures = MultisigSignatureDaoV1::find_by_queue_id(queue_id, pool).await?;

        Ok(MultisigQueueData::new(
            queue,
            MultisigSignatureEntities(signatures),
        ))
    }

    pub async fn ongoing_queue(
        &self,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<MultisigQueueEntity>, crate::Error> {
        let queue =
            MultisigQueueDaoV1::ongoing_queue(self.repo.db_pool.as_ref(), chain_code, address)
                .await?;
        Ok(queue)
    }
}
