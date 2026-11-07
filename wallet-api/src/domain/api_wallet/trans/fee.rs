#![allow(deprecated)]

use crate::{
    context::Context,
    error::{
        business::{
            BusinessError,
            api_wallet::{ApiWalletError, wallet::WalletError},
        },
        service::ServiceError,
    },
    messaging::notify::{FrontendNotifyEvent, api_wallet::FeeFront, event::NotifyEvent},
    request::api_wallet::trans::ApiTransferFeeReq,
};
use chrono::Utc;
use std::time::Instant;
use wallet_database::{
    entities::api_fee::ApiFeeStatus,
    repositories::api_wallet::{fee::ApiFeeRepo, wallet::ApiWalletRepo},
};

pub struct ApiFeeDomain {}

fn fee_confirmation_status_matches(
    current: ApiFeeStatus,
    expected: ApiFeeStatus,
    has_transaction_time: bool,
) -> bool {
    if has_transaction_time {
        return true;
    }
    match expected {
        ApiFeeStatus::Success => {
            matches!(current, ApiFeeStatus::Success | ApiFeeStatus::ConfirmSuccessReport)
        }
        ApiFeeStatus::Failure => {
            matches!(current, ApiFeeStatus::Failure | ApiFeeStatus::ConfirmFailureReport)
        }
        _ => current == expected,
    }
}

async fn verify_fee_confirmation_after_zero_update(
    pool: &wallet_database::ApiTransactionDbPool,
    trade_no: &str,
    expected: ApiFeeStatus,
) -> Result<(), ServiceError> {
    let refreshed = ApiFeeRepo::get_api_fee_by_trade_no(pool, trade_no).await?;
    if fee_confirmation_status_matches(
        refreshed.status,
        expected,
        refreshed.transaction_time.is_some(),
    ) {
        tracing::info!(
            trade_no = %trade_no,
            persisted_status = %refreshed.status,
            expected_status = %expected,
            has_transaction_time = %refreshed.transaction_time.is_some(),
            "Fee confirmation was already persisted or advanced concurrently"
        );
        return Ok(());
    }

    Err(crate::error::system::SystemError::Internal(format!(
        "fee confirmation status mismatch: trade_no={trade_no}, expected={expected}, persisted={}",
        refreshed.status
    ))
    .into())
}

