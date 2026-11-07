use crate::{
    context::Context,
    domain::{
        self,
        bill::BillDomain,
        chain::{adapter::MultisigAdapter, transaction::ChainTransDomain},
        task_queue::TaskQueueDomain,
    },
    infrastructure::task_queue::task::Tasks,
    messaging::mqtt::topics::MultiSignTransAcceptCompleteMsgBody,
    response_vo::standard_wallet::multisig_transaction::ExtraData,
};
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use wallet_chain_interact::tron::operations::multisig::TransactionOpt;
use wallet_database::{
    CoreDbPool, DbPool,
    entities::{
        multisig_account::MultisigAccountEntity,
        multisig_queue::{
            MultisigQueueData, MultisigQueueEntity, MultisigQueueSimpleEntity, MultisigQueueStatus,
            NewMultisigQueueEntity,
        },
        multisig_signatures::{MultisigSignatureStatus, NewSignatureEntity},
        permission::PermissionWithUserEntity,
    },
    repositories::{
        account::AccountRepo, multisig_account::MultisigAccountRepo,
        multisig_member::MultisigMemberRepo, multisig_queue::MultisigQueueRepo,
        permission::PermissionRepo, wallet::WalletRepo,
    },
};
use wallet_transport_backend::{
    api::wallet::permission::PermissionAcceptReq,
    consts::endpoint,
    request::{PermissionData, SignedTranAcceptReq, SignedTranCreateReq},
};

pub struct MultisigQueueDomain;
impl MultisigQueueDomain {
    pub fn validate_queue(
        queue: &MultisigQueueEntity,
        execute: bool,
    ) -> Result<(), crate::error::service::ServiceError> {
        // check queue is expired
        let time = sqlx::types::chrono::Utc::now().timestamp();
        if queue.expiration < time {
            return Err(crate::error::business::BusinessError::MultisigQueue(
                crate::error::business::multisig_queue::MultisigQueueError::Expired,
            ))?;
        }

        // check status
        if queue.status == MultisigQueueStatus::InConfirmation.to_i8() {
            return Err(crate::error::business::BusinessError::MultisigQueue(
                crate::error::business::multisig_queue::MultisigQueueError::AlreadyExecuted,
            ))?;
        }

        if queue.status == MultisigQueueStatus::Fail.to_i8() {
            return Err(crate::error::business::BusinessError::MultisigQueue(
                crate::error::business::multisig_queue::MultisigQueueError::FailedQueue,
            ))?;
        }

        // execute multisig tx need check status
        if execute && queue.status != MultisigQueueStatus::PendingExecution.to_i8() {
            return Err(crate::error::business::BusinessError::MultisigQueue(
                crate::error::business::multisig_queue::MultisigQueueError::NotPendingExecStatus,
            ))?;
        }

        Ok(())
    }

    pub async fn recover_all_uid_queue_data_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        let uid_list = WalletRepo::uid_list(CoreDbPool::new(pool.clone()))
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<Vec<String>>();

        let raw_time = Self::get_raw_time_with_ctx(ctx, &uid_list).await?;

        for uid in uid_list {
            Self::recover_queue_data_with_raw_time_with_ctx(ctx, &uid, raw_time.clone()).await?;
        }

