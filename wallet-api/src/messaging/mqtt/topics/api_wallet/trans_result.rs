// messaging/mqtt/topics/api_wallet/trans_result.rs
use crate::{
    context::Context,
    domain::api_wallet::trans::{
        collect::ApiCollectDomain, fee::ApiFeeDomain, withdraw::ApiWithdrawDomain,
    },
    messaging::{
        mqtt::topics::api_wallet::result_fields::{
            AwmResultTxFee, deserialize_optional_non_empty_string,
        },
        notify::{FrontendNotifyEvent, event::NotifyEvent},
    },
};
use tracing;
use wallet_database::{
    entities::{
        api_resource_delegation::{ApiResourceDelegationResultStatus, NewApiResourceDelegation},
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, fee::ApiFeeRepo, resource_delegation::ApiResourceDelegationRepo,
        resource_operation::ApiResourceOperationRepo, wallet::ApiWalletRepo,
        withdraw::ApiWithdrawRepo,
    },
};
use wallet_transport_backend::request::api_wallet::msg::MsgAckReq;

// biz_type = AWM_ORDER_TRANS_RES
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwmOrderTransResMsg {
    trade_no: String,
    /// 交易类型： 1 提币 / 2 归集 / 3 归集手续费交易
    #[serde(
        deserialize_with = "wallet_utils::serde_func::string_to_u32",
        serialize_with = "wallet_utils::serde_func::u32_to_string"
    )]
    trade_type: u32,
    /// 交易结果： true 成功 /false 失败
    status: bool,
    /// 订单结失败型：0 默认，无意义 /  1 交易正常失败 / 2 手续费失败
    fail_type: Option<i32>,
    uid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tx_fee: Option<AwmResultTxFee>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_non_empty_string",
        skip_serializing_if = "Option::is_none"
    )]
    block_number: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct WithdrawActualFeeUpdate {
    transaction_fee: Option<String>,
    resource_consume: Option<String>,
    block_height: Option<String>,
}

// API钱包的订单结果消息
impl AwmOrderTransResMsg {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        tracing::info!(
            msg_id = %_msg_id,
            trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            status = %self.status,
            fail_type = ?self.fail_type,
            "Received AwmOrderTransResMsg"
        );

        tracing::info!(
            msg_id = %_msg_id,
            trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            phase = "pre_ack_local_process",
            "AwmOrderTransResMsg processing before backend msg ack"
        );
        if let Err(e) = self.check_uid(ctx).await {
            tracing::warn!(
                msg_id = %_msg_id,
                trade_no = %self.trade_no,
                trade_type = %self.trade_type,
                status = %self.status,
                fail_type = ?self.fail_type,
                error = %e,
                phase = "pre_ack_local_process",
                "AwmOrderTransResMsg check_uid failed (message will NOT be acked; waiting resend)"
            );
            return Err(e);
        }

        let backend = ctx.get_global_backend_api();
        let mut msg_ack_req = MsgAckReq::default();
        msg_ack_req.push(_msg_id);
        tracing::info!(
            msg_id = %_msg_id,
            trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            phase = "ack_backend_msg",
            "AwmOrderTransResMsg local processing done; acking backend message"
        );
        backend.msg_ack(msg_ack_req).await?;
        tracing::info!(
            msg_id = %_msg_id,
            trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            phase = "ack_backend_msg",
            "AwmOrderTransResMsg acked"
        );