impl ApiFeeDomain {
    pub(crate) async fn transfer_fee_with_ctx(
        ctx: &'static Context,
        req: &ApiTransferFeeReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        let start_time = Instant::now();
        tracing::info!(
            "开始处理手续费交易, trade_no: {}, uid: {}, from: {}, to: {}, value: {}, chain: {}, token: {}, symbol: {}, start_time: {:?}",
            req.trade_no,
            req.uid,
            req.from,
            req.to,
            req.value,
            req.chain_code,
            req.token_address.as_db_str(),
            req.symbol,
            start_time
        );

        // 获取数据库连接
        let core_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;

        // 获取钱包
        tracing::info!(trade_no=%req.trade_no, "查询钱包信息");
        let wallet = ApiWalletRepo::find_by_uid(&core_pool, &req.uid).await?.ok_or(
            BusinessError::ApiWallet(ApiWalletError::Wallet(WalletError::NotFound.into())),
        )?;
        let wallet_find_time = Instant::now();
        tracing::info!(trade_no=%req.trade_no, "找到钱包: name={}, 耗时: {:?}", wallet.name, wallet_find_time - start_time);

        tracing::info!(trade_no=%req.trade_no, "检查手续费交易记录");
        let res = ApiFeeRepo::get_api_fee_by_trade_no(&api_transaction_pool, &req.trade_no).await;
        let tx_check_time = Instant::now();
        tracing::info!(trade_no=%req.trade_no, "检查交易记录, 耗时: {:?}", tx_check_time - wallet_find_time);

        if res.is_err() {
            tracing::info!(trade_no=%req.trade_no, "未找到现有手续费交易记录，开始插入新记录");
            let insert_time = Instant::now();
            ApiFeeRepo::upsert_api_fee(
                &api_transaction_pool,
                &req.uid,
                &wallet.name,
                &req.from,
                &req.to,
                &req.value,
                &req.validate,
                &req.chain_code,
                req.token_address.to_option_string_for_api(),
                &req.symbol.to_uppercase(),
                &req.trade_no,
                req.trade_type,
            )
            .await?;
            tracing::info!(trade_no=%req.trade_no, "落盘手续费: 耗时: {:?}", insert_time.elapsed());

            tracing::info!(trade_no=%req.trade_no, "准备发送前端通知");
            let notify_time = Instant::now();
            let data = NotifyEvent::Fee(FeeFront {
                uid: req.uid.to_string(),
                from_addr: req.from.to_string(),
                to_addr: req.to.to_string(),
                value: req.value.to_string(),
            });
            FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
            tracing::info!(trade_no=%req.trade_no, "前端通知发送成功, 耗时: {:?}", notify_time.elapsed());
        } else {
            tracing::warn!(trade_no=%req.trade_no, "fee tx found, 交易记录已存在");
        }

        // 注意：在 v2 架构下，不再需要显式提交交易
        // Shadow Scanner 会在下一轮扫描中自动发现新记录并推进执行
        // 交易执行完全由事实驱动，而不是命令式触发

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_fee_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_fee(&req.trade_no).await {
                    tracing::warn!(trade_no=%req.trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%req.trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        tracing::info!(trade_no=%req.trade_no, "手续费交易处理完成, 总耗时: {:?}", start_time.elapsed());
        Ok(())
    }

    pub async fn confirm_tx(
        ctx: &'static Context,
        trade_no: &str,
        status: bool,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_transaction_pool()?;
        Self::confirm_tx_in_pool(&pool, trade_no, status).await?;

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_fee_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_fee(trade_no).await {
                    tracing::warn!(trade_no=%trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn confirm_tx_in_pool(
        pool: &wallet_database::ApiTransactionDbPool,
        trade_no: &str,
        status: bool,
    ) -> Result<(), ServiceError> {
        let start_time = Instant::now();
        tracing::info!(trade_no=%trade_no, "开始确认手续费交易, 状态: {}, start_time: {:?}", status, start_time);

        tracing::info!(trade_no=%trade_no, "查询手续费交易记录");
        let mut tx = match ApiFeeRepo::get_api_fee_by_trade_no(pool, trade_no).await {
            Ok(tx) => tx,
            Err(e) => {
                let is_row_not_found = matches!(
                    &e,
                    wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
                        sqlx::Error::RowNotFound
                    ))
                );
                if is_row_not_found {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        error = %e,
                        "fee confirm_tx: trade_no not found (idempotent ignore; record may be cleaned, message already acked upstream)"
                    );
                    return Ok(());
                }
                tracing::warn!(
                    trade_no = %trade_no,
                    status = %status,
                    error = %e,
                    "fee confirm_tx: failed to load trade record"
                );
                return Err(e.into());
            }
        };
        // tracing::info!(trade_no=%trade_no, "找到手续费交易记录, 当前状态: {:?}, 耗时: {:?}", tx.status, query_time.elapsed());

        let has_broadcast_fact = tx.last_broadcast_at.is_some();
        let has_non_empty_tx_hash =
            tx.tx_hash.as_ref().map(|h| !h.trim().is_empty()).unwrap_or(false);
        let is_pre_broadcast_fail = !status && !has_broadcast_fact;

        // ====== 必须先确保 transaction_time 事实存在，再做任何 repeat early return ======
        if tx.transaction_time.is_none() && !is_pre_broadcast_fail {
            let now = Utc::now().to_rfc3339();
            let rows = ApiFeeRepo::confirm_transaction_time_if_absent(pool, trade_no, &now)
                .await
                .map_err(|e| {
                tracing::warn!(
                    trade_no = %trade_no,
                    status = %status,
                    error = %e,
                    "fee confirm_tx: confirm_transaction_time_if_absent failed (will NOT ack)"
                );
                e
            })?;

            if rows == 0 {
                tx = ApiFeeRepo::get_api_fee_by_trade_no(pool, trade_no).await?;
                if tx.transaction_time.is_none() {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        "fee confirm_tx: expected transaction_time to be set, but still NULL after retry (will NOT ack)"
                    );
                    return Err(crate::error::system::SystemError::Internal(
                        "transaction_time still NULL after confirm_transaction_time_if_absent"
                            .to_string(),
                    )
                    .into());
                }
            } else {
                tx = ApiFeeRepo::get_api_fee_by_trade_no(pool, trade_no).await?;
            }
        } else if tx.transaction_time.is_none() && is_pre_broadcast_fail {
            tracing::warn!(
                trade_no = %trade_no,
                status = %status,
                last_broadcast_at_present = %has_broadcast_fact,
                tx_hash_present = %has_non_empty_tx_hash,
                "fee confirm_tx: skip confirm_transaction_time_if_absent for pre-broadcast failure"
            );
        }

        if status {
            if tx.status == ApiFeeStatus::Success || tx.status == ApiFeeStatus::ConfirmSuccessReport
            {
                tracing::warn!(trade_no=%trade_no, "fee confirmation repeat, 确认重复");
                return Ok(());
            }
        } else {
            if tx.status == ApiFeeStatus::Failure || tx.status == ApiFeeStatus::ConfirmFailureReport
            {
                tracing::warn!(trade_no=%trade_no, "fee confirmation repeat, 确认重复");
                return Ok(());
            }
        }

        tracing::info!(trade_no=%trade_no, "更新手续费交易状态");
        let update_time = Instant::now();
        let next_status: ApiFeeStatus =
            if status { ApiFeeStatus::Success } else { ApiFeeStatus::Failure };
        let rows_affected = ApiFeeRepo::update_api_fee_next_status(
            pool,
            trade_no,
            ApiFeeStatus::SendingTxReport,
            next_status,
        )
        .await?;
        tracing::info!(trade_no=%trade_no, "更新交易状态, 影响行数: {}, 耗时: {:?}", rows_affected, update_time.elapsed());

        if rows_affected == 0 {
            verify_fee_confirmation_after_zero_update(pool, trade_no, next_status).await?;
        }

        tracing::info!(trade_no=%trade_no, "手续费交易确认完成, 总耗时: {:?}", start_time.elapsed());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::fee_confirmation_status_matches;
    use wallet_database::entities::api_fee::ApiFeeStatus;

    #[test]
    fn fee_confirmation_status_accepts_concurrent_result_and_terminal_states() {
        assert!(fee_confirmation_status_matches(
            ApiFeeStatus::Success,
            ApiFeeStatus::Success,
            false
        ));
        assert!(fee_confirmation_status_matches(
            ApiFeeStatus::ConfirmSuccessReport,
            ApiFeeStatus::Success,
            false
        ));
        assert!(fee_confirmation_status_matches(
            ApiFeeStatus::Failure,
            ApiFeeStatus::Failure,
            false
        ));
        assert!(fee_confirmation_status_matches(
            ApiFeeStatus::ConfirmFailureReport,
            ApiFeeStatus::Failure,
            false
        ));
        assert!(fee_confirmation_status_matches(
            ApiFeeStatus::SendingTx,
            ApiFeeStatus::Success,
            true
        ));
        assert!(!fee_confirmation_status_matches(
            ApiFeeStatus::SendingTxReport,
            ApiFeeStatus::Success,
            false
        ));
    }
}
