use wallet_chain_interact::tron::operations::stake::{DelegateArgs, UnDelegateArgs};
use wallet_database::{
    CoreDbPool, DbPool,
    entities::{
        api_resource_delegation::{ApiResourceDelegationEntity, ApiResourceDelegationMode},
        permission::PermissionWithUserEntity,
    },
    repositories::{api_wallet::account::ApiAccountRepo, permission::PermissionRepo},
};

use crate::{
    context::Context, domain::chain::adapter::ChainAdapterFactory, error::service::ServiceError,
    messaging::mqtt::topics::NewPermissionUser,
};
use wallet_types::constant::chain_code;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResourceDelegationSigner {
    pub(crate) signer_address: String,
    pub(crate) permission_id: Option<i64>,
}

pub(crate) async fn resolve_resource_delegation_signer(
    ctx: &Context,
    delegation: &ApiResourceDelegationEntity,
) -> Result<ResourceDelegationSigner, ServiceError> {
    match delegation.delegation_mode {
        ApiResourceDelegationMode::WithdrawAddress => Ok(ResourceDelegationSigner {
            signer_address: delegation.owner_address.clone(),
            permission_id: None,
        }),
        ApiResourceDelegationMode::AuthorizedAddress => {
            resolve_authorized_resource_signer(ctx, delegation).await
        }
    }
}

pub(crate) fn new_tron_delegate_args(
    owner_address: &str,
    receiver_address: &str,
    trx_amount: i64,
    resource: &str,
    permission_id: Option<i64>,
) -> wallet_chain_interact::Result<DelegateArgs> {
    let args = DelegateArgs::new(owner_address, receiver_address, trx_amount, resource)?;
    Ok(match permission_id {
        Some(permission_id) => args.with_permission_id(permission_id),
        None => args,
    })
}

pub(crate) fn new_tron_undelegate_args(
    owner_address: &str,
    receiver_address: &str,
    trx_amount: i64,
    resource: &str,
    permission_id: Option<i64>,
) -> wallet_chain_interact::Result<UnDelegateArgs> {
    UnDelegateArgs::new(owner_address, receiver_address, trx_amount, resource, permission_id)
}

async fn resolve_authorized_resource_signer(
    ctx: &Context,
    delegation: &ApiResourceDelegationEntity,
) -> Result<ResourceDelegationSigner, ServiceError> {
    let permission_id = authorized_resource_permission_id(delegation)?;

    let core_pool = CoreDbPool::new(ctx.get_global_sqlite_pool()?);
    let permission = find_authorized_permission_with_recovery(
        &core_pool,
        &delegation.owner_address,
        permission_id,
        || async {
            let pool = ctx.get_global_sqlite_pool()?;
            recover_api_wallet_authorized_permission_from_chain(
                ctx,
                &pool,
                &delegation.owner_address,
                permission_id,
            )
            .await
        },
    )
    .await?
    .ok_or_else(|| {
        ServiceError::Parameter(format!(
            "authorized resource permission not found: owner={}, permissionId={}",
            delegation.owner_address, permission_id
        ))
    })?;

    if permission.permission.grantor_addr != delegation.owner_address {
        return Err(ServiceError::Parameter(format!(
            "authorized resource permission owner mismatch: trade_no={}, owner={}, permission_owner={}",
            delegation.resource_trade_no,
            delegation.owner_address,
            permission.permission.grantor_addr
        )));
    }

    let api_wallet_pool = ctx.api_wallet_pool()?;
    for user in permission.user {
        if ApiAccountRepo::find_one_by_address_chain_code(
            &user.address,
            &delegation.chain_code,
            &api_wallet_pool,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?
        .is_some()
        {
            return Ok(ResourceDelegationSigner {
                signer_address: user.address,
                permission_id: Some(permission.permission.active_id),
            });
        }
    }

    Err(ServiceError::Parameter(format!(
        "authorized resource signer not found: owner={}, permissionId={}",
        delegation.owner_address, permission_id
    )))
}

fn authorized_resource_permission_id(
    delegation: &ApiResourceDelegationEntity,
) -> Result<&str, ServiceError> {
    delegation.permission_id.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
        || {
            ServiceError::Parameter(format!(
                "authorized resource delegation missing permissionId: trade_no={}",
                delegation.resource_trade_no
            ))
        },
    )
}