        let data = NotifyEvent::AwmOrderTransRes(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub(crate) async fn exec_resource_result(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        tracing::info!(
            msg_id = %_msg_id,
            trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            status = %self.status,
            fail_type = ?self.fail_type,
            "Received AwmCmdRscResMsg"
        );

        if self.uid_exists(ctx).await? {
            let api_transaction_pool = ctx.api_transaction_pool()?;
            let api_resource_pool = ctx.api_resource_pool()?;
            self.resource_result_with_ctx(&api_transaction_pool, &api_resource_pool, Some(ctx))
                .await?;
        } else {
            tracing::warn!("AwmCmdRscResMsg uid not found: {}", self.uid);
        }

        let backend = ctx.get_global_backend_api();
        let mut msg_ack_req = MsgAckReq::default();
        msg_ack_req.push(_msg_id);
        backend.msg_ack(msg_ack_req).await?;

        let data = NotifyEvent::AwmOrderTransRes(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    pub(crate) async fn check_uid(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        // tracing::info!("临时这样做");
        // return Ok(());

        if !self.uid_exists(ctx).await? {
            tracing::warn!("AwmOrderTransResMsg uid not found: {}", self.uid);
            return Ok(());
        }

        // ✅ 强顺序屏障：先持久化“已收到 SER TxRes”事实，再进入 confirm_tx 路径
        // ⚠️ 若此处失败，必须返回错误并禁止 ack MQTT（让其重投）
        let api_transaction_pool = ctx.api_transaction_pool()?;
        let api_resource_pool = ctx.api_resource_pool()?;
        match self.trade_type {
            1 => {
                ApiWithdrawRepo::update_tx_res_received_at(&api_transaction_pool, &self.trade_no)
                    .await?;
                self.persist_withdraw_actual_fee(&api_transaction_pool).await?;
                self.withdraw(ctx).await?;
            }
            2 => {
                ApiCollectRepo::update_tx_res_received_at(&api_transaction_pool, &self.trade_no)
                    .await?;
                let fail_type = self.fail_type.unwrap_or(0);
                self.collect(ctx, fail_type).await?;
            }
            3 => {
                ApiFeeRepo::update_tx_res_received_at(&api_transaction_pool, &self.trade_no)
                    .await?;
                self.transfer_fee(ctx).await?;
            }
            4 => {
                self.resource_operation_result_with_ctx(&api_resource_pool, Some(ctx)).await?;
            }
            5 => {
                self.collect_resource_delegation_result_with_ctx(
                    &api_transaction_pool,
                    &api_resource_pool,
                    Some(ctx),
                )
                .await?;
            }
            7 => {
                self.withdraw_resource_delegation_result_with_ctx(
                    &api_transaction_pool,
                    &api_resource_pool,
                    Some(ctx),
                )
                .await?;
            }
            6 | 8 => {
                self.resource_reclaim_result(&api_resource_pool).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn uid_exists(
        &self,
        ctx: &'static Context,
    ) -> Result<bool, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let res = ApiWalletRepo::find_by_uid(&pool, &self.uid).await?;
        Ok(res.is_some())
    }

    async fn resource_result_with_ctx(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        // AWM_CMD_RSC_RES is shared by two local roles. Platform wallets receive
        // real resource task results (CD/CR resource trade numbers), while
        // merchant wallets receive resource results projected onto the original
        // collect/withdraw order number. The handlers below resolve the local
        // fact first instead of trusting only the message trade type.
        match self.trade_type {
            1 | 7 => {
                self.withdraw_resource_delegation_result_with_ctx(
                    api_transaction_pool,
                    api_resource_pool,
                    ctx,
                )
                .await
            }
            2 | 5 => {
                self.collect_resource_delegation_result_with_ctx(
                    api_transaction_pool,
                    api_resource_pool,
                    ctx,
                )
                .await
            }
            4 => self.resource_operation_result_with_ctx(api_resource_pool, ctx).await,
            6 | 8 => self.resource_reclaim_result(api_resource_pool).await,
            _ => {
                tracing::warn!(
                    trade_no = %self.trade_no,
                    trade_type = %self.trade_type,
                    "Unsupported AWM_CMD_RSC_RES trade type"
                );
                Ok(())
            }
        }
    }

    async fn resource_operation_result_with_ctx(
        &self,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let result_status = if self.status { "success" } else { "fail" };
        let result_payload = wallet_utils::serde_func::serde_to_string(self).ok();
        ApiResourceOperationRepo::mark_result_received(
            api_resource_pool,
            &self.trade_no,
            result_status,
            self.fail_type.map(i64::from),
            None,
            None,
            result_payload.as_deref(),
        )
        .await?;

        self.trigger_resource_operation_shadow(ctx).await;

        Ok(())
    }

    async fn collect_resource_delegation_result_with_ctx(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let result_status = if self.status {
            ApiResourceDelegationResultStatus::Success
        } else {
            ApiResourceDelegationResultStatus::Fail
        };
        let result_payload = wallet_utils::serde_func::serde_to_string(self).ok();

        if let Some(resource_task) =
            ApiResourceDelegationRepo::find_by_resource_trade_no(api_resource_pool, &self.trade_no)
                .await?
        {
            // Platform wallet: the message trade number is the real resource
            // delegation task (for example CD...), so this fact is ACKed later
            // as a resource task result with TX_RES.
            ApiResourceDelegationRepo::mark_backend_result_received(
                api_resource_pool,
                &self.trade_no,
                result_status,
                self.fail_type.map(i64::from),
                None,
                None,
                result_payload.as_deref(),
            )
            .await?;

            if let Some(origin_trade_no) = resource_task.origin_trade_no.as_deref() {
                self.trigger_collect_shadow(ctx, origin_trade_no).await;
            }

            return Ok(());
        }

        let collect = ApiCollectRepo::find_by_resource_dependency_trade_no(
            api_transaction_pool,
            &self.trade_no,
        )
        .await?
        .or(ApiCollectRepo::find_api_collect_by_trade_no(
            api_transaction_pool,
            &self.trade_no,
        )
        .await?);

        let Some(collect) = collect else {
            tracing::warn!(
                resource_trade_no = %self.trade_no,
                "Collect resource delegation result has no local resource task or blocked origin order"
            );
            return Ok(());
        };

        // Merchant wallet: the message trade number is the original collect
        // order, or a dependency that points back to it. Persist a projection
        // fact so the side-effect worker can ACK it as COL + TX_RSC_RES.
        if self.status {
            let rows = ApiCollectRepo::mark_resource_released(
                api_transaction_pool,
                &collect.trade_no,
                ApiResourceGateResult::PlatformDelegateSuccess,
            )
            .await?;
            tracing::info!(
                trade_no = %collect.trade_no,
                resource_trade_no = %self.trade_no,
                rows,
                status = self.status,
                fail_type = ?self.fail_type,
                "Collect platform resource result released resource gate"
            );
        } else {
            // 平台代理失败不是归集资源链的最终出口。
            // 文档顺序是：自身资源 -> 平台代理 -> 本地代理 -> 主链/手续费。
            // 所以商户侧收到原单失败结果时，只把 collect 切回本地代理入口；
            // 只有本地代理也失败后，才由 local delegation 收口释放 gate。
            let rows = ApiCollectRepo::mark_resource_blocked(
                api_transaction_pool,
                &collect.trade_no,
                ApiResourceBlockReason::NeedLocalDelegate,
                None,
                Some(ApiResourceDependencyType::LocalDelegate),
            )
            .await?;
            tracing::warn!(
                trade_no = %collect.trade_no,
                resource_trade_no = %self.trade_no,
                rows,
                status = self.status,
                fail_type = ?self.fail_type,
                "Collect platform resource result fell back to local delegation"
            );
        }

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            api_resource_pool,
            NewApiResourceDelegation::platform_delegate(
                &self.uid,
                &self.trade_no,
                &collect.trade_no,
                ApiTradeType::Collect as i64,
                "",
                "",
                "0",
            ),
            result_status,
            self.fail_type.map(i64::from),
            result_payload.as_deref(),
        )
        .await?;

        tracing::info!(
            trade_no = %collect.trade_no,
            resource_trade_no = %self.trade_no,
            "Collect resource result persisted; shadow will ACK resource result before advancing"
        );
        self.trigger_collect_shadow(ctx, &collect.trade_no).await;

        Ok(())
    }

    async fn withdraw_resource_delegation_result_with_ctx(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        tracing::info!(
            resource_trade_no = %self.trade_no,
            trade_type = %self.trade_type,
            status = %self.status,
            fail_type = ?self.fail_type,
            "Received withdraw resource delegation result"
        );
        let result_status = if self.status {
            ApiResourceDelegationResultStatus::Success
        } else {
            ApiResourceDelegationResultStatus::Fail
        };
        let result_payload = wallet_utils::serde_func::serde_to_string(self).ok();

        if let Some(resource_task) =
            ApiResourceDelegationRepo::find_by_resource_trade_no(api_resource_pool, &self.trade_no)
                .await?
        {
            // Platform wallet: the message trade number is the real resource
            // delegation task (for example CD...), so this fact is ACKed later
            // as a resource task result with TX_RES.
            tracing::info!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = ?resource_task.origin_trade_no,
                "Withdraw resource delegation result matched local resource task"
            );
            ApiResourceDelegationRepo::mark_backend_result_received(
                api_resource_pool,
                &self.trade_no,
                result_status,
                self.fail_type.map(i64::from),
                None,
                None,
                result_payload.as_deref(),
            )
            .await?;

            if let Some(origin_trade_no) = resource_task.origin_trade_no.as_deref() {
                self.trigger_withdraw_shadow(ctx, origin_trade_no).await;
            }

            return Ok(());
        }

        let withdraw = ApiWithdrawRepo::find_by_resource_dependency_trade_no(
            api_transaction_pool,
            &self.trade_no,
        )
        .await?
        .or(ApiWithdrawRepo::find_api_withdraw_by_trade_no(
            api_transaction_pool,
            &self.trade_no,
            ApiTradeType::Withdraw,
        )
        .await?);

        let Some(withdraw) = withdraw else {
            tracing::warn!(
                resource_trade_no = %self.trade_no,
                "Withdraw resource delegation result has no local resource task or blocked origin order"
            );
            return Ok(());
        };

        // Merchant wallet: the message trade number is the original withdraw
        // order, or a dependency that points back to it. Persist a projection
        // fact so the side-effect worker can ACK it as WD + TX_RSC_RES.
        tracing::info!(
            resource_trade_no = %self.trade_no,
            origin_trade_no = %withdraw.trade_no,
            dependency_trade_no = ?withdraw.resource_dependency_trade_no,
            status = %self.status,
            "Withdraw resource delegation result matched blocked origin order"
        );
        let gate_result = if self.status {
            ApiResourceGateResult::PlatformDelegateSuccess
        } else {
            ApiResourceGateResult::ResourceDelegationFailedBypass
        };
        let rows = ApiWithdrawRepo::mark_platform_delegation_result_received(
            api_transaction_pool,
            &withdraw.trade_no,
            gate_result,
        )
        .await?;
        tracing::info!(
            resource_trade_no = %self.trade_no,
            origin_trade_no = %withdraw.trade_no,
            gate_result = ?gate_result,
            rows,
            "Withdraw platform delegation result persisted; gate remains blocked until ACK"
        );

        ApiResourceDelegationRepo::upsert_original_order_result_fact(
            api_resource_pool,
            NewApiResourceDelegation::platform_delegate(
                &self.uid,
                &self.trade_no,
                &withdraw.trade_no,
                ApiTradeType::Withdraw as i64,
                "",
                "",
                "0",
            ),
            result_status,
            self.fail_type.map(i64::from),
            result_payload.as_deref(),
        )
        .await?;

        self.trigger_withdraw_shadow(ctx, &withdraw.trade_no).await;

        Ok(())
    }

    async fn trigger_collect_shadow(&self, ctx: Option<&'static Context>, origin_trade_no: &str) {
        let Some(ctx) = ctx else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Skip collect shadow trigger: no context passed"
            );
            return;
        };
        let Some(handles) = optional_handles(ctx).await else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Skip collect shadow trigger: global handles are not available"
            );
            return;
        };
        let collect_handle = handles.get_global_processed_collect_tx_handle();
        let Some(shadow_system) = collect_handle.get_shadow_system() else {
            return;
        };
        if let Err(e) = shadow_system.trigger_collect(origin_trade_no).await {
            tracing::warn!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Trigger collect shadow failed after resource result, but continuing: {:?}",
                e
            );
        }
    }

    async fn trigger_withdraw_shadow(&self, ctx: Option<&'static Context>, origin_trade_no: &str) {
        let Some(ctx) = ctx else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Skip withdraw shadow trigger: no context passed"
            );
            return;
        };
        let Some(handles) = optional_handles(ctx).await else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Skip withdraw shadow trigger: global handles are not available"
            );
            return;
        };
        let withdraw_handle = handles.get_global_processed_withdraw_tx_handle();
        let Some(shadow_system) = withdraw_handle.get_shadow_system() else {
            return;
        };
        if let Err(e) = shadow_system.trigger_withdraw(origin_trade_no).await {
            tracing::warn!(
                resource_trade_no = %self.trade_no,
                origin_trade_no = %origin_trade_no,
                "Trigger withdraw shadow failed after resource result, but continuing: {:?}",
                e
            );
        }
    }

    async fn trigger_resource_operation_shadow(&self, ctx: Option<&'static Context>) {
        let Some(ctx) = ctx else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                "Skip resource operation shadow trigger: no context passed"
            );
            return;
        };
        let Some(handles) = optional_handles(ctx).await else {
            tracing::debug!(
                resource_trade_no = %self.trade_no,
                "Skip resource operation shadow trigger: global handles are not available"
            );
            return;
        };
        if let Err(e) = handles.trigger_resource_operation(&self.trade_no).await {
            tracing::warn!(
                resource_trade_no = %self.trade_no,
                "Trigger resource operation shadow failed after resource operation result, but continuing: {:?}",
                e
            );
        }
    }

    async fn resource_reclaim_result(
        &self,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let result_status = if self.status {
            ApiResourceDelegationResultStatus::Success
        } else {
            ApiResourceDelegationResultStatus::Fail
        };
        let result_payload = wallet_utils::serde_func::serde_to_string(self).ok();
        ApiResourceDelegationRepo::mark_backend_result_received(
            api_resource_pool,
            &self.trade_no,
            result_status,
            self.fail_type.map(i64::from),
            None,
            None,
            result_payload.as_deref(),
        )
        .await?;

        tracing::info!(
            resource_trade_no = %self.trade_no,
            status = %self.status,
            "Resource reclaim result received"
        );

        Ok(())
    }

    pub(crate) async fn transfer_fee(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiFeeDomain::confirm_tx(ctx, &self.trade_no, self.status).await.map_err(|e| {
            tracing::warn!(
                trade_no = %self.trade_no,
                trade_type = %self.trade_type,
                status = %self.status,
                error = %e,
                "ApiFeeDomain::confirm_tx failed for AwmOrderTransResMsg"
            );
            e
        })
    }

    pub(crate) async fn collect(
        &self,
        ctx: &'static Context,
        fail_type: i32,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiCollectDomain::confirm_tx(ctx, &self.trade_no, self.status, fail_type).await.map_err(
            |e| {
                tracing::warn!(
                    trade_no = %self.trade_no,
                    trade_type = %self.trade_type,
                    status = %self.status,
                    fail_type = %fail_type,
                    error = %e,
                    "ApiCollectDomain::confirm_tx failed for AwmOrderTransResMsg"
                );
                e
            },
        )
    }

    pub(crate) async fn withdraw(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiWithdrawDomain::confirm_tx(ctx, &self.trade_no, self.status).await.map_err(|e| {
            tracing::warn!(
                trade_no = %self.trade_no,
                trade_type = %self.trade_type,
                status = %self.status,
                fail_type = ?self.fail_type,
                error = %e,
                "ApiWithdrawDomain::confirm_tx failed for AwmOrderTransResMsg"
            );
            e
        })
    }

    fn withdraw_actual_fee_update(&self) -> Option<WithdrawActualFeeUpdate> {
        let transaction_fee = self.tx_fee.as_ref().and_then(|tx_fee| tx_fee.native_fee.clone());

        let resource_consume = self.tx_fee.as_ref().and_then(|tx_fee| {
            let mut resource = serde_json::Map::new();
            if let Some(bandwidth) = tx_fee.bandwidth {
                resource.insert("bandwidth".to_string(), serde_json::Value::from(bandwidth));
            }
            if let Some(energy) = tx_fee.energy {
                resource.insert("energy".to_string(), serde_json::Value::from(energy));
            }
            if resource.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(resource).to_string())
            }
        });

        let block_height = self.block_number.clone();

        if transaction_fee.is_none() && resource_consume.is_none() && block_height.is_none() {
            None
        } else {
            Some(WithdrawActualFeeUpdate { transaction_fee, resource_consume, block_height })
        }
    }

    async fn persist_withdraw_actual_fee(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let Some(update) = self.withdraw_actual_fee_update() else {
            return Ok(());
        };

        let rows = ApiWithdrawRepo::update_actual_fee(
            api_transaction_pool,
            &self.trade_no,
            update.transaction_fee.as_deref(),
            update.resource_consume.as_deref(),
            update.block_height.as_deref(),
        )
        .await?;

        tracing::info!(
            trade_no = %self.trade_no,
            rows,
            has_transaction_fee = update.transaction_fee.is_some(),
            has_resource_consume = update.resource_consume.is_some(),
            has_block_height = update.block_height.is_some(),
            "Persisted withdraw actual fee fields from AWM_ORDER_TRANS_RES"
        );

        Ok(())
    }

    #[cfg(test)]
    async fn resource_operation_result(
        &self,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.resource_operation_result_with_ctx(api_resource_pool, ctx).await
    }

    #[cfg(test)]
    async fn collect_resource_delegation_result(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.collect_resource_delegation_result_with_ctx(
            api_transaction_pool,
            api_resource_pool,
            ctx,
        )
        .await
    }

    #[cfg(test)]
    async fn withdraw_resource_delegation_result(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.withdraw_resource_delegation_result_with_ctx(
            api_transaction_pool,
            api_resource_pool,
            ctx,
        )
        .await
    }

    #[cfg(test)]
    async fn resource_result(
        &self,
        api_transaction_pool: &wallet_database::ApiTransactionDbPool,
        api_resource_pool: &wallet_database::ApiResourceDbPool,
        ctx: Option<&'static Context>,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.resource_result_with_ctx(api_transaction_pool, api_resource_pool, ctx).await
    }
}

