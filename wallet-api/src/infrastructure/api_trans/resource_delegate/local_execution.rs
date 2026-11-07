use crate::{
    context::Context,
    domain::api_wallet::trans::ApiTransDomain,
    error::service::ServiceError,
    infrastructure::api_trans::resource_delegation::{
        execute_resource_delegation, local_delegate_execution_allowed,
        mark_recovered_resource_delegation_failure,
        record_resource_uncertain_and_maybe_rebroadcast, resource_delegation_is_built_unbroadcast,
        resource_recovery_error_is_uncertain,
    },
};
use wallet_database::{
    ApiResourceDbPool, entities::api_resource_delegation::ApiResourceDelegationEntity,
    repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo,
};

#[derive(Debug)]
pub(crate) enum LocalDelegateExecutionOutcome {
    Skipped,
    Broadcasted { tx_hash: String },
    RecoveredSuccess(ApiResourceDelegationEntity),
    RecoveryUncertain { rebroadcasted: bool, origin_trade_no: Option<String> },
    RecoveredFailure(ApiResourceDelegationEntity),
}

pub(crate) async fn execute_local_delegate_step(
    ctx: &'static Context,
    pool: &ApiResourceDbPool,
    resource_trade_no: &str,
    rpc_purpose: &'static str,
) -> Result<LocalDelegateExecutionOutcome, ServiceError> {
    let existing = ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
    if !local_delegate_execution_allowed(&existing) {
        tracing::info!(
            resource_trade_no,
            source = ?existing.source,
            operation_type = ?existing.operation_type,
            finished_at = ?existing.finished_at,
            err_code = ?existing.err_code,
            "Skipping ineligible local delegation execution command"
        );
        return Ok(LocalDelegateExecutionOutcome::Skipped);
    }

    if existing.tx_hash.as_deref().map(str::trim).filter(|hash| !hash.is_empty()).is_some()
        && existing.last_broadcast_at.is_some()
    {
        return recover_local_delegate(ctx, pool, existing, rpc_purpose).await;
    }

    let affected = if resource_delegation_is_built_unbroadcast(&existing) {
        1
    } else {
        ApiResourceDelegationRepo::claim_build_slot(pool, resource_trade_no)
            .await
            .map_err(|e| ServiceError::Database(e.into()))?
    };
    if affected == 0 {
        return Ok(LocalDelegateExecutionOutcome::Skipped);
    }

    let delegation = ApiResourceDelegationRepo::get_by_resource_trade_no(pool, resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
    if delegation.tx_hash.is_some() && delegation.last_broadcast_at.is_some() {
        return Ok(LocalDelegateExecutionOutcome::Skipped);
    }

    let tx_hash = execute_resource_delegation(ctx, pool, &delegation, rpc_purpose).await?;
    Ok(LocalDelegateExecutionOutcome::Broadcasted { tx_hash })
}

async fn recover_local_delegate(
    ctx: &'static Context,
    pool: &ApiResourceDbPool,
    delegation: ApiResourceDelegationEntity,
    rpc_purpose: &'static str,
) -> Result<LocalDelegateExecutionOutcome, ServiceError> {
    if resource_delegation_is_built_unbroadcast(&delegation) {
        let tx_hash = execute_resource_delegation(ctx, pool, &delegation, rpc_purpose).await?;
        return Ok(LocalDelegateExecutionOutcome::Broadcasted { tx_hash });
    }

    let Some(tx_hash) =
        delegation.tx_hash.as_deref().map(str::trim).filter(|hash| !hash.is_empty())
    else {
        return Ok(LocalDelegateExecutionOutcome::Skipped);
    };

    match ApiTransDomain::process_recovered_tx(
        ctx,
        &delegation.chain_code,
        &delegation.owner_address,
        tx_hash,
        0,
        "0",
    )
    .await
    {
        Ok(Some(resp)) => {
            let transaction_time = resp
                .transaction_time_ms
                .and_then(|millis| {
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
                })
                .map(|time| time.to_rfc3339());
            let payload = format!("local_delegation_recovered:{}", resp.tx_hash);
            ApiResourceDelegationRepo::mark_recovered_chain_success(
                pool,
                &delegation.resource_trade_no,
                tx_hash,
                transaction_time.as_deref(),
                &payload,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            let refreshed = ApiResourceDelegationRepo::get_by_resource_trade_no(
                pool,
                &delegation.resource_trade_no,
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            Ok(LocalDelegateExecutionOutcome::RecoveredSuccess(refreshed))
        }
        Ok(None) => {
            let rebroadcasted = record_resource_uncertain_and_maybe_rebroadcast(
                pool,
                &delegation.resource_trade_no,
            )
            .await?;
            Ok(LocalDelegateExecutionOutcome::RecoveryUncertain {
                rebroadcasted,
                origin_trade_no: delegation.origin_trade_no,
            })
        }
        Err(error) if resource_recovery_error_is_uncertain(&error) => {
            let rebroadcasted = record_resource_uncertain_and_maybe_rebroadcast(
                pool,
                &delegation.resource_trade_no,
            )
            .await?;
            Ok(LocalDelegateExecutionOutcome::RecoveryUncertain {
                rebroadcasted,
                origin_trade_no: delegation.origin_trade_no,
            })
        }
        Err(error) => {
            let recovery_context = if delegation.result_payload.as_deref()
                == Some("historical_chain_reconciliation_pending")
            {
                "historical_chain_reconciliation_confirmed_failure:local_delegation"
            } else {
                "local_delegation_recovered_failure"
            };
            let failed = mark_recovered_resource_delegation_failure(
                pool,
                &delegation,
                tx_hash,
                &error,
                recovery_context,
            )
            .await?;
            Ok(LocalDelegateExecutionOutcome::RecoveredFailure(failed))
        }
    }
}