async fn recover_api_wallet_authorized_permission_from_chain(
    ctx: &Context,
    pool: &DbPool,
    owner_address: &str,
    permission_id: &str,
) -> Result<(), ServiceError> {
    let active_id = permission_id.parse::<i64>().map_err(|_| {
        ServiceError::Parameter(format!(
            "authorized resource permissionId must be active id: owner={}, permissionId={}",
            owner_address, permission_id
        ))
    })?;

    let chain = ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await?;
    let account = chain.account_info(owner_address).await?;
    let Some(active_permission) = account
        .active_permission
        .iter()
        .find(|permission| permission.id.unwrap_or_default() as i64 == active_id)
    else {
        tracing::warn!(
            owner_address = %owner_address,
            permission_id = %permission_id,
            "Authorized resource permission not found on chain"
        );
        return Ok(());
    };

    let permission_with_user = NewPermissionUser::try_from((active_permission, owner_address))?;
    let api_wallet_pool = ctx.api_wallet_pool()?;
    let mut users = Vec::with_capacity(permission_with_user.users.len());
    let mut has_local_api_signer = false;
    for mut user in permission_with_user.users {
        if ApiAccountRepo::find_one_by_address_chain_code(
            &user.address,
            chain_code::TRON,
            &api_wallet_pool,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))?
        .is_some()
        {
            user.is_self = 1;
            has_local_api_signer = true;
        }
        users.push(user);
    }

    if !has_local_api_signer {
        tracing::warn!(
            owner_address = %owner_address,
            permission_id = %permission_id,
            "Authorized resource permission recovered from chain but has no local API signer"
        );
        return Ok(());
    }

    let core_pool = CoreDbPool::new(pool.clone());
    if PermissionRepo::find_by_grantor_and_active(&core_pool, owner_address, active_id, true)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?
        .is_some()
    {
        PermissionRepo::update_with_user(&core_pool, &permission_with_user.permission, &users)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
    } else {
        PermissionRepo::add_with_user(&core_pool, &permission_with_user.permission, &users)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
    }
    Ok(())
}

async fn find_authorized_permission_with_recovery<F, Fut>(
    core_pool: &CoreDbPool,
    owner_address: &str,
    permission_id: &str,
    recover: F,
) -> Result<Option<PermissionWithUserEntity>, ServiceError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), ServiceError>>,
{
    if let Some(permission) =
        find_authorized_permission(core_pool, owner_address, permission_id).await?
    {
        return Ok(Some(permission));
    }

    tracing::info!(
        owner_address = %owner_address,
        permission_id = %permission_id,
        "Authorized resource permission missing locally, recovering permission facts"
    );
    recover().await?;

    find_authorized_permission(core_pool, owner_address, permission_id).await
}