async fn optional_handles(
    ctx: &'static Context,
) -> Option<std::sync::Arc<crate::handles::Handles>> {
    ctx.get_global_handles().await.upgrade()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wallet_database::{
        SqliteContext,
        entities::{
            api_collect::ApiCollectStatus, api_resource_operation::NewApiResourceOperation,
        },
        repositories::api_wallet::resource_operation::ApiResourceOperationRepo,
    };

    #[test]
    fn mqtt_result_mock_data_parses_order_trans_actual_fee_fields() -> anyhow::Result<()> {
        let msg: AwmOrderTransResMsg = serde_json::from_value(serde_json::json!({
            "tradeNo": "W_mqtt_mock",
            "tradeType": "1",
            "status": true,
            "failType": 0,
            "uid": "uid_1",
            "txFee": {
                "nativeFee": "1.23",
                "bandwidth": 345,
                "energy": "678"
            },
            "blockNumber": 12345678
        }))?;

        assert_eq!(msg.trade_no, "W_mqtt_mock");
        assert_eq!(msg.trade_type, 1);
        assert!(msg.status);
        assert_eq!(msg.fail_type, Some(0));
        assert_eq!(msg.uid, "uid_1");
        assert_eq!(msg.block_number.as_deref(), Some("12345678"));

        let tx_fee = msg.tx_fee.as_ref().expect("txFee should parse");
        assert_eq!(tx_fee.native_fee.as_deref(), Some("1.23"));
        assert_eq!(tx_fee.bandwidth, Some(345));
        assert_eq!(tx_fee.energy, Some(678));

        let update = msg.withdraw_actual_fee_update().expect("actual fee update should be created");
        assert_eq!(update.transaction_fee.as_deref(), Some("1.23"));
        assert_eq!(update.resource_consume.as_deref(), Some(r#"{"bandwidth":345,"energy":678}"#));
        assert_eq!(update.block_height.as_deref(), Some("12345678"));

        let serialized = serde_json::to_value(&msg)?;
        assert_eq!(serialized["txFee"]["nativeFee"], "1.23");
        assert_eq!(serialized["txFee"]["bandwidth"], 345);
        assert_eq!(serialized["txFee"]["energy"], 678);
        assert_eq!(serialized["blockNumber"], "12345678");
        Ok(())
    }

    #[test]
    fn mqtt_result_mock_data_parses_resource_actual_fee_fields() -> anyhow::Result<()> {
        let msg: AwmOrderTransResMsg = serde_json::from_value(serde_json::json!({
            "tradeNo": "DL_mqtt_mock",
            "tradeType": "5",
            "status": true,
            "failType": null,
            "uid": "uid_1",
            "txFee": {
                "nativeFee": 0,
                "bandwidth": "0",
                "energy": 1200
            },
            "blockNumber": "12345679"
        }))?;

        assert_eq!(msg.trade_no, "DL_mqtt_mock");
        assert_eq!(msg.trade_type, 5);
        assert!(msg.status);
        assert_eq!(msg.fail_type, None);
        assert_eq!(msg.uid, "uid_1");
        assert_eq!(msg.block_number.as_deref(), Some("12345679"));

        let tx_fee = msg.tx_fee.as_ref().expect("txFee should parse");
        assert_eq!(tx_fee.native_fee.as_deref(), Some("0"));
        assert_eq!(tx_fee.bandwidth, Some(0));
        assert_eq!(tx_fee.energy, Some(1200));

        let serialized = serde_json::to_value(&msg)?;
        assert_eq!(serialized["txFee"]["nativeFee"], "0");
        assert_eq!(serialized["txFee"]["bandwidth"], 0);
        assert_eq!(serialized["txFee"]["energy"], 1200);
        assert_eq!(serialized["blockNumber"], "12345679");
        Ok(())
    }

    #[tokio::test]
    async fn resource_operation_result_persists_trade_type_4_result_fact() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiResourceOperationRepo::upsert(
            &resource_pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_result_msg", "owner", "1"),
        )
        .await?;

        let msg = AwmOrderTransResMsg {
            trade_no: "op_result_msg".to_string(),
            trade_type: 4,
            status: false,
            fail_type: Some(2),
            uid: "uid_1".to_string(),
            tx_fee: None,
            block_number: None,
        };

        msg.resource_operation_result(&resource_pool, None).await?;

        let got =
            ApiResourceOperationRepo::get_by_resource_trade_no(&resource_pool, "op_result_msg")
                .await?;
        assert_eq!(got.result_status.as_deref(), Some("fail"));
        assert_eq!(got.fail_type, Some(2));
        assert!(got.result_received_at.is_some());
        assert!(got.result_payload.as_deref().unwrap_or_default().contains("op_result_msg"));
        assert!(got.result_ack_sent_at.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn withdraw_result_actual_fee_persists_backend_fields() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            "W_actual_fee",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            wallet_database::entities::api_withdraw::ApiWithdrawStatus::Init,
            wallet_database::entities::api_withdraw::ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;

        let msg: AwmOrderTransResMsg = serde_json::from_value(serde_json::json!({
            "tradeNo": "W_actual_fee",
            "tradeType": "1",
            "status": true,
            "failType": 0,
            "uid": "uid_1",
            "txFee": {
                "nativeFee": "1.23",
                "bandwidth": 345,
                "energy": "678"
            },
            "blockNumber": 12345678
        }))?;

        msg.persist_withdraw_actual_fee(&pool).await?;

        let withdraw = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_actual_fee",
            ApiTradeType::Withdraw,
        )
        .await?;
        assert_eq!(withdraw.transaction_fee, "1.23");
        assert_eq!(withdraw.resource_consume, r#"{"bandwidth":345,"energy":678}"#);
        assert_eq!(withdraw.block_height.as_deref(), Some("12345678"));

        Ok(())
    }

    #[test]
    fn withdraw_result_actual_fee_missing_fields_keeps_fallback_empty() -> anyhow::Result<()> {
        let msg: AwmOrderTransResMsg = serde_json::from_value(serde_json::json!({
            "tradeNo": "W_no_fee",
            "tradeType": "1",
            "status": true,
            "failType": null,
            "uid": "uid_1"
        }))?;

        assert_eq!(msg.withdraw_actual_fee_update(), None);
        Ok(())
    }

    async fn insert_blocked_collect_waiting_platform_result(
        pool: &wallet_database::ApiTransactionDbPool,
        trade_no: &str,
        resource_trade_no: &str,
    ) -> anyhow::Result<()> {
        ApiCollectRepo::upsert_api_collect(
            pool,
            "uid_1",
            "collect",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            trade_no,
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        ApiCollectRepo::mark_resource_blocked(
            pool,
            trade_no,
            ApiResourceBlockReason::NeedPlatformDelegate,
            Some(resource_trade_no),
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn collect_resource_result_releases_origin_without_placeholder_task() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;
        insert_blocked_collect_waiting_platform_result(&pool, "C_wait_success", "DL_success")
            .await?;

        let msg = AwmOrderTransResMsg {
            trade_no: "DL_success".to_string(),
            trade_type: 5,
            status: true,
            fail_type: None,
            uid: "uid_1".to_string(),
            tx_fee: None,
            block_number: None,
        };

        msg.collect_resource_delegation_result(&pool, &resource_pool, None).await?;

        let collect = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "C_wait_success").await?;
        assert!(collect.resource_gate_released_at.is_some());
        assert_eq!(
            collect.resource_gate_result,
            Some(ApiResourceGateResult::PlatformDelegateSuccess)
        );
        assert_eq!(collect.resource_dependency_trade_no.as_deref(), Some("DL_success"));
        let result_fact =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, "DL_success")
                .await?;
        assert_eq!(result_fact.origin_trade_no.as_deref(), Some("C_wait_success"));
        assert_eq!(result_fact.result_status, Some(ApiResourceDelegationResultStatus::Success));
        assert!(result_fact.result_ack_sent_at.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn collect_resource_result_failure_switches_to_local_fallback() -> anyhow::Result<()> {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;
        insert_blocked_collect_waiting_platform_result(&pool, "C_wait_fail", "DL_fail").await?;

        let msg = AwmOrderTransResMsg {
            trade_no: "DL_fail".to_string(),
            trade_type: 5,
            status: false,
            fail_type: Some(1),
            uid: "uid_1".to_string(),
            tx_fee: None,
            block_number: None,
        };

        msg.collect_resource_delegation_result(&pool, &resource_pool, None).await?;

        let collect = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "C_wait_fail").await?;
        assert_eq!(collect.resource_block_reason, Some(ApiResourceBlockReason::NeedLocalDelegate));
        assert!(collect.resource_gate_released_at.is_none());
        assert!(collect.resource_gate_result.is_none());
        assert!(collect.resource_dependency_trade_no.is_none());
        assert_eq!(
            collect.resource_dependency_type,
            Some(ApiResourceDependencyType::LocalDelegate)
        );
        let result_fact =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, "DL_fail").await?;
        assert_eq!(result_fact.origin_trade_no.as_deref(), Some("C_wait_fail"));
        assert_eq!(result_fact.result_status, Some(ApiResourceDelegationResultStatus::Fail));
        assert!(result_fact.result_ack_sent_at.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn awm_cmd_rsc_res_trade_type_2_does_not_write_collect_tx_result_facts()
    -> anyhow::Result<()> {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiCollectRepo::upsert_api_collect(
            &pool,
            "uid_1",
            "collect",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            "C_origin_result",
            2,
            ApiCollectStatus::Init,
            1,
        )
        .await?;
        ApiCollectRepo::mark_resource_blocked(
            &pool,
            "C_origin_result",
            ApiResourceBlockReason::NeedPlatformDelegate,
            None,
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await?;

        let msg = AwmOrderTransResMsg {
            trade_no: "C_origin_result".to_string(),
            trade_type: 2,
            status: false,
            fail_type: Some(3),
            uid: "uid_1".to_string(),
            tx_fee: None,
            block_number: None,
        };

        msg.resource_result(&pool, &resource_pool, None).await?;

        let collect = ApiCollectRepo::get_api_collect_by_trade_no(&pool, "C_origin_result").await?;
        assert_eq!(collect.resource_block_reason, Some(ApiResourceBlockReason::NeedLocalDelegate));
        assert!(collect.resource_gate_released_at.is_none());
        assert!(collect.resource_gate_result.is_none());
        assert!(collect.resource_dependency_trade_no.is_none());
        assert!(collect.tx_res_received_at.is_none());
        assert!(collect.transaction_time.is_none());
        assert!(collect.finished_at.is_none());
        assert!(collect.result_ack_sent_at.is_none());

        let result_fact =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, "C_origin_result")
                .await?;
        assert_eq!(result_fact.origin_trade_no.as_deref(), Some("C_origin_result"));
        assert_eq!(result_fact.result_status, Some(ApiResourceDelegationResultStatus::Fail));
        assert!(result_fact.result_ack_sent_at.is_none());
        assert!(
            result_fact
                .result_payload
                .as_deref()
                .unwrap_or_default()
                .contains("\"tradeNo\":\"C_origin_result\"")
        );

        let result_ack_rows =
            ApiResourceDelegationRepo::scan_need_result_ack(&resource_pool, 100).await?;
        assert!(result_ack_rows.iter().any(|row| row.resource_trade_no == "C_origin_result"));

        Ok(())
    }

    #[tokio::test]
    async fn awm_cmd_rsc_res_trade_type_1_creates_withdraw_resource_ack_fact() -> anyhow::Result<()>
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_root = dir.path().to_string_lossy().to_string();
        let pool = SqliteContext::new(&db_root, Some("api_transaction.db"))
            .await?
            .into_transaction_db_pool()?;
        let resource_pool = SqliteContext::new(&db_root, Some("api_resource.db"))
            .await?
            .into_api_resource_db_pool()?;

        ApiWithdrawRepo::upsert_api_withdraw(
            &pool,
            "uid_1",
            "withdraw",
            "from_addr",
            "to_addr",
            "1.12",
            "digest",
            "tron",
            None,
            "TRX",
            "W_origin_result",
            None,
            None,
            None,
            ApiTradeType::Withdraw,
            0,
            None,
            wallet_database::entities::api_withdraw::ApiWithdrawStatus::Init,
            wallet_database::entities::api_withdraw::ApiWithdrawStatus::Init,
            "0",
            "0",
            None,
            None,
        )
        .await?;
        ApiWithdrawRepo::mark_resource_blocked(
            &pool,
            "W_origin_result",
            ApiResourceBlockReason::NeedPlatformDelegate,
            None,
            Some(ApiResourceDependencyType::PlatformDelegate),
        )
        .await?;

        let msg = AwmOrderTransResMsg {
            trade_no: "W_origin_result".to_string(),
            trade_type: 1,
            status: true,
            fail_type: None,
            uid: "uid_1".to_string(),
            tx_fee: None,
            block_number: None,
        };

        msg.resource_result(&pool, &resource_pool, None).await?;

        let withdraw = ApiWithdrawRepo::get_api_withdraw_by_trade_no(
            &pool,
            "W_origin_result",
            ApiTradeType::Withdraw,
        )
        .await?;
        assert!(
            withdraw.resource_gate_released_at.is_none(),
            "the result fact must not release the origin gate before its ACK is sent"
        );
        assert_eq!(
            withdraw.resource_gate_result,
            Some(ApiResourceGateResult::PlatformDelegateSuccess)
        );
        assert!(withdraw.tx_res_received_at.is_none());
        assert!(withdraw.transaction_time.is_none());
        assert!(withdraw.finished_at.is_none());
        assert!(withdraw.tx_res_ack_sent_at.is_none());

        let result_fact =
            ApiResourceDelegationRepo::get_by_resource_trade_no(&resource_pool, "W_origin_result")
                .await?;
        assert_eq!(result_fact.origin_trade_no.as_deref(), Some("W_origin_result"));
        assert_eq!(result_fact.origin_trade_type, Some(ApiTradeType::Withdraw as i64));
        assert_eq!(result_fact.result_status, Some(ApiResourceDelegationResultStatus::Success));
        assert!(result_fact.result_ack_sent_at.is_none());

        let collect_ack_rows = ApiResourceDelegationRepo::scan_need_result_ack_for_origin_type(
            &resource_pool,
            ApiTradeType::Collect as i64,
            100,
        )
        .await?;
        assert!(!collect_ack_rows.iter().any(|row| row.resource_trade_no == "W_origin_result"));

        let withdraw_ack_rows = ApiResourceDelegationRepo::scan_need_result_ack_for_origin_type(
            &resource_pool,
            ApiTradeType::Withdraw as i64,
            100,
        )
        .await?;
        assert!(withdraw_ack_rows.iter().any(|row| row.resource_trade_no == "W_origin_result"));

        Ok(())
    }
}
