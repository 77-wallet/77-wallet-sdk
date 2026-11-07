#![allow(deprecated)]

use crate::{
    context::Context,
    messaging::notify::{FrontendNotifyEvent, api_wallet::CollectFront, event::NotifyEvent},
    request::api_wallet::trans::ApiCollectReq,
};
use std::time::Instant;
use wallet_database::{
    entities::api_collect::ApiCollectStatus,
    repositories::api_wallet::{collect::ApiCollectRepo, wallet::ApiWalletRepo},
};

pub struct ApiCollectDomain {}

fn is_row_not_found_db_error(err: &wallet_database::Error) -> bool {
    matches!(
        err,
        wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::RowNotFound
        ))
    )
}

fn collect_confirmation_status_matches(
    current: ApiCollectStatus,
    expected: ApiCollectStatus,
    has_transaction_time: bool,
) -> bool {
    if has_transaction_time {
        return true;
    }
    match expected {
        ApiCollectStatus::Success => {
            matches!(current, ApiCollectStatus::Success | ApiCollectStatus::ConfirmSuccessReport)
        }
        ApiCollectStatus::Failure => {
            matches!(current, ApiCollectStatus::Failure | ApiCollectStatus::ConfirmFailureReport)
        }
        _ => current == expected,
    }
}

async fn verify_collect_confirmation_after_zero_update(
    pool: &wallet_database::ApiTransactionDbPool,
    trade_no: &str,
    expected: ApiCollectStatus,
) -> Result<(), crate::error::service::ServiceError> {
    let refreshed = ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await?;
    if collect_confirmation_status_matches(
        refreshed.status,
        expected,
        refreshed.transaction_time.is_some(),
    ) {
        tracing::info!(
            trade_no = %trade_no,
            persisted_status = %refreshed.status,
            expected_status = %expected,
            has_transaction_time = %refreshed.transaction_time.is_some(),
            "Collect confirmation was already persisted or advanced concurrently"
        );
        return Ok(());
    }

    Err(crate::error::system::SystemError::Internal(format!(
        "collect confirmation status mismatch: trade_no={trade_no}, expected={expected}, persisted={}",
        refreshed.status
    ))
    .into())
}