        Ok(())
    }

    pub async fn recover_all_queue_data_with_ctx(
        ctx: &'static Context,
        uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let raw_time = Self::get_raw_time_with_ctx(ctx, &[uid.to_string()]).await?;
        Self::recover_queue_data_with_raw_time_with_ctx(ctx, uid, raw_time).await?;
        Ok(())
    }

    pub(crate) async fn get_raw_time_with_ctx(
        ctx: &'static Context,
        uid_list: &[String],
    ) -> Result<Option<String>, crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        let account_ids = MultisigMemberRepo::list_by_uids(&core_pool, uid_list)
            .await?
            .0
            .into_iter()
            .map(|member| member.account_id)
            .collect::<HashSet<String>>();

        let account_ids_vec: Vec<String> = account_ids.into_iter().collect();
        let queue = MultisigQueueRepo::list_by_account_ids(&core_pool, &account_ids_vec).await?;

        let raw_time = queue.map(|q| {
            let now = q.created_at - Duration::from_secs(86400);
            now.format("%Y-%m-%d %H:%M:%S").to_string()
        });
        Ok(raw_time)
    }

    pub async fn recover_queue_data_with_ctx(
        ctx: &'static Context,
        uid: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let raw_time = Self::get_raw_time_with_ctx(ctx, &[uid.to_string()]).await?;

        Self::recover_queue_data_with_raw_time_with_ctx(ctx, uid, raw_time).await?;

        Ok(())
    }

    pub(crate) async fn recover_queue_data_with_raw_time_with_ctx(
        ctx: &'static Context,
        uid: &str,
        raw_time: Option<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        let pool = ctx.get_global_sqlite_pool()?;

        let req = wallet_transport_backend::request::FindAddressRawDataReq::new_trans(
            Some(uid.to_string()),
            raw_time,
            None,
        );
        let data = backend.address_find_address_raw_data(req).await?;

        let list = data.list;
        for item in list {
            if let Some(raw_data) = item.raw_data {
                match MultisigQueueData::from_string(&raw_data) {
                    Ok(r) => {
                        let id = r.queue.id.clone();
                        if let Err(e) = Self::insert_with_ctx(ctx, pool.clone(), r).await {
                            tracing::error!("recover queue data error: {} queue_id = {}", e, id);
                        }
                    }
                    Err(e) => {
                        tracing::error!("revover queue data paser : {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn insert_with_ctx(
        ctx: &'static Context,
        pool: DbPool,
        data: MultisigQueueData,
    ) -> Result<(), crate::error::service::ServiceError> {
        let MultisigQueueData { queue, signatures } = data;
        let id = queue.id.clone();

        // 构建交易数据
        let mut params = MultisigQueueRepo::build_queue_from_entity(queue).check_expiration();

        let mut report = false;
        if !params.tx_hash.is_empty() && params.status == MultisigQueueStatus::InConfirmation {
            let tx = domain::bill::BillDomain::get_onchain_bill(
                ctx,
                &params.tx_hash,
                &params.chain_code,
            )
            .await?;
            if let Some(tx) = tx {
                if tx.status == 2 {
                    params.status = MultisigQueueStatus::Success;
                } else {
                    params.status = MultisigQueueStatus::Fail;
                }
            }
            report = true;
        }

        for sig in signatures.0 {
            let signature = MultisigQueueRepo::build_signature_from_entity(sig)?;
            params = params.with_signatures(signature);
        }

        let core_pool = CoreDbPool::new(pool.clone());
        let queue =
            MultisigQueueRepo::create_queue_with_sign(core_pool.clone(), &mut params).await?;
        if let Err(_e) =
            MultisigQueueRepo::sync_sign_status(&queue, params.status.to_i8(), core_pool.clone())
                .await
        {
            if !queue.permission_id.is_empty() {
                tracing::warn!(
                    "recover permission queue faild, perrmision not found and delete queue queue_id = {},permision_id = {}",
                    queue.id,
                    queue.permission_id
                );
                MultisigQueueRepo::delete_queue(&core_pool, &queue.id).await?;
            }
        };

        if report {
            Self::update_raw_data_with_ctx(ctx, &id, pool.clone()).await?;
        }

        Ok(())
    }

    // For transactions in the confirmation queue, periodically query the transaction results.
    pub async fn sync_queue_status_with_ctx(
        ctx: &'static Context,
        queue_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());

        let queue = MultisigQueueRepo::find_by_id(&core_pool, queue_id).await?;

        let Some(queue) = queue else {
            return Ok(());
        };

        // 仅处理状态为 InConfirmation 且有 tx_hash 的队列
        if queue.status != MultisigQueueStatus::InConfirmation.to_i8() || queue.tx_hash.is_empty() {
            return Ok(());
        }

        // 获取链上交易状态
        if let Some(rs) =
            BillDomain::get_onchain_bill(ctx, &queue.tx_hash, &queue.chain_code).await?
        {
            // 更新状态：2 为成功，否则为失败
            let tx_status = if rs.status == 2 {
                MultisigQueueStatus::Success
            } else {
                MultisigQueueStatus::Fail
            };

            MultisigQueueRepo::update_status(queue_id, tx_status, &core_pool).await?;

            Self::update_raw_data_with_ctx(ctx, queue_id, pool).await?;
        }

        Ok(())
    }

    // Report the successful transaction queue back to the backend to update the raw data.
    pub async fn update_raw_data_with_ctx(
        ctx: &'static Context,
        queue_id: &str,
        pool: DbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let raw_data = MultisigQueueRepo::multisig_queue_data(queue_id, CoreDbPool::new(pool))
            .await?
            .to_string()?;

        let backend_api = ctx.get_global_backend_api();
        Ok(backend_api.update_raw_data(queue_id, raw_data).await?)
    }

    // 波场交易队列事件、并进行默认签名
    pub async fn tron_sign_and_create_queue(
        ctx: &'static Context,
        queue: &mut NewMultisigQueueEntity,
        account: &MultisigAccountEntity,
        password: String,
        pool: DbPool,
    ) -> Result<MultisigQueueEntity, crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        let mut members =
            MultisigQueueRepo::self_member_by_account(&account.id, &core_pool).await?;
        members.prioritize_by_address(&account.initiator_addr);

        // sign num
        let sign_num = members.0.len().min(account.threshold as usize);
        for i in 0..sign_num {
            let member = members.0.get(i).unwrap();
            let key = crate::domain::account::open_subpk_with_password(
                ctx,
                &queue.chain_code,
                &member.address,
                &password,
            )
            .await?;

            let sign_result = TransactionOpt::sign_transaction(&queue.raw_data, key)?;
            let sign = MultisigQueueRepo::build_signature(
                &queue.id,
                &member.address,
                &sign_result.signature,
                MultisigSignatureStatus::Approved,
                None,
            );
            queue.signatures.push(sign);
        }

        // 计算签名的状态
        queue.compute_status(account.threshold);

        let res = MultisigQueueRepo::create_queue_with_sign(core_pool, queue).await?;

        Ok(res)
    }

    // 非solana链对交易队列进行批量签名
    pub async fn batch_sign_queue(
        ctx: &'static Context,
        queue: &mut NewMultisigQueueEntity,
        password: &str,
        account: &MultisigAccountEntity,
        adapter: &MultisigAdapter,
        pool: &DbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        let mut members =
            MultisigQueueRepo::self_member_by_account(&account.id, &core_pool).await?;
        members.prioritize_by_address(&account.initiator_addr);

        // sign num
        let sign_num = members.0.len().min(account.threshold as usize);
        for i in 0..sign_num {
            let member = members.0.get(i).unwrap();
            let key =
                ChainTransDomain::get_key(ctx, &member.address, &queue.chain_code, password, &None)
                    .await?;

            let sign_res =
                adapter.sign_multisig_tx(account, &member.address, key, &queue.raw_data).await?;

            let sign = MultisigQueueRepo::build_approved_signature(
                &queue.id,
                &member.address,
                sign_res.signature,
                None,
            );
            queue.signatures.push(sign);
        }

        Ok(())
    }

    // 签名时权限创建所有的签名者(目前仅有tron)
    pub async fn batch_sign_with_permission(
        ctx: &'static Context,
        queue: &mut NewMultisigQueueEntity,
        password: &str,
        p: &PermissionWithUserEntity,
        pool: &DbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        // sign num
        let mut signatures = p
            .user
            .iter()
            .map(|u| MultisigQueueRepo::build_signature_from_permission_user(u, queue.id.as_str()))
            .collect::<Vec<NewSignatureEntity>>();

        // 需要执行几次签名
        let sign_num = p.total_weight().min(p.permission.threshold as i32);
        let mut signed = 0;

        let core_pool = CoreDbPool::new(pool.clone());
        for user in signatures.iter_mut() {
            // let query_req = QueryReq::new_address_chain(&user.address, &queue.chain_code);

            if AccountRepo::detail_by_address_and_chain_code(
                core_pool.clone(),
                &user.address,
                &queue.chain_code,
            )
            .await?
            .is_some()
            {
                let key = ChainTransDomain::get_key(
                    ctx,
                    &user.address,
                    &queue.chain_code,
                    password,
                    &None,
                )
                .await?;

                let res = TransactionOpt::sign_transaction(&queue.raw_data, key)?;

                user.signature = res.signature;
                user.status = MultisigSignatureStatus::Approved;

                signed += user.weight.unwrap_or(1);
            }

            if signed >= sign_num {
                break;
            }
        }

        queue.signatures = signatures;

        Ok(())
    }

    // 队列数据上报到后端
    pub async fn upload_queue_backend(
        ctx: &'static Context,
        queue_id: String,
        pool: &DbPool,
        backend_params: Option<PermissionAcceptReq>,
        opt_data: Option<PermissionData>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let raw_data =
            MultisigQueueRepo::multisig_queue_data(&queue_id, CoreDbPool::new(pool.clone()))
                .await?;

        let req = SignedTranCreateReq {
            withdraw_id: raw_data.queue.id.clone(),
            address: raw_data.queue.from_addr.clone(),
            chain_code: raw_data.queue.chain_code.clone(),
            raw_data: raw_data.to_string()?,
            tx_kind: raw_data.queue.transfer_type as i8,
            permission_data: opt_data,
        };

        let mut tasks = Tasks::new();
        let task = TaskQueueDomain::send_or_wrap_task_with_ctx(
            ctx,
            req,
            endpoint::multisig::SIGNED_TRAN_CREATE,
        )
        .await?;
        if let Some(task) = task {
            tasks = tasks.push(task);
        }

        // let task = task_queue::Task::BackendApi(task_queue::BackendApiTask::BackendApi(
        //     BackendApiTaskData::new(endpoint::multisig::SIGNED_TRAN_CREATE, &req)?,
        // ));
        // let mut tasks = task_queue::Tasks::new().push(task);
        // 多签权限的修改单独上报一份权限的数据
        if let Some(req) = backend_params {
            let task = TaskQueueDomain::send_or_wrap_task_with_ctx(
                ctx,
                req,
                endpoint::multisig::PERMISSION_ACCEPT,
            )
            .await?;
            if let Some(task) = task {
                tasks = tasks.push(task);
            }
            // let task = task_queue::Task::BackendApi(task_queue::BackendApiTask::BackendApi(
            //     BackendApiTaskData::new(endpoint::multisig::PERMISSION_ACCEPT, &backend_params)?,
            // ));
            // tasks = tasks.push(task);
        }

        tasks.send_with_ctx(ctx).await?;

        Ok(())
    }

    pub async fn upload_queue_sign(
        ctx: &'static Context,
        queue_id: &str,
        pool: DbPool,
        signed: Vec<NewSignatureEntity>,
        status: MultisigSignatureStatus,
    ) -> Result<(), crate::error::service::ServiceError> {
        let tx_str =
            signed.iter().map(|i| i.into()).collect::<Vec<MultiSignTransAcceptCompleteMsgBody>>();
        let accept_address = signed.iter().map(|v| v.address.clone()).collect();

        let raw_data = MultisigQueueRepo::multisig_queue_data(queue_id, CoreDbPool::new(pool))
            .await?
            .to_string()?;
        let req = SignedTranAcceptReq {
            withdraw_id: queue_id.to_string(),
            tx_str: json!(tx_str),
            accept_address,
            status: status.to_i8(),
            raw_data,
        };
        TaskQueueDomain::send_or_to_queue_with_ctx(
            ctx,
            req,
            endpoint::multisig::SIGNED_TRAN_ACCEPT,
        )
        .await?;

        // let task = Task::BackendApi(BackendApiTask::BackendApi(BackendApiTaskData {
        //     endpoint: endpoint::multisig::SIGNED_TRAN_ACCEPT.to_string(),
        //     body: serde_func::serde_to_value(&req)?,
        // }));
        // Tasks::new().push(task).send().await?;

        Ok(())
    }

    // 过期时间 小时转换为秒
    pub fn sub_expiration(expiration: i64) -> i64 {
        if expiration == 24 { expiration * 3600 - 61 } else { expiration * 3600 }
    }

    pub async fn handle_queue_extra(
        queue: &MultisigQueueSimpleEntity,
        pool: &DbPool,
    ) -> Result<Option<ExtraData>, crate::error::service::ServiceError> {
        if !queue.account_id.is_empty() {
            let account =
                MultisigAccountRepo::find_by_id(&CoreDbPool::new(pool.clone()), &queue.account_id)
                    .await?;
            if let Some(account) = account {
                return Ok(Some(ExtraData::from(account)));
            }
        } else {
            let permission =
                PermissionRepo::find_option(&CoreDbPool::new(pool.clone()), &queue.permission_id)
                    .await?;

            if let Some(permission) = permission {
                return Ok(Some(ExtraData::from(permission)));
            };
        }

        Ok(None)
    }
}
