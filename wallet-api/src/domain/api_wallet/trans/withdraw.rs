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
    messaging::notify::{FrontendNotifyEvent, api_wallet::WithdrawFront, event::NotifyEvent},
    request::api_wallet::trans::ApiWithdrawReq,
};
use chrono::Utc;
use wallet_database::{
    entities::{api_trade_type::ApiTradeType, api_withdraw::ApiWithdrawStatus},
    repositories::api_wallet::{wallet::ApiWalletRepo, withdraw::ApiWithdrawRepo},
};

pub struct ApiWithdrawDomain {}

#[derive(Debug)]
pub(crate) struct WithdrawConfirmOutcome {
    pub tx: wallet_database::entities::api_withdraw::ApiWithdrawEntity,
    pub should_notify: bool,
}

fn is_row_not_found_db_error(err: &wallet_database::Error) -> bool {
    matches!(
        err,
        wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::RowNotFound
        ))
    )
}

impl ApiWithdrawDomain {
    pub(crate) fn has_no_audit_decision(
        entity: &wallet_database::entities::api_withdraw::ApiWithdrawEntity,
    ) -> bool {
        entity.audit_passed_at.is_none() && entity.audit_rejected_at.is_none()
    }

    pub(crate) async fn withdraw_with_ctx(
        ctx: &'static Context,
        req: &ApiWithdrawReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        // 获取数据库连接
        let core_pool = ctx.api_wallet_pool()?;
        let api_transaction_pool = ctx.api_transaction_pool()?;
        // 获取钱包
        tracing::info!(trade_no=%req.trade_no, "查询钱包信息");
        let wallet = ApiWalletRepo::find_by_uid(&core_pool, &req.uid).await?.ok_or(
            BusinessError::ApiWallet(ApiWalletError::Wallet(WalletError::NotFound.into())),
        )?;

        let init_status =
            if req.audit == 1 { ApiWithdrawStatus::AuditPass } else { ApiWithdrawStatus::Init };
        let res = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &api_transaction_pool,
            &req.trade_no,
            ApiTradeType::Withdraw,
        )
        .await;
        let is_existing_trade = match res {
            Ok(_) => true,
            Err(e) if is_row_not_found_db_error(&e) => false,
            Err(e) => return Err(e.into()),
        };

        if !is_existing_trade {
            ApiWithdrawRepo::upsert_api_withdraw(
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
                req.out_order_id.clone(),
                req.client_id.clone(),
                req.create_time.clone(),
                ApiTradeType::Withdraw,
                0,
                None,
                init_status,
                ApiWithdrawStatus::InitOrder,
                "",
                "",
                None,
                None,
            )
            .await?;
            tracing::info!(trade_no=%req.trade_no, "upsert_api_withdraw (step 5): {}", init_status);
        } else {
            tracing::warn!(trade_no=%req.trade_no, "withdraw tx found");
        }

        let data = NotifyEvent::Withdraw(WithdrawFront {
            uid: req.uid.to_string(),
            from_addr: req.from.to_string(),
            to_addr: req.to.to_string(),
            value: req.value.to_string(),
        });
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        if req.audit == 1 {
            Self::sign_withdrawal_order(ctx, &req.trade_no).await?;
        }

        ApiWithdrawRepo::update_api_withdraw_status(
            &api_transaction_pool,
            &req.trade_no,
            init_status,
        )
        .await?;

        // 注意：在 v2 架构下，不再需要显式提交交易
        // Shadow Scanner 会在下一轮扫描中自动发现新记录并推进执行
        // 交易执行完全由事实驱动，而不是命令式触发

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_withdraw_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_withdraw(&req.trade_no).await {
                    tracing::warn!(trade_no=%req.trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%req.trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }
        Ok(())
    }

