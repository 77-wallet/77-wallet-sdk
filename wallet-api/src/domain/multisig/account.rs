use crate::domain::{
    self,
    task_queue::{BackendApiTask, Task},
};
use sqlx::{Pool, Sqlite};
use wallet_database::{
    dao::{
        multisig_account::MultisigAccountDaoV1, multisig_member::MultisigMemberDaoV1,
        multisig_queue::MultisigQueueDaoV1,
    },
    entities::{
        assets::AssetsEntity,
        coin::CoinMultisigStatus,
        device::DeviceEntity,
        multisig_account::{
            MultiAccountOwner, MultisigAccountData, MultisigAccountEntity,
            MultisigAccountPayStatus, MultisigAccountStatus, NewMultisigAccountEntity,
        },
        multisig_member::MemberVo,
        multisig_queue::MultisigQueueEntity,
        wallet::WalletEntity,
    },
};
use wallet_transport_backend::request::FindAddressRawDataReq;

pub struct MultisigDomain;

impl MultisigDomain {
    pub fn validate_queue(account: &MultisigAccountEntity) -> Result<(), crate::ServiceError> {
        if account.owner == MultiAccountOwner::Participant.to_i8() {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::OnlyInitiatorCreateTx,
            ))?;
        }

        if account.pay_status != MultisigAccountPayStatus::Paid.to_i8() {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::NotPay,
            ))?;
        }

        if account.status != MultisigAccountStatus::OnChain.to_i8() {
            return Err(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::NotOnChain,
            ))?;
        }

        Ok(())
    }

    pub(crate) async fn recover_uid_multisig_data(uid: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let uid_list = WalletEntity::uid_list(&*pool)
            .await?
            .into_iter()
            .map(|uid| uid.0)
            .collect::<std::collections::HashSet<String>>();
        MultisigDomain::recover_multisig_data(uid, &uid_list).await?;
        Ok(())
    }

    pub(crate) async fn recover_multisig_data(
        uid: &str,
        uid_list: &std::collections::HashSet<String>,
    ) -> Result<(), crate::ServiceError> {
        let backend = crate::manager::Context::get_global_backend_api()?;
        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let req = FindAddressRawDataReq::new_multisig(uid);
        let data = backend.address_find_address_raw_data(req).await?;
        let list = data.list;

        for address_raw_data in list {
            if let Some(raw_data) = address_raw_data.raw_data {
                if let Ok(mut data) = MultisigAccountData::from_string(&raw_data) {
                    // handle deploy status
                    if !data.account.deploy_hash.is_empty()
                        && data.account.status != MultisigAccountStatus::OnChain.to_i8()
                    {
                        let adapter =
                            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(
                                &data.account.chain_code,
                            )
                            .await?;
                        if let Some(tx_result) =
                            adapter.query_tx_res(&data.account.deploy_hash).await?
                        {
                            data.account.status = if tx_result.status == 2 {
                                MultisigAccountStatus::OnChain.to_i8()
                            } else {
                                MultisigAccountStatus::OnChainFail.to_i8()
                            }
                        }
                    }
                    // handle pay status
                    if !data.account.fee_hash.is_empty()
                        && data.account.pay_status != MultisigAccountPayStatus::Paid.to_i8()
                    {
                        let adapter =
                            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(
                                &data.account.chain_code,
                            )
                            .await?;
                        if let Some(tx_result) =
                            adapter.query_tx_res(&data.account.deploy_hash).await?
                        {
                            data.account.pay_status = if tx_result.status == 2 {
                                MultisigAccountPayStatus::Paid.to_i8()
                            } else {
                                MultisigAccountPayStatus::PaidFail.to_i8()
                            }
                        }
                    }
                    if let Err(e) = Self::insert(pool.clone(), data, uid_list).await {
                        tracing::error!("insert multisig data error: {:?}", e);
                    };
                } else {
                    tracing::error!("parse multisig data error: {:?}", raw_data);
                };
            }
        }

        let Some(device) = DeviceEntity::get_device_info(&*pool).await? else {
            return Err(crate::BusinessError::Device(crate::DeviceError::Uninitialized).into());
        };

        let device_bind_address_task_data =
            domain::app::DeviceDomain::gen_device_bind_address_task_data(&device.sn).await?;
        domain::task_queue::Tasks::new()
            .push(Task::BackendApi(BackendApiTask::BackendApi(
                device_bind_address_task_data,
            )))
            .send()
            .await?;
        Ok(())
    }

    pub async fn insert(
        pool: std::sync::Arc<Pool<Sqlite>>,
        data: MultisigAccountData,
        uid_list: &std::collections::HashSet<String>,
    ) -> Result<(), crate::ServiceError> {
        let MultisigAccountData { account, members } = data;
        let member_list = members
            .0
            .into_iter()
            .map(|member| MemberVo {
                name: member.name,
                address: member.address,
                pubkey: member.pubkey,
                confirmed: member.confirmed,
                uid: member.uid,
            })
            .collect::<Vec<MemberVo>>();

        let pay_status = MultisigAccountPayStatus::try_from(account.pay_status)?;
        let status = MultisigAccountStatus::try_from(account.status)?;

        let mut params = NewMultisigAccountEntity::new(
            Some(account.id),
            account.name,
            account.initiator_addr,
            account.address,
            account.chain_code,
            account.threshold,
            account.address_type,
            member_list,
            uid_list,
        )
        .with_authority_addr(account.authority_addr)
        .with_salt(account.salt)
        .with_deploy_hash(&account.deploy_hash)
        .with_fee_hash(&account.fee_hash)
        .with_status(status)
        .with_pay_status(pay_status);

        // 账号归属问题
        let initial_addr = params.initiator_addr.clone();

        // 判断是否是创建者
        let is_owner = params
            .member_list
            .iter()
            .any(|m| m.address == initial_addr && m.is_self == 1);

        // 判断是否还有其他参与者是自己
        let has_other_self = params
            .member_list
            .iter()
            .any(|m| m.address != initial_addr && m.is_self == 1);

        // 根据判断结果设置 owner
        let owner = match (is_owner, has_other_self) {
            (true, true) => MultiAccountOwner::Both,
            (true, false) => MultiAccountOwner::Owner,
            (false, _) => MultiAccountOwner::Participant,
        };
        params.owner = owner;
        params.create_at = account.created_at;

        if pay_status == MultisigAccountPayStatus::Paid && status == MultisigAccountStatus::OnChain
        {
            // 初始化多签资产
            domain::assets::AssetsDomain::init_default_multisig_assets(
                params.address.clone(),
                params.chain_code.clone(),
            )
            .await?;

            // 如果不是参与者,那么这个账号下所有的资产都应该被恢复为多签的
            if owner != MultiAccountOwner::Participant {
                AssetsEntity::update_tron_multisig_assets(
                    &params.address.clone(),
                    &params.chain_code.clone(),
                    CoinMultisigStatus::IsMultisig.to_i8(),
                    pool.as_ref(),
                )
                .await?;
            }
        }

        MultisigAccountDaoV1::create_account_with_member(&params, pool).await?;

        Ok(())
    }

    pub async fn account_by_id(
        id: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<MultisigAccountEntity, crate::ServiceError> {
        let account = MultisigAccountDaoV1::find_by_conditions(vec![("id", id)], pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?
            .ok_or(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::NotFound,
            ))?;
        Ok(account)
    }
    pub async fn account_by_address(
        address: &str,
        exclude_del: bool,
        pool: &std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<MultisigAccountEntity, crate::ServiceError> {
        let mut conditions = vec![("address", address)];
        if exclude_del {
            conditions.push(("is_del", "0"));
        }

        let account = MultisigAccountDaoV1::find_by_conditions(conditions, pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?
            .ok_or(crate::BusinessError::MultisigAccount(
                crate::MultisigAccountError::NotFound,
            ))?;
        Ok(account)
    }

    pub async fn list(
        pool: &std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigAccountEntity>, crate::ServiceError> {
        let accounts = MultisigAccountDaoV1::list(vec![], pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?;
        Ok(accounts)
    }

    pub async fn queue_by_id(
        queue_id: &str,
        pool: &std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<MultisigQueueEntity, crate::ServiceError> {
        let res = MultisigQueueDaoV1::find_by_id(queue_id, pool.as_ref())
            .await
            .map_err(|e| crate::SystemError::Database(e.into()))?
            .ok_or(crate::BusinessError::MultisigQueue(
                crate::MultisigQueueError::NotFound,
            ))?;

        Ok(res)
    }

    pub async fn logic_delete_account(
        account_id: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<(), crate::ServiceError> {
        MultisigAccountDaoV1::logic_del_multisig_account(account_id, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        wallet_database::dao::multisig_member::MultisigMemberDaoV1::logic_del_multisig_member(
            account_id, &*pool,
        )
        .await
        .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        let queues = MultisigQueueDaoV1::logic_del_multisig_queue(account_id, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?
            .into_iter()
            .map(|queue| queue.id)
            .collect();
        wallet_database::dao::multisig_signatures::MultisigSignatureDaoV1::logic_del_multi_multisig_signatures(queues, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        Ok(())
    }

    pub async fn physical_delete_account(
        members: &[wallet_database::entities::multisig_member::MultisigMemberEntity],
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigAccountEntity>, crate::ServiceError> {
        let mut res = Vec::new();
        for member in members {
            let mut multisig_account =
                Self::physical_delete_multisig_data(&member.account_id, pool.clone()).await?;
            if let Some(multisig_account) = multisig_account.pop() {
                res.push(multisig_account);
            }
        }

        Ok(res)
    }

    pub async fn physical_delete_wallet_account(
        members: wallet_database::entities::multisig_member::MultisigMemberEntities,
        uid: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigAccountEntity>, crate::ServiceError> {
        let mut res = Vec::new();
        for member in members.0 {
            // 如果有多个钱包都参与了多签,那么不删除这个account_id的多签资产
            // 如何判断呢?
            // 查询这个account_id下所有的member， member和钱包之间有映射关系，如果钱包表中的其他钱包也参与了多签,那么不删除这个account_id的多签资产
            let account_id = member.account_id;
            // 过滤掉uid
            let uids = WalletEntity::uid_list(&*pool)
                .await?
                .into_iter()
                .filter(|u| u.0 != uid)
                .map(|uid| uid.0)
                .collect::<Vec<String>>();

            let members = MultisigMemberDaoV1::list_by_uids(&uids, &*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
            // 如果members中有参与了多签的,那么不删除这个account_id的多签资产
            if members.iter().any(|m| m.account_id == account_id) {
                continue;
            }
            let mut multisig_account =
                Self::physical_delete_multisig_data(&account_id, pool.clone()).await?;
            if let Some(multisig_account) = multisig_account.pop() {
                res.push(multisig_account);
            }
        }

        Ok(res)
    }

    async fn physical_delete_multisig_data(
        account_id: &str,
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigAccountEntity>, crate::ServiceError> {
        let multisig_account =
            MultisigAccountDaoV1::physical_del_multisig_account(&account_id, &*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        // member也不能删除,因为可能还有其他的账户参与了多签
        wallet_database::dao::multisig_member::MultisigMemberDaoV1::physical_del_multisig_member(
            &account_id,
            &*pool,
        )
        .await
        .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        // queue也不能删除,因为可能还有其他的账户参与了多签
        let queues = MultisigQueueDaoV1::physical_del_multisig_queue(&account_id, &*pool)
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?
            .into_iter()
            .map(|queue| queue.id)
            .collect();
        // signatures也不能删除,因为可能还有其他的账户参与了多签
        wallet_database::dao::multisig_signatures::MultisigSignatureDaoV1::physical_del_multi_multisig_signatures(&*pool,queues, )
    .await
    .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        Ok(multisig_account)
    }

    pub async fn physical_delete_all_account(
        pool: std::sync::Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigAccountEntity>, crate::ServiceError> {
        let accounts = MultisigAccountDaoV1::physical_del_multi_multisig_account(&*pool, &[])
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        wallet_database::dao::multisig_member::MultisigMemberDaoV1::physical_del_multi_multisig_member(&*pool, &[])
        .await
        .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        MultisigQueueDaoV1::physical_del_multi_multisig_queue(&*pool, &[])
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        wallet_database::dao::multisig_signatures::MultisigSignatureDaoV1::physical_del_multi_multisig_signatures(&*pool,Vec::new() )
            .await
            .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;
        Ok(accounts)
    }
}
