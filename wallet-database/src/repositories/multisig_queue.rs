use crate::{
    CoreDbPool,
    dao::{
        multisig_account::MultisigAccountDaoV1, multisig_member::MultisigMemberDaoV1,
        multisig_queue::MultisigQueueDaoV1, multisig_signatures::MultisigSignatureDaoV1,
        permission::PermissionDao, permission_user::PermissionUserDao,
    },
    entities::{
        multisig_member::MultisigMemberEntities,
        multisig_queue::{
            MemberSignedResult, MultisigQueueData, MultisigQueueEntity, MultisigQueueSimpleEntity,
            MultisigQueueStatus, NewMultisigQueueEntity, fail_reason::SIGN_FAILED,
        },
        multisig_signatures::{
            MultisigSignatureEntities, MultisigSignatureEntity, MultisigSignatureStatus,
            NewSignatureEntity,
        },
        permission_user::PermissionUserEntity,
    },
    pagination::Pagination,
};
use once_cell::sync::Lazy;
use sqlx::{Executor, Pool, Sqlite};
use tokio::sync::Mutex;

static CREATE_QUEUE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub struct MultisigQueueRepo;
impl MultisigQueueRepo {
    pub fn build_queue_from_entity(queue: MultisigQueueEntity) -> NewMultisigQueueEntity {
        NewMultisigQueueEntity::from(queue)
    }

    pub fn build_signature_from_entity(
        signature: MultisigSignatureEntity,
    ) -> Result<NewSignatureEntity, crate::Error> {
        NewSignatureEntity::try_from(signature)
    }

    pub fn build_signature(
        queue_id: &str,
        address: &str,
        signature: &str,
        status: MultisigSignatureStatus,
        weight: Option<i32>,
    ) -> NewSignatureEntity {
        NewSignatureEntity::new(queue_id, address, signature, status, weight)
    }

    pub fn build_approved_signature(
        queue_id: &str,
        address: &str,
        signature: String,
        weight: Option<i32>,
    ) -> NewSignatureEntity {
        NewSignatureEntity::new_approve(queue_id, address, signature, weight)
    }

    pub fn build_signature_from_permission_user(
        user: &PermissionUserEntity,
        queue_id: &str,
    ) -> NewSignatureEntity {
        NewSignatureEntity::from((user, queue_id))
    }

    pub async fn create_queue_with_sign(
        pool: CoreDbPool,
        params: &mut NewMultisigQueueEntity,
    ) -> Result<MultisigQueueEntity, crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // create multisig queue
        let res = MultisigQueueDaoV1::create_queue(params, tx.as_mut()).await?;