impl ApiCollectDomain {
    pub(crate) async fn collect_v2_with_ctx(
        ctx: &'static Context,
        req: &ApiCollectReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        let start_time = Instant::now();
        tracing::info!(
            "开始处理归集交易v2, trade_no: {}, uid: {}, from: {}, to: {}, value: {}, chain: {}, token: {}, symbol: {}, start_time: {:?}",
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

        let api_wallet_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;

        // 1. 校验 + 查钱包
        let wallet = ApiWalletRepo::find_by_uid(&api_wallet_pool, &req.uid).await?.ok_or(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::Wallet(
                    crate::error::business::api_wallet::wallet::WalletError::NotFound,
                ),
            ),
        )?;
        let wallet_find_time = Instant::now();

        tracing::info!(trade_no=%req.trade_no, "找到钱包: name={}, 耗时: {:?}", wallet.name, wallet_find_time - start_time);

        // 2. upsert_api_collect（事实落库）
        let res =
            ApiCollectRepo::get_api_collect_by_trade_no(&api_transaction_pool, &req.trade_no).await;
        let tx_check_time = Instant::now();
        tracing::info!(trade_no=%req.trade_no, "检查交易记录, 耗时: {:?}", tx_check_time - wallet_find_time);

        let is_existing_trade = match res {
            Ok(_) => true,
            Err(e) if is_row_not_found_db_error(&e) => false,
            Err(e) => return Err(e.into()),
        };

        if !is_existing_trade {
            tracing::info!(trade_no=%req.trade_no, "未找到现有交易记录，开始插入新记录");
            let insert_time = Instant::now();
            ApiCollectRepo::upsert_api_collect(
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
                ApiCollectStatus::Init,
                req.risk_addr,
            )
            .await?;

            tracing::info!(trade_no=%req.trade_no, "成功插入/更新归集交易记录, 耗时: {:?}", insert_time.elapsed());
        } else {
            tracing::warn!(trade_no=%req.trade_no, "归集交易记录已存在，跳过插入");
        }

        let data = NotifyEvent::Collect(CollectFront {
            uid: req.uid.to_string(),
            from_addr: req.from.to_string(),
            to_addr: req.to.to_string(),
            value: req.value.to_string(),
        });
        tracing::info!(trade_no=%req.trade_no, "发送前端通知");
        let notify_time = Instant::now();
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        tracing::info!(trade_no=%req.trade_no, "前端通知发送成功, 耗时: {:?}", notify_time.elapsed());

        // 注意：在 v2 架构下，不再需要显式提交交易
        // Shadow Scanner 会在下一轮扫描中自动发现新记录并推进执行
        // 交易执行完全由事实驱动，而不是命令式触发

        // 3. 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_collect_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_collect(&req.trade_no).await {
                    tracing::warn!(trade_no=%req.trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%req.trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        tracing::info!(trade_no=%req.trade_no, "归集交易v2处理完成, 总耗时: {:?}", start_time.elapsed());
        Ok(())
    }

    /// recover 的语义：
    /// 修复“手续费不足”这一事实，使交易重新具备构建条件
    /// 不做任何状态回滚，不保证一定继续推进
    pub async fn recover(
        ctx: &Context,
        trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let start_time = Instant::now();
        tracing::info!(trade_no=%trade_no, "开始恢复归集交易");

        let pool = ctx.api_transaction_pool()?;

        // 1. 解除事实阻断（核心）
        tracing::info!(trade_no=%trade_no, "清除服务费需求标记");
        let clear_time = Instant::now();
        ApiCollectRepo::clear_need_service_fee(&pool, trade_no).await?;
        tracing::info!(trade_no=%trade_no, "服务费需求标记清除成功, 耗时: {:?}", clear_time.elapsed());

        // 2. 快速触发 Shadow
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_collect_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_collect(trade_no).await {
                    tracing::warn!(trade_no=%trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        tracing::info!(
            trade_no=%trade_no,
            "归集交易 recover 完成, 耗时: {:?}",
            start_time.elapsed()
        );
        Ok(())
    }

    pub async fn confirm_tx(
        ctx: &Context,
        trade_no: &str,
        status: bool,
        fail_type: i32,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_transaction_pool()?;
        Self::confirm_tx_in_pool(&pool, trade_no, status, fail_type).await?;

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_collect_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_collect(trade_no).await {
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
        fail_type: i32,
    ) -> Result<(), crate::error::service::ServiceError> {
        let start_time = Instant::now();
        tracing::info!(trade_no=%trade_no, "开始确认归集交易, 状态: {}, 失败类型: {}, start_time: {:?}", status, fail_type, start_time);

        tracing::info!(trade_no=%trade_no, "查询交易记录");
        let query_time = Instant::now();
        let mut tx = match ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await {
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
                        fail_type = %fail_type,
                        error = %e,
                        "collect confirm_tx: trade_no not found (idempotent ignore; record may be cleaned, message already acked upstream)"
                    );
                    return Ok(());
                }
                tracing::warn!(
                    trade_no = %trade_no,
                    status = %status,
                    fail_type = %fail_type,
                    error = %e,
                    "collect confirm_tx: failed to load trade record"
                );
                return Err(e.into());
            }
        };
        tracing::info!(trade_no=%trade_no, "找到交易记录, 当前状态: {:?}, 耗时: {:?}", tx.status, query_time.elapsed());

        let has_broadcast_fact = tx.last_broadcast_at.is_some();
        let has_non_empty_tx_hash =
            tx.tx_hash.as_ref().map(|h| !h.trim().is_empty()).unwrap_or(false);
        let is_pre_broadcast_fee_fail = !status && fail_type == 2 && !has_broadcast_fact;

        // ====== 必须先确保 transaction_time 事实存在，再做任何 repeat early return ======
        if tx.transaction_time.is_none() && !is_pre_broadcast_fee_fail {
            let now = chrono::Utc::now().to_rfc3339();
            let rows = ApiCollectRepo::confirm_transaction_time_if_absent(pool, trade_no, &now)
                .await
                .map_err(|e| {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        fail_type = %fail_type,
                        error = %e,
                        "collect confirm_tx: confirm_transaction_time_if_absent failed (will NOT ack)"
                    );
                    e
                })?;

            if rows == 0 {
                // 并发场景：可能已被其他路径写入；重查一次确认事实
                tx = ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await?;
                if tx.transaction_time.is_none() {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        fail_type = %fail_type,
                        "collect confirm_tx: expected transaction_time to be set, but still NULL after retry (will NOT ack)"
                    );
                    return Err(crate::error::system::SystemError::Internal(
                        "transaction_time still NULL after confirm_transaction_time_if_absent"
                            .to_string(),
                    )
                    .into());
                }
            } else {
                // 写入成功后刷新一次，保证后续判断基于最新事实
                tx = ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await?;
            }
        } else if tx.transaction_time.is_none() && is_pre_broadcast_fee_fail {
            tracing::warn!(
                trade_no = %trade_no,
                status = %status,
                fail_type = %fail_type,
                last_broadcast_at_present = %has_broadcast_fact,
                tx_hash_present = %has_non_empty_tx_hash,
                "collect confirm_tx: skip confirm_transaction_time_if_absent for pre-broadcast fee failure"
            );
        }

