use tracing::info;
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool,
    entities::{
        api_resource_delegation::{ApiResourceDelegationEntity, ApiResourceDelegationResultStatus},
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
        withdraw::ApiWithdrawRepo,
    },
};

use crate::{
    error::service::ServiceError,
    infrastructure::api_trans::{
        resource_ack_type::is_original_order_resource_result_fact,
        resource_chain_facts::{resource_chain_failed, resource_chain_success},
    },
};

pub(crate) async fn mark_result_ack_and_project_gate(
    resource_pool: &ApiResourceDbPool,
    transaction_pool: &ApiTransactionDbPool,
    task: &ApiResourceDelegationEntity,
    source: &'static str,
) -> Result<(), ServiceError> {
    let release = gate_release_from_result(task);
    let origin_trade_type =
        task.origin_trade_type.map(|x| x.to_string()).unwrap_or_else(|| "None".to_string());
    let origin_trade_no = task.origin_trade_no.as_deref().unwrap_or("None");
    let (release_origin, release_result) = release
        .as_ref()
        .map(|(o, r)| (o.as_str(), r.to_string()))
        .unwrap_or(("None", "None".to_string()));
    info!(
        resource_trade_no = %task.resource_trade_no,
        origin_trade_no = %origin_trade_no,
        origin_trade_type = %origin_trade_type,
        gate_release_origin = %release_origin,
        gate_release_result = %release_result,
        result_status = ?task.result_status,
        err_code = %task.err_code.as_deref().unwrap_or("None"),
        source = source,
        "Marking resource delegation result ACK and projecting origin gate"
    );

    // The gate projection is idempotent and lives in a different SQLite DB.
    // Complete it before marking the resource ACK so a projection failure
    // remains visible to the ACK retry scanner.
    if let Some((origin, result)) = release.as_ref() {
        match task.origin_trade_type {
            Some(x) if x == ApiTradeType::Collect as i64 => {
                ApiCollectRepo::mark_resource_released(transaction_pool, origin, *result)
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
            }
            Some(x) if x == ApiTradeType::Withdraw as i64 => {
                if *result == ApiResourceGateResult::ResourceDelegationSuccess {
                    ApiWithdrawRepo::mark_resource_delegation_confirmed(transaction_pool, origin)
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                } else {
                    ApiWithdrawRepo::mark_resource_released(transaction_pool, origin, *result)
                        .await
                        .map_err(|e| ServiceError::Database(e.into()))?;
                }
            }
            _ => {}
        }
    }

    ApiResourceDelegationRepo::mark_result_ack_sent(resource_pool, &task.resource_trade_no)
        .await
        .map_err(|e| ServiceError::Database(e.into()))?;
    Ok(())
}