async fn find_authorized_permission(
    core_pool: &CoreDbPool,
    owner_address: &str,
    permission_id: &str,
) -> Result<Option<PermissionWithUserEntity>, ServiceError> {
    if let Ok(active_id) = permission_id.parse::<i64>() {
        return PermissionRepo::permission_with_user(core_pool, owner_address, active_id, false)
            .await
            .map_err(|e| ServiceError::Database(e.into()));
    }

    let permission = PermissionRepo::find_option(core_pool, permission_id)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
    if let Some(permission) = permission {
        PermissionRepo::permission_with_user(
            core_pool,
            &permission.grantor_addr,
            permission.active_id,
            false,
        )
        .await
        .map_err(|e| ServiceError::Database(e.into()))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{find_authorized_permission_with_recovery, resolve_resource_delegation_signer};
    use crate::error::service::ServiceError;
    use chrono::Utc;
    use wallet_database::{
        CoreDbPool, SqliteContext,
        entities::{
            api_resource_delegation::{
                ApiResourceDelegationEntity, ApiResourceDelegationMode,
                ApiResourceDelegationOperationType, ApiResourceDelegationSource,
                ApiResourceDelegationStatus,
            },
            api_resource_type::ApiResourceType,
            api_trade_type::ApiTradeType,
            permission::PermissionEntity,
            permission_user::PermissionUserEntity,
        },
        repositories::permission::PermissionRepo,
    };

    fn base_delegation() -> ApiResourceDelegationEntity {
        ApiResourceDelegationEntity {
            id: 1,
            uid: "uid".to_string(),
            source: ApiResourceDelegationSource::Platform,
            operation_type: ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: None,
            origin_trade_type: Some(ApiTradeType::Collect as i64),
            resource_trade_no: "CD_missing_permission".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "T_authorized_owner".to_string(),
            receiver_address: "T_receiver".to_string(),
            delegation_mode: ApiResourceDelegationMode::AuthorizedAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: "1".to_string(),
            amount: "100".to_string(),
            status: ApiResourceDelegationStatus::Pending,
            task_ack_sent_at: None,
            task_ack_send_count: 0,
            building_at: None,
            last_broadcast_at: None,
            tx_hash: None,
            raw_tx: None,
            chain_success_at: None,
            chain_failed_at: None,
            transaction_time: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_exec_receipt_uploaded_at: None,
            result_status: None,
            tx_res_received_at: None,
            finished_at: None,
            result_ack_sent_at: None,
            result_ack_send_count: 0,
            result_payload: None,
            fail_type: None,
            err_code: None,
            err_msg: None,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn authorized_resource_permission_id_requires_permission_id_without_context() {
        let err = super::authorized_resource_permission_id(&base_delegation())
            .expect_err("missing permissionId should fail");

        assert!(err.to_string().contains("missing permissionId"));
        assert!(!err.to_string().contains("Account not found"));
    }

    #[tokio::test]
    async fn authorized_resource_permission_lookup_recovers_when_local_cache_missing()
    -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let db_root = dir.path().to_string_lossy().to_string();
        let core_pool = SqliteContext::new(&db_root, Some("data.db")).await?.into_core_db_pool()?;
        let owner = "T_authorized_owner";
        let signer = "T_platform_signer";
        let permission_id = "3";
        let permission_db_id = PermissionRepo::get_id(owner, 3);

        let recovered =
            find_authorized_permission_with_recovery(&core_pool, owner, permission_id, || {
                let core_pool = core_pool.clone();
                let permission_db_id = permission_db_id.clone();
                async move {
                    let now = Utc::now();
                    let permission = PermissionEntity {
                        id: permission_db_id.clone(),
                        name: "api-resource".to_string(),
                        grantor_addr: owner.to_string(),
                        types: "active".to_string(),
                        active_id: 3,
                        threshold: 1,
                        member: 1,
                        chain_code: "tron".to_string(),
                        operations: String::new(),
                        is_del: 0,
                        created_at: now,
                        updated_at: None,
                    };
                    let user = PermissionUserEntity {
                        id: None,
                        address: signer.to_string(),
                        grantor_addr: owner.to_string(),
                        permission_id: permission_db_id,
                        is_self: 1,
                        weight: 1,
                        created_at: now,
                        updated_at: None,
                    };
                    PermissionRepo::add_with_user(&core_pool, &permission, &[user])
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                    Ok::<(), ServiceError>(())
                }
            })
            .await?
            .expect("permission should be recovered");

        assert_eq!(recovered.permission.grantor_addr, owner);
        assert_eq!(recovered.permission.active_id, 3);
        assert_eq!(recovered.user.len(), 1);
        assert_eq!(recovered.user[0].address, signer);

        Ok(())
    }
}