        let update_time = Instant::now();
        if status {
            if tx.status == ApiCollectStatus::Success
                || tx.status == ApiCollectStatus::ConfirmSuccessReport
            {
                tracing::warn!(trade_no=%trade_no, "归集交易确认重复");
                return Ok(());
            }

            let rows_affected = ApiCollectRepo::update_api_collect_next_status(
                pool,
                trade_no,
                ApiCollectStatus::SendingTxReport,
                ApiCollectStatus::Success,
            )
            .await?;
            if rows_affected == 0 {
                verify_collect_confirmation_after_zero_update(
                    pool,
                    trade_no,
                    ApiCollectStatus::Success,
                )
                .await?;
            }
        } else {
            if tx.status == ApiCollectStatus::Failure
                || tx.status == ApiCollectStatus::ConfirmFailureReport
            {
                tracing::warn!(trade_no=%trade_no, "归集交易确认重复");
                return Ok(());
            }

            if fail_type == 2 && !has_broadcast_fact {
                tracing::info!(trade_no=%trade_no, "更新交易状态为失败(余额不足)");
                let rows_affected = ApiCollectRepo::update_api_collect_next_status_and_err(
                    pool,
                    trade_no,
                    tx.status,
                    ApiCollectStatus::Failure,
                    6002,
                    "confirm transfer fee failed insufficient balance",
                )
                .await?;
                if rows_affected == 0 {
                    verify_collect_confirmation_after_zero_update(
                        pool,
                        trade_no,
                        ApiCollectStatus::Failure,
                    )
                    .await?;
                }
                tracing::info!(trade_no=%trade_no, "交易状态更新成功");

                let finished_rows = ApiCollectRepo::mark_chain_finished(pool, trade_no).await?;
                if finished_rows != 1 {
                    tracing::warn!(
                        trade_no = %trade_no,
                        rows_affected = finished_rows,
                        "pre-broadcast collect failure did not mark lifecycle finished"
                    );
                }
            } else {
                tracing::info!(trade_no=%trade_no, "更新交易状态为失败");
                let rows_affected = ApiCollectRepo::update_api_collect_next_status(
                    pool,
                    trade_no,
                    ApiCollectStatus::SendingTxReport,
                    ApiCollectStatus::Failure,
                )
                .await?;
                if rows_affected == 0 {
                    verify_collect_confirmation_after_zero_update(
                        pool,
                        trade_no,
                        ApiCollectStatus::Failure,
                    )
                    .await?;
                }
                tracing::info!(trade_no=%trade_no, "交易状态更新成功");
            }
        }
        tracing::info!(trade_no=%trade_no, "更新交易状态, 耗时: {:?}", update_time.elapsed());

        tracing::info!(trade_no=%trade_no, "归集交易确认完成, 总耗时: {:?}", start_time.elapsed());
        Ok(())
    }
}

impl ApiCollectDomain {}

#[cfg(test)]
mod tests {
    use super::{collect_confirmation_status_matches, is_row_not_found_db_error};
    use wallet_database::entities::api_collect::ApiCollectStatus;

    #[test]
    fn collect_row_not_found_guard() {
        let not_found = wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::RowNotFound,
        ));
        assert!(is_row_not_found_db_error(&not_found));

        let other = wallet_database::Error::Database(wallet_database::DatabaseError::QueryFailed);
        assert!(!is_row_not_found_db_error(&other));
    }

    #[test]
    fn collect_confirmation_status_accepts_concurrent_result_and_terminal_states() {
        assert!(collect_confirmation_status_matches(
            ApiCollectStatus::Success,
            ApiCollectStatus::Success,
            false
        ));
        assert!(collect_confirmation_status_matches(
            ApiCollectStatus::ConfirmSuccessReport,
            ApiCollectStatus::Success,
            false
        ));
        assert!(collect_confirmation_status_matches(
            ApiCollectStatus::Failure,
            ApiCollectStatus::Failure,
            false
        ));
        assert!(collect_confirmation_status_matches(
            ApiCollectStatus::ConfirmFailureReport,
            ApiCollectStatus::Failure,
            false
        ));
        assert!(collect_confirmation_status_matches(
            ApiCollectStatus::SendingTx,
            ApiCollectStatus::Success,
            true
        ));
        assert!(!collect_confirmation_status_matches(
            ApiCollectStatus::SendingTxReport,
            ApiCollectStatus::Success,
            false
        ));
    }
}