pub(crate) async fn project_gate_if_failure(
    transaction_pool: &ApiTransactionDbPool,
    task: &ApiResourceDelegationEntity,
    source: &'static str,
) -> Result<(), ServiceError> {
    if !is_failure(task) {
        return Ok(());
    }
    let Some(origin_trade_no) = task.origin_trade_no.as_deref() else {
        return Ok(());
    };
    match task.origin_trade_type {
        Some(x) if x == ApiTradeType::Collect as i64 => {
            let rows = ApiCollectRepo::mark_resource_blocked(
                transaction_pool,
                origin_trade_no,
                ApiResourceBlockReason::NeedLocalDelegate,
                None,
                Some(ApiResourceDependencyType::LocalDelegate),
            )
            .await
            .map_err(|e| ServiceError::Database(e.into()))?;
            info!(
                origin_trade_no = %origin_trade_no,
                resource_trade_no = %task.resource_trade_no,
                source = source,
                rows_affected = %rows,
                "Switch collect resource gate to local delegation after platform failure"
            );
        }
        Some(x) if x == ApiTradeType::Withdraw as i64 => {
            let result = ApiResourceGateResult::ResourceDelegationFailedBypass;
            let rows =
                ApiWithdrawRepo::mark_resource_released(transaction_pool, origin_trade_no, result)
                    .await
                    .map_err(|e| ServiceError::Database(e.into()))?;
            info!(
                origin_trade_no = %origin_trade_no,
                resource_trade_no = %task.resource_trade_no,
                source = source,
                rows_affected = %rows,
                release_reason = ?result,
                "Release withdraw gate by delegation failure bypass"
            );
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn gate_release_from_result(
    task: &ApiResourceDelegationEntity,
) -> Option<(String, ApiResourceGateResult)> {
    let origin = task.origin_trade_no.clone()?;
    let result = if is_original_order_resource_result_fact(task) {
        match task.result_status {
            Some(ApiResourceDelegationResultStatus::Success)
                if task.origin_trade_type == Some(ApiTradeType::Collect as i64) =>
            {
                ApiResourceGateResult::PlatformDelegateSuccess
            }
            Some(ApiResourceDelegationResultStatus::Success) => {
                ApiResourceGateResult::ResourceDelegationSuccess
            }
            Some(ApiResourceDelegationResultStatus::Fail) => {
                ApiResourceGateResult::ResourceDelegationFailedBypass
            }
            None => return None,
        }
    } else if task.origin_trade_type == Some(ApiTradeType::Collect as i64)
        && resource_chain_success(task)
        && !resource_chain_failed(task)
    {
        ApiResourceGateResult::PlatformDelegateSuccess
    } else if resource_chain_success(task) && !resource_chain_failed(task) {
        ApiResourceGateResult::ResourceDelegationSuccess
    } else if is_failure(task) {
        ApiResourceGateResult::ResourceDelegationFailedBypass
    } else {
        return None;
    };
    Some((origin, result))
}

fn is_failure(task: &ApiResourceDelegationEntity) -> bool {
    resource_chain_failed(task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use wallet_database::entities::{
        api_resource_delegation::{
            ApiResourceDelegationMode, ApiResourceDelegationOperationType,
            ApiResourceDelegationSource, ApiResourceDelegationStatus,
        },
        api_resource_type::ApiResourceType,
    };

    fn task(origin_trade_type: ApiTradeType) -> ApiResourceDelegationEntity {
        let now = Utc::now();
        ApiResourceDelegationEntity {
            id: 1,
            uid: "uid".to_string(),
            source: ApiResourceDelegationSource::Platform,
            operation_type: ApiResourceDelegationOperationType::Delegate,
            origin_trade_no: Some("ORIGIN1".to_string()),
            origin_trade_type: Some(origin_trade_type as i64),
            resource_trade_no: "RSC1".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: "receiver".to_string(),
            delegation_mode: ApiResourceDelegationMode::WithdrawAddress,
            permission_id: None,
            resource_type: ApiResourceType::Energy,
            native_amount: "199".to_string(),
            amount: "14650".to_string(),
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
            created_at: now,
            updated_at: None,
        }
    }

    #[test]
    fn resource_gate_projection_original_collect_success_uses_platform_success() {
        let mut record = task(ApiTradeType::Collect);
        record.origin_trade_no = Some(record.resource_trade_no.clone());
        record.result_status = Some(ApiResourceDelegationResultStatus::Success);

        assert_eq!(
            gate_release_from_result(&record),
            Some(("RSC1".to_string(), ApiResourceGateResult::PlatformDelegateSuccess))
        );
    }

    #[test]
    fn resource_gate_projection_withdraw_success_uses_resource_success() {
        let mut record = task(ApiTradeType::Withdraw);
        record.chain_success_at = Some(Utc::now());

        assert_eq!(
            gate_release_from_result(&record),
            Some(("ORIGIN1".to_string(), ApiResourceGateResult::ResourceDelegationSuccess))
        );
    }

    #[test]
    fn resource_gate_projection_failure_uses_failed_bypass() {
        let mut record = task(ApiTradeType::Withdraw);
        record.err_code = Some("ERR_6008".to_string());

        assert_eq!(
            gate_release_from_result(&record),
            Some(("ORIGIN1".to_string(), ApiResourceGateResult::ResourceDelegationFailedBypass))
        );
    }
}