    pub async fn sign_withdrawal_order(
        ctx: &'static Context,
        trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_transaction_pool()?;
        // ApiWithdrawRepo::update_api_withdraw_status(&pool, trade_no, ApiWithdrawStatus::AuditPass)
        //     .await?;

        let audit_rows = ApiWithdrawRepo::set_audit_passed(&pool, trade_no).await?;
        tracing::info!(
            trade_no = %trade_no,
            audit_rows,
            "withdraw audit pass fact persisted"
        );

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_withdraw_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_withdraw(trade_no).await {
                    tracing::warn!(trade_no=%trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(
                        trade_no=%trade_no,
                        audit_rows,
                        "withdraw audit pass triggered Shadow fast path"
                    );
                }
            } else {
                tracing::warn!(
                    trade_no = %trade_no,
                    audit_rows,
                    "withdraw audit pass fast path skipped: shadow system unavailable"
                );
            }
        } else {
            tracing::warn!(
                trade_no = %trade_no,
                audit_rows,
                "withdraw audit pass fast path skipped: global handles unavailable"
            );
        }

        Ok(())
    }

    pub async fn reject_withdrawal_order(
        ctx: &'static Context,
        trade_no: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_transaction_pool()?;
        // ApiWithdrawRepo::update_api_withdraw_status_and_err(
        //     &pool,
        //     trade_no,
        //     ApiWithdrawStatus::AuditReject,
        //     ErrCode::UnknownError,
        //     "rejected",
        // )
        // .await?;

        ApiWithdrawRepo::set_audit_rejected(&pool, trade_no, "rejected").await?;

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_withdraw_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_withdraw(trade_no).await {
                    tracing::warn!(trade_no=%trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        Ok(())
    }

    pub async fn confirm_tx(
        ctx: &'static Context,
        trade_no: &str,
        status: bool,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_transaction_pool()?;
        let outcome = match Self::confirm_tx_in_pool(&pool, trade_no, status).await {
            Ok(outcome) => outcome,
            Err(e) => {
                let is_row_not_found = matches!(
                    &e,
                    ServiceError::Database(wallet_database::Error::Database(
                        wallet_database::DatabaseError::Sqlx(sqlx::Error::RowNotFound)
                    ))
                );
                if is_row_not_found {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        error = %e,
                        "withdraw confirm_tx: trade_no not found (idempotent ignore; record may be cleaned, message already acked upstream)"
                    );
                    return Ok(());
                }
                return Err(e);
            }
        };

        // 注意：在 v2 架构下，不再需要显式提交确认报告
        // Shadow Scanner 会在下一轮扫描中自动发现状态变化并触发确认报告
        // 交易执行完全由事实驱动，而不是命令式触发

        // 立即触发一次 Shadow 推进（快速通道）
        if let Some(handles) = ctx.get_global_handles().await.upgrade() {
            if let Some(shadow_system) =
                handles.get_global_processed_withdraw_tx_handle().get_shadow_system()
            {
                if let Err(e) = shadow_system.trigger_withdraw(trade_no).await {
                    tracing::warn!(trade_no=%trade_no, "触发 Shadow 推进失败，但不影响流程: {:?}", e);
                } else {
                    tracing::info!(trade_no=%trade_no, "成功触发 Shadow 快速通道推进");
                }
            }
        }

        // 仅在本次确实推进了新事实时才通知前端，避免重投导致重复业务侧效应
        if outcome.should_notify {
            let data = NotifyEvent::Withdraw(WithdrawFront {
                uid: outcome.tx.uid.to_string(),
                from_addr: outcome.tx.from_addr.to_string(),
                to_addr: outcome.tx.to_addr.to_string(),
                value: outcome.tx.value.to_string(),
            });
            FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        }

        Ok(())
    }

    pub(crate) async fn confirm_tx_in_pool(
        pool: &wallet_database::ApiTransactionDbPool,
        trade_no: &str,
        status: bool,
    ) -> Result<WithdrawConfirmOutcome, ServiceError> {
        let mut tx = match ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            pool,
            trade_no,
            ApiTradeType::Withdraw,
        )
        .await
        {
            Ok(tx) => tx,
            Err(e) => {
                tracing::warn!(
                    trade_no = %trade_no,
                    status = %status,
                    error = %e,
                    "withdraw confirm_tx: failed to load trade record"
                );
                return Err(e.into());
            }
        };

        let mut should_notify = false;
        let is_pre_broadcast_failure = !status && tx.last_broadcast_at.is_none();

        // ====== 必须先确保 transaction_time 事实存在，再做任何 repeat early return ======
        if tx.transaction_time.is_none() {
            let now = Utc::now().to_rfc3339();
            let rows = ApiWithdrawRepo::confirm_transaction_time_if_absent(pool, trade_no, &now)
                .await
                .map_err(|e| {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        error = %e,
                        "withdraw confirm_tx: confirm_transaction_time_if_absent failed (will NOT ack)"
                    );
                    e
                })?;

            if rows == 0 {
                tx = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
                    pool,
                    trade_no,
                    ApiTradeType::Withdraw,
                )
                .await?;
                if tx.transaction_time.is_none() {
                    tracing::warn!(
                        trade_no = %trade_no,
                        status = %status,
                        "withdraw confirm_tx: expected transaction_time to be set, but still NULL after retry (will NOT ack)"
                    );
                    return Err(crate::error::system::SystemError::Internal(
                        "transaction_time still NULL after confirm_transaction_time_if_absent"
                            .to_string(),
                    )
                    .into());
                }
            } else {
                should_notify = true;
                tx = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
                    pool,
                    trade_no,
                    ApiTradeType::Withdraw,
                )
                .await?;
            }
        }

        // repeat 判定（在 ensure transaction_time 之后）
        if status {
            if tx.status == ApiWithdrawStatus::Success
                || tx.status == ApiWithdrawStatus::ConfirmSuccessReport
            {
                tracing::warn!(trade_no=%trade_no, "withdraw confirmation repeat");
                return Ok(WithdrawConfirmOutcome { tx, should_notify });
            }

            // 写入【事实】：链上成功
            let rows = ApiWithdrawRepo::set_chain_success(pool, trade_no).await?;
            if rows > 0 {
                should_notify = true;
            }
            tracing::info!(trade_no=%trade_no, "设置链上成功事实");
        } else {
            if tx.status == ApiWithdrawStatus::Failure
                || tx.status == ApiWithdrawStatus::ConfirmFailureReport
            {
                tracing::warn!(trade_no=%trade_no, "withdraw confirmation repeat");
                return Ok(WithdrawConfirmOutcome { tx, should_notify });
            }

            // 写入【事实】：链上失败
            let rows = ApiWithdrawRepo::set_chain_failed(pool, trade_no).await?;
            if rows > 0 {
                should_notify = true;
            }
            tracing::info!(trade_no=%trade_no, "设置链上失败事实");

            if is_pre_broadcast_failure {
                let finished_rows = ApiWithdrawRepo::mark_chain_finished(pool, trade_no).await?;
                tracing::info!(
                    trade_no = %trade_no,
                    rows_affected = finished_rows,
                    "pre-broadcast withdraw failure finished locally"
                );
            }
        }

        // 返回最新事实快照
        let tx =
            ApiWithdrawRepo::get_api_withdraw_by_trade_no(pool, trade_no, ApiTradeType::Withdraw)
                .await?;

        Ok(WithdrawConfirmOutcome { tx, should_notify })
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiWithdrawDomain, is_row_not_found_db_error};
    use chrono::Utc;
    use tempfile::TempDir;
    use wallet_database::{
        ApiTransactionDbPool, SqliteContext,
        entities::{
            api_trade_type::ApiTradeType,
            api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus},
            asset_token_key::AssetTokenKey,
        },
        repositories::api_wallet::withdraw::ApiWithdrawRepo,
    };

    struct TestWithdrawDb {
        _dir: TempDir,
        pool: ApiTransactionDbPool,
    }

    impl TestWithdrawDb {
        async fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let ctx = SqliteContext::new(
                dir.path().to_string_lossy().as_ref(),
                Some("api_transaction.db"),
            )
            .await
            .expect("init api_transaction.db");
            let pool = ctx.into_transaction_db_pool().expect("transaction pool");
            Self { _dir: dir, pool }
        }

        async fn insert_withdraw(&self, trade_no: &str, status: ApiWithdrawStatus) {
            ApiWithdrawRepo::upsert_api_withdraw(
                &self.pool,
                "uid",
                "withdraw",
                "from",
                "to",
                "1",
                "validate",
                "tron",
                AssetTokenKey::Native,
                "TRX",
                trade_no,
                None,
                None,
                None,
                ApiTradeType::Withdraw,
                0,
                None,
                ApiWithdrawStatus::Init,
                status,
                "0",
                "0",
                None,
                None,
            )
            .await
            .expect("insert withdraw");
        }
    }

    fn make_entity(
        audit_passed_at: Option<chrono::DateTime<Utc>>,
        audit_rejected_at: Option<chrono::DateTime<Utc>>,
    ) -> ApiWithdrawEntity {
        let now = Utc::now();
        ApiWithdrawEntity {
            id: 0,
            name: "test".to_string(),
            uid: "test_uid".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "100".to_string(),
            validate: "validate".to_string(),
            chain_code: "TRX".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "USDT".to_string(),
            trade_no: "T2024000000000000001".to_string(),
            out_order_id: None,
            client_id: None,
            create_time: None,
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: None,
            raw_tx: None,
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            estimated_transaction_fee: None,
            estimated_resource_consume: None,
            fee_estimated_at: None,
            transaction_time: None,
            block_height: None,
            notes: None,
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: None,
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            tx_ack_sent_at: None,
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            audit_passed_at,
            audit_rejected_at,
            audit_reason: None,
            chain_success_at: None,
            chain_failed_at: None,
            failure_stage: None,
            created_at: now,
            updated_at: None,
        }
    }

    #[test]
    fn withdraw_row_not_found_guard() {
        let not_found = wallet_database::Error::Database(wallet_database::DatabaseError::Sqlx(
            sqlx::Error::RowNotFound,
        ));
        assert!(is_row_not_found_db_error(&not_found));

        let other = wallet_database::Error::Database(wallet_database::DatabaseError::QueryFailed);
        assert!(!is_row_not_found_db_error(&other));
    }

    #[test]
    fn withdraw_audit_report_gate_skips_existing_decisions() {
        assert!(ApiWithdrawDomain::has_no_audit_decision(&make_entity(None, None)));
        assert!(!ApiWithdrawDomain::has_no_audit_decision(&make_entity(Some(Utc::now()), None)));
        assert!(!ApiWithdrawDomain::has_no_audit_decision(&make_entity(None, Some(Utc::now()))));
        assert!(!ApiWithdrawDomain::has_no_audit_decision(&make_entity(
            Some(Utc::now()),
            Some(Utc::now())
        )));
    }

    #[tokio::test]
    async fn withdraw_confirm_success_writes_transaction_time_and_chain_success() {
        let db = TestWithdrawDb::new().await;
        let trade_no = "W_CONFIRM_SUCCESS_FACTS";
        db.insert_withdraw(trade_no, ApiWithdrawStatus::SendingTxReport).await;

        let outcome = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, trade_no, true)
            .await
            .expect("confirm withdraw success");

        assert!(outcome.should_notify, "new confirmation facts should notify once");
        assert!(outcome.tx.transaction_time.is_some(), "transaction_time fact must be written");
        assert!(outcome.tx.chain_success_at.is_some(), "chain_success_at fact must be written");
        assert!(outcome.tx.chain_failed_at.is_none(), "success must clear failure fact");
        assert_eq!(outcome.tx.status, ApiWithdrawStatus::Success);

        let saved = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &db.pool,
            trade_no,
            ApiTradeType::Withdraw,
        )
        .await
        .expect("reload withdraw");
        assert!(saved.transaction_time.is_some());
        assert!(saved.chain_success_at.is_some());
        assert!(saved.chain_failed_at.is_none());
        assert_eq!(saved.status, ApiWithdrawStatus::Success);
    }

    #[tokio::test]
    async fn withdraw_confirm_failure_writes_transaction_time_and_chain_failed() {
        let db = TestWithdrawDb::new().await;
        let trade_no = "W_CONFIRM_FAILURE_FACTS";
        db.insert_withdraw(trade_no, ApiWithdrawStatus::SendingTxReport).await;

        let outcome = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, trade_no, false)
            .await
            .expect("confirm withdraw failure");

        assert!(outcome.should_notify, "new confirmation facts should notify once");
        assert!(outcome.tx.transaction_time.is_some(), "transaction_time fact must be written");
        assert!(outcome.tx.chain_failed_at.is_some(), "chain_failed_at fact must be written");
        assert!(outcome.tx.chain_success_at.is_none(), "failure must clear success fact");
        assert_eq!(outcome.tx.status, ApiWithdrawStatus::Failure);

        let saved = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &db.pool,
            trade_no,
            ApiTradeType::Withdraw,
        )
        .await
        .expect("reload withdraw");
        assert!(saved.transaction_time.is_some());
        assert!(saved.chain_failed_at.is_some());
        assert!(saved.chain_success_at.is_none());
        assert_eq!(saved.status, ApiWithdrawStatus::Failure);
    }

    #[tokio::test]
    async fn withdraw_confirm_pre_broadcast_failure_finishes_without_receipt() {
        let db = TestWithdrawDb::new().await;
        let trade_no = "W_CONFIRM_PRE_BROADCAST_CANCEL";
        db.insert_withdraw(trade_no, ApiWithdrawStatus::SendingTx).await;

        sqlx::query(
            r#"
            UPDATE api_withdraws
            SET
                tx_ack_sent_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                raw_tx = '{"tx":true}',
                tx_hash = '0xpre_broadcast_cancel',
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(trade_no)
        .execute(db.pool.as_ref())
        .await
        .expect("seed built withdraw");

        let outcome = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, trade_no, false)
            .await
            .expect("confirm withdraw cancel");

        assert!(outcome.should_notify, "new cancel facts should notify once");

        let saved = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &db.pool,
            trade_no,
            ApiTradeType::Withdraw,
        )
        .await
        .expect("reload withdraw");
        assert!(saved.transaction_time.is_some(), "cancel result fact must be written");
        assert!(saved.chain_failed_at.is_some(), "cancel should be recorded as failure fact");
        assert!(saved.last_broadcast_at.is_none(), "precondition: SDK never broadcasted");
        assert!(saved.finished_at.is_some(), "pre-broadcast cancel should finish locally");
        assert_eq!(saved.status, ApiWithdrawStatus::Failure);

        let broadcast_rows =
            ApiWithdrawRepo::scan_can_broadcast(&db.pool, 10).await.expect("scan broadcast rows");
        assert!(
            !broadcast_rows.iter().any(|row| row.trade_no == trade_no),
            "cancelled withdraw must not be broadcast"
        );

        let receipt_rows = ApiWithdrawRepo::scan_need_tx_exec_receipt_upload(&db.pool, 10)
            .await
            .expect("scan receipt rows");
        assert!(
            !receipt_rows.iter().any(|row| row.trade_no == trade_no),
            "pre-broadcast cancel must not upload tx execution receipt"
        );
    }

    #[tokio::test]
    async fn withdraw_confirm_repeat_success_does_not_notify_again() {
        let db = TestWithdrawDb::new().await;
        let trade_no = "W_CONFIRM_REPEAT_SUCCESS";
        db.insert_withdraw(trade_no, ApiWithdrawStatus::Success).await;
        let existing_time = Utc::now();
        sqlx::query(
            r#"
            UPDATE api_withdraws
            SET transaction_time = ?,
                chain_success_at = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            WHERE trade_no = ?
            "#,
        )
        .bind(existing_time)
        .bind(existing_time)
        .bind(trade_no)
        .execute(db.pool.as_ref())
        .await
        .expect("seed existing success facts");

        let outcome = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, trade_no, true)
            .await
            .expect("repeat confirm withdraw success");

        assert!(!outcome.should_notify, "repeat confirmation must not notify again");
        assert_eq!(outcome.tx.status, ApiWithdrawStatus::Success);
        assert_eq!(outcome.tx.transaction_time, Some(existing_time));
        assert_eq!(outcome.tx.chain_success_at, Some(existing_time));
        assert!(outcome.tx.chain_failed_at.is_none());
    }

    #[tokio::test]
    async fn withdraw_confirm_missing_trade_no_errors() {
        let db = TestWithdrawDb::new().await;

        let res = ApiWithdrawDomain::confirm_tx_in_pool(&db.pool, "W_CONFIRM_MISSING", true).await;

        assert!(res.is_err(), "pool seam should surface missing withdraw rows");
    }
}