        //  if signatures is not empty insert signatures
        if !params.signatures.is_empty() {
            for signature in &mut params.signatures {
                signature.queue_id = res.id.clone();

                let exists = MultisigSignatureDaoV1::find_signature(
                    &signature.queue_id,
                    &signature.address,
                    tx.as_mut(),
                )
                .await?;

                match exists {
                    Some(s) => {
                        if s.signature.is_empty() {
                            MultisigSignatureDaoV1::update_status(signature, tx.as_mut()).await?
                        }
                    }
                    None => {
                        MultisigSignatureDaoV1::create_signature(signature, tx.as_mut()).await?
                    }
                };
            }
        }

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        Ok(res)
    }

    // 拼接额外的信息(区分多签账号和权限)
    pub async fn find_by_id_with_extra(
        id: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<MultisigQueueSimpleEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::find_with_extra(id, pool.read_ref()).await?)
    }

    pub async fn queue_list(
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
        pool: CoreDbPool,
    ) -> Result<Pagination<MultisigQueueSimpleEntity>, crate::Error> {
        let lists =
            MultisigQueueDaoV1::lists(from, chain_code, status, page, page_size, pool.into_inner())
                .await?;

        Ok(lists)
    }

    pub async fn find_by_id(
        pool: &CoreDbPool,
        queue_id: &str,
    ) -> Result<Option<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::find_by_id(queue_id, pool.read_ref()).await?)
    }

    pub async fn list_by_account_ids(
        pool: &CoreDbPool,
        account_ids: &[String],
    ) -> Result<Option<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::list_by_account_ids(account_ids, pool.read_ref()).await?)
    }

    pub async fn logic_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<Vec<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::logic_del_multisig_queue(account_id, pool.write_ref()).await?)
    }

    pub async fn physical_delete_by_account_id(
        pool: &CoreDbPool,
        account_id: &str,
    ) -> Result<Vec<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::physical_del_multisig_queue(account_id, pool.write_ref()).await?)
    }

    pub async fn physical_delete_by_account_ids(
        pool: &CoreDbPool,
        account_ids: &[&str],
    ) -> Result<Vec<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::physical_del_multi_multisig_queue(pool.write_ref(), account_ids)
            .await?)
    }

    pub async fn update_fail(
        pool: &CoreDbPool,
        queue_id: &str,
        reason: &str,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_fail(queue_id, reason, pool.write_ref()).await?)
    }

    pub async fn signed_result(
        queue_id: &str,
        account_id: &str,
        permission_id: &str,
        pool: CoreDbPool,
    ) -> Result<Vec<MemberSignedResult>, crate::Error> {
        if !account_id.is_empty() {
            Self::member_signed_result(account_id, queue_id, pool.into_inner()).await
        } else {
            Self::permission_signed_result(permission_id, queue_id, pool).await
        }
    }

    // 多签账号的前面结果
    pub async fn member_signed_result(
        account_id: &str,
        queue_id: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MemberSignedResult>, crate::Error> {
        let mut result = vec![];

        let mut member = MultisigMemberDaoV1::find_records_by_id(account_id, pool.as_ref()).await?;

        for item in member.0.iter_mut() {
            let mut sign_result =
                MemberSignedResult::new(&item.name, &item.address, item.is_self, 1);

            // 获取签名的结果
            let sign =
                MultisigSignatureDaoV1::find_signature(queue_id, &item.address, pool.as_ref())
                    .await?;
            if let Some(sign) = sign {
                sign_result.singed = sign.status;
                sign_result.signature = sign.signature;
            }
            result.push(sign_result);
        }
        Ok(result)
    }

    // 权限的签名结果
    pub async fn permission_signed_result(
        permission_id: &str,
        queue_id: &str,
        pool: CoreDbPool,
    ) -> Result<Vec<MemberSignedResult>, crate::Error> {
        let mut result = vec![];

        let mut users =
            PermissionUserDao::find_by_permission(permission_id, pool.read_ref()).await?;

        for user in users.iter_mut() {
            let mut sign_result =
                MemberSignedResult::new("", &user.address, user.is_self as i8, user.weight);

            // 获取签名的结果
            let sign =
                MultisigSignatureDaoV1::find_signature(queue_id, &user.address, pool.read_ref())
                    .await?;
            if let Some(sign) = sign {
                sign_result.singed = sign.status;
                sign_result.signature = sign.signature;
            }
            result.push(sign_result);
        }
        Ok(result)
    }

    pub async fn create_or_update_sign(
        params: &NewSignatureEntity,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        // 防止mqtt 消息进来导致并发问题
        let _lock = CREATE_QUEUE_LOCK.lock().await;
        let signature = MultisigSignatureDaoV1::find_signature(
            &params.queue_id,
            &params.address,
            pool.read_ref(),
        )
        .await?;

        match signature {
            Some(s) => {
                if s.signature.is_empty() {
                    MultisigSignatureDaoV1::update_status(params, pool.write_ref()).await?
                }
            }
            None => MultisigSignatureDaoV1::create_signature(params, pool.write_ref()).await?,
        };
        Ok(())
    }

    // 未执行的交易修改状态(根据签名,数量)
    pub async fn sync_sign_status(
        queue: &MultisigQueueEntity,
        status: i8,
        pool: crate::CoreDbPool,
    ) -> Result<(), crate::Error> {
        let status = MultisigQueueStatus::from_i8(status);

        if !status.need_sync_status() {
            return Ok(());
        }

        // 多签的账号或者权限的账号
        let (status, reason) = if !queue.account_id.is_empty() {
            MultisigQueueRepo::compute_status_by_account(queue, &pool).await?
        } else {
            MultisigQueueRepo::compute_status_by_permission(queue, &pool).await?
        };

        match status {
            MultisigQueueStatus::Fail => {
                MultisigQueueDaoV1::update_fail(&queue.id, &reason, pool.write_ref()).await?
            }
            _ => MultisigQueueDaoV1::update_status(&queue.id, status, pool.write_ref()).await?,
        }

        Ok(())
    }

    // 根据多签账号计算队列里面的状态
    async fn compute_status_by_account(
        queue: &MultisigQueueEntity,
        pool: &CoreDbPool,
    ) -> Result<(MultisigQueueStatus, String), crate::Error> {
        let account = MultisigAccountDaoV1::find_by_id(&queue.account_id, pool.read_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;

        // fetch all member sign result
        let signed =
            MultisigQueueRepo::member_signed_result(&queue.account_id, &queue.id, pool.read_pool())
                .await?;

        Ok((Self::compute_status(signed, account.threshold as i64), SIGN_FAILED.to_string()))
    }

    fn compute_status(signed: Vec<MemberSignedResult>, threshold: i64) -> MultisigQueueStatus {
        let mut status = MultisigQueueStatus::HasSignature;
        let mut remain_num = 0;
        let mut approved_num = 0;

        for sign in signed {
            if sign.is_self == 1 && sign.singed == MultisigSignatureStatus::UnSigned.to_i8() {
                status = MultisigQueueStatus::PendingSignature;
            }

            // 剩余未签名的数量
            if sign.singed == MultisigSignatureStatus::UnSigned.to_i8() {
                remain_num += sign.weight;
            }

            // 已经同意签名的数量
            if sign.singed == MultisigSignatureStatus::Approved.to_i8() {
                approved_num += sign.weight;
            }
        }

        if approved_num >= threshold {
            status = MultisigQueueStatus::PendingExecution;
        } else {
            // 如果剩余的未签名数量 + 同意签名的数量 < 阈值 则这个交易队列失败
            if remain_num + approved_num < threshold {
                status = MultisigQueueStatus::Fail;
            }
        }
        status
    }

    async fn compute_status_by_permission(
        queue: &MultisigQueueEntity,
        pool: &CoreDbPool,
    ) -> Result<(MultisigQueueStatus, String), crate::Error> {
        let permission = PermissionDao::find_by_id(&queue.permission_id, false, pool.read_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;

        // let Some(permission) = permission else {
        //     return Ok((MultisigQueueStatus::Fail, PERMISSION_CHANGE.to_string()));
        // };

        // fetch all user sign result
        let signed = MultisigQueueRepo::signed_result(
            &queue.id,
            &queue.account_id,
            &queue.permission_id,
            pool.clone(),
        )
        .await?;

        Ok((Self::compute_status(signed, permission.threshold), SIGN_FAILED.to_string()))
    }

    pub async fn self_member_account_id(
        id: &str,
        pool: &CoreDbPool,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::get_self_by_id(id, pool.read_ref()).await?)
    }

    pub async fn self_member_by_account(
        id: &str,
        pool: &CoreDbPool,
    ) -> Result<MultisigMemberEntities, crate::Error> {
        Ok(MultisigMemberDaoV1::get_self_by_id(id, pool.read_ref()).await?)
    }

    pub async fn get_signed_list(
        pool: &CoreDbPool,
        queue_id: &str,
    ) -> Result<MultisigSignatureEntities, crate::Error> {
        Ok(MultisigSignatureDaoV1::get_signed_list(queue_id, pool.read_ref()).await?)
    }

    pub async fn update_status_and_hash(
        queue_id: &str,
        status: MultisigQueueStatus,
        tx_hash: &str,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_status_and_tx_hash(
            queue_id,
            status,
            tx_hash,
            pool.write_ref(),
        )
        .await?)
    }

    pub async fn update_status_hash(
        queue_id: &str,
        status: MultisigQueueStatus,
        tx_hash: &str,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_status_and_tx_hash(
            queue_id,
            status,
            tx_hash,
            pool.write_ref(),
        )
        .await?)
    }

    pub async fn multisig_queue_data(
        queue_id: &str,
        pool: crate::CoreDbPool,
    ) -> Result<MultisigQueueData, crate::Error> {
        let queue = MultisigQueueDaoV1::find_by_id(queue_id, pool.read_ref())
            .await?
            .ok_or(crate::DatabaseError::ReturningNone)?;

        let signatures =
            MultisigSignatureDaoV1::find_by_queue_id(queue_id, pool.into_inner()).await?;

        Ok(MultisigQueueData::new(queue, MultisigSignatureEntities(signatures)))
    }

    pub async fn permission_update_fail(
        address: &str,
        pool: &CoreDbPool,
    ) -> Result<Vec<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::permission_fail(address, pool.write_ref()).await?)
    }

    pub async fn ongoing_queue(
        chain_code: &str,
        address: &str,
        pool: &CoreDbPool,
    ) -> Result<Option<MultisigQueueEntity>, crate::Error> {
        let queue = MultisigQueueDaoV1::ongoing_queue(pool.read_ref(), chain_code, address).await?;
        Ok(queue)
    }

    // delete queue and signature
    pub async fn delete_queue(pool: &CoreDbPool, queue_id: &str) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // delete permission
        MultisigQueueDaoV1::delete_by_id(queue_id, tx.as_mut()).await?;

        // delete all signature
        MultisigSignatureDaoV1::physical_del_multi_multisig_signatures(
            tx.as_mut(),
            vec![queue_id.to_string()],
        )
        .await?;

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    // delete queue and signature
    pub async fn delete_queue_by_permission(
        pool: &CoreDbPool,
        permission_id: &str,
    ) -> Result<(), crate::Error> {
        let mut tx = pool
            .write_ref()
            .begin()
            .await
            .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        // delete permission
        let queues = MultisigQueueDaoV1::delete_by_permission(permission_id, tx.as_mut()).await?;

        let ids = queues.iter().map(|q| q.id.clone()).collect::<Vec<String>>();

        // delete all signature
        if !ids.is_empty() {
            MultisigSignatureDaoV1::physical_del_multi_multisig_signatures(tx.as_mut(), ids)
                .await?;
        }

        tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

        Ok(())
    }

    pub async fn pending_handle(
        pool: &CoreDbPool,
    ) -> Result<Vec<MultisigQueueEntity>, crate::Error> {
        Ok(MultisigQueueDaoV1::pending_handle(pool.read_ref()).await?)
    }

    pub async fn update_status(
        queue_id: &str,
        status: MultisigQueueStatus,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        Ok(MultisigQueueDaoV1::update_status(queue_id, status, pool.write_ref()).await?)
    }

    pub async fn update_expired_queue(pool: &CoreDbPool) -> Result<(), crate::Error> {
        MultisigQueueDaoV1::update_expired_queue(pool.write_ref()).await?;
        Ok(())
    }

    pub async fn rollback_update_fail(
        pool: &CoreDbPool,
        queue_id: &str,
        status: i8,
    ) -> Result<(), crate::Error> {
        MultisigQueueDaoV1::rollback_update_fail(queue_id, status, pool.write_ref()).await?;
        Ok(())
    }

    pub async fn update_status_with_executor<'a, E>(
        queue_id: &str,
        status: MultisigQueueStatus,
        exec: E,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        Ok(MultisigQueueDaoV1::update_status(queue_id, status, exec).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::MultisigQueueRepo;
    use crate::{
        dao::multisig_queue::MultisigQueueDaoV1,
        entities::{
            asset_token_key::AssetTokenKey,
            bill::BillKind,
            multisig_queue::{MultisigQueueEntity, MultisigQueueStatus},
            multisig_signatures::{MultisigSignatureEntity, MultisigSignatureStatus},
            permission_user::PermissionUserEntity,
        },
        repositories::test_helper::setup_core_pool,
    };

    #[test]
    fn multisig_queue_repo_build_queue_from_entity_maps_id_and_status() {
        let queue = MultisigQueueEntity {
            id: "q1".to_string(),
            account_id: "a1".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "1".to_string(),
            symbol: "TRX".to_string(),
            expiration: 100,
            chain_code: "tron".to_string(),
            token_addr: AssetTokenKey::Native,
            msg_hash: "mh".to_string(),
            tx_hash: "th".to_string(),
            raw_data: "raw".to_string(),
            status: MultisigQueueStatus::InConfirmation.to_i8(),
            notes: "".to_string(),
            fail_reason: "".to_string(),
            created_at: sqlx::types::chrono::Utc::now().into(),
            updated_at: None,
            transfer_type: 1,
            permission_id: "".to_string(),
        };

        let built = MultisigQueueRepo::build_queue_from_entity(queue);
        assert_eq!(built.id, "q1");
        assert_eq!(built.status, MultisigQueueStatus::InConfirmation);
    }

    #[test]
    fn multisig_queue_repo_build_signature_helpers_work() {
        let sig = MultisigQueueRepo::build_signature(
            "q2",
            "addr1",
            "0xab",
            MultisigSignatureStatus::Approved,
            Some(2),
        );
        assert_eq!(sig.queue_id, "q2");
        assert_eq!(sig.address, "addr1");
        assert_eq!(sig.status.to_i8(), MultisigSignatureStatus::Approved.to_i8());
        assert_eq!(sig.weight, Some(2));

        let approved =
            MultisigQueueRepo::build_approved_signature("q3", "addr2", "0xcd".to_string(), None);
        assert_eq!(approved.queue_id, "q3");
        assert_eq!(approved.status.to_i8(), MultisigSignatureStatus::Approved.to_i8());
    }

    #[test]
    fn multisig_queue_repo_build_signature_from_entity_and_permission_user_work() {
        let source = MultisigSignatureEntity {
            id: 1,
            queue_id: "q4".to_string(),
            address: "addr3".to_string(),
            signature: "".to_string(),
            status: MultisigSignatureStatus::UnSigned.to_i8(),
            created_at: sqlx::types::chrono::Utc::now().into(),
            updated_at: None,
        };
        let mapped = MultisigQueueRepo::build_signature_from_entity(source).unwrap();
        assert_eq!(mapped.queue_id, "q4");
        assert_eq!(mapped.status.to_i8(), MultisigSignatureStatus::UnSigned.to_i8());

        let user = PermissionUserEntity {
            id: None,
            permission_id: "p1".to_string(),
            address: "addr4".to_string(),
            grantor_addr: "".to_string(),
            weight: 3,
            is_self: 0_i64,
            created_at: sqlx::types::chrono::Utc::now().into(),
            updated_at: None,
        };
        let from_user = MultisigQueueRepo::build_signature_from_permission_user(&user, "q5");
        assert_eq!(from_user.queue_id, "q5");
        assert_eq!(from_user.address, "addr4");
        assert_eq!(from_user.weight, Some(3));
        assert_eq!(from_user.status.to_i8(), MultisigSignatureStatus::UnSigned.to_i8());
    }

    fn build_queue(
        account_id: &str,
        queue_id: &str,
    ) -> crate::entities::multisig_queue::NewMultisigQueueEntity {
        crate::entities::multisig_queue::NewMultisigQueueEntity::new(
            account_id.to_string(),
            "T_from".to_string(),
            "T_to".to_string(),
            3600,
            "msg_hash",
            "raw_data",
            BillKind::Transfer,
            "1".to_string(),
        )
        .with_id(queue_id)
    }

    #[tokio::test]
    async fn multisig_queue_repo_create_and_find_success() {
        let pool = setup_core_pool("wallet_db_multisig_queue_repo_success").await;
        let mut queue = build_queue("acc_q_success", "queue_success");
        let created =
            MultisigQueueRepo::create_queue_with_sign(pool.clone(), &mut queue).await.unwrap();
        assert_eq!(created.id, "queue_success");

        let found = MultisigQueueRepo::find_by_id(&pool, "queue_success").await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn multisig_queue_repo_find_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_multisig_queue_repo_edge").await;
        let found = MultisigQueueRepo::find_by_id(&pool, "queue_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn multisig_queue_repo_tx_rollback_keeps_queue_absent() {
        let pool = setup_core_pool("wallet_db_multisig_queue_repo_rollback").await;
        let queue = build_queue("acc_q_rb", "queue_rb");

        let mut tx = pool.write_ref().begin().await.unwrap();
        MultisigQueueDaoV1::create_queue(&queue, tx.as_mut()).await.unwrap();
        tx.rollback().await.unwrap();

        let found = MultisigQueueRepo::find_by_id(&pool, "queue_rb").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn multisig_queue_repo_list_binds_address_and_chain_filters() {
        let pool = setup_core_pool("wallet_db_multisig_queue_list_quotes").await;
        let mut queue = build_queue("account_list", "queue_list_quotes");
        queue.from_addr = "owner's address".to_string();
        queue.chain_code = "tron'network".to_string();
        MultisigQueueDaoV1::create_queue(&queue, pool.write_ref()).await.unwrap();

        let page = MultisigQueueRepo::queue_list(
            Some("owner's address"),
            Some("tron'network"),
            MultisigQueueStatus::PendingSignature.to_i8() as i32,
            0,
            10,
            pool.clone(),
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].id, "queue_list_quotes");
    }

    #[tokio::test]
    async fn multisig_queue_repo_account_id_lookup_binds_values() {
        let pool = setup_core_pool("wallet_db_multisig_queue_account_quotes").await;
        let queue = build_queue("account'quoted", "queue_account_quotes");
        MultisigQueueDaoV1::create_queue(&queue, pool.write_ref()).await.unwrap();

        let found = MultisigQueueRepo::list_by_account_ids(&pool, &["account'quoted".to_string()])
            .await
            .unwrap()
            .expect("matching queue should be returned");

        assert_eq!(found.id, "queue_account_quotes");
    }

    #[tokio::test]
    async fn multisig_queue_repo_multi_delete_binds_account_ids() {
        let pool = setup_core_pool("wallet_db_multisig_queue_delete_quotes").await;
        let first = build_queue("account'quoted", "queue_delete_quotes");
        let other = build_queue("other_account", "queue_delete_other");
        MultisigQueueDaoV1::create_queue(&first, pool.write_ref()).await.unwrap();
        MultisigQueueDaoV1::create_queue(&other, pool.write_ref()).await.unwrap();

        let deleted = MultisigQueueRepo::physical_delete_by_account_ids(&pool, &["account'quoted"])
            .await
            .unwrap();

        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].id, "queue_delete_quotes");
        assert!(
            MultisigQueueRepo::find_by_id(&pool, "queue_delete_quotes").await.unwrap().is_none()
        );
        assert!(
            MultisigQueueRepo::find_by_id(&pool, "queue_delete_other").await.unwrap().is_some()
        );
    }
}
