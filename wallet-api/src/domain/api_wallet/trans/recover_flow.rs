use super::{
    ApiTransDomain,
    recover_mechanism::{
        RecoverAdapterErrorDecision, RecoverAuthRetryState, RecoverMissingTxRoute,
        RecoverQueryErrorDecision, RecoverQueryOutcome, classify_recover_missing_tx_route,
        log_evm_missing_tx_hash_state, recover_query_outcome,
    },
    shared_mechanism,
};
use crate::{
    context::Context,
    domain::{
        api_wallet::{adapter::tx::Tx, adapter_factory::ApiChainAdapterFactory},
        chain::TransferResp,
    },
    error::service::ServiceError,
    infrastructure::{
        api_trans::{
            recover_rpc_observability::{
                RECOVER_OP_GET_ADAPTER, RECOVER_OP_QUERY_TX_RES, log_recover_query_error_with_ctx,
                log_sol_visibility_probe_error_with_ctx,
                log_tron_pending_pool_query_error_with_ctx, record_recover_rpc_success,
            },
            rpc_auth_retry_mechanism,
        },
        chain_rpc_guard,
    },
};
use std::sync::Arc;
use wallet_chain_interact::BillResourceConsume;
use wallet_utils::RetryableError as _;

// Unlike the legacy optional result, workers must distinguish a missing receipt
// from an observation failure before running timeout/nonce recovery policies.
pub(crate) enum RecoveredTxObservation {
    Confirmed(TransferResp),
    Missing,
    QueryUncertain,
}

enum RecoverAdapterAcquire {
    Ready(Arc<dyn Tx + Send + Sync>),
    RetryAuth,
    StayUncertain,
}

enum RecoverQueryErrorAction {
    RetryAuth,
    StayUncertain,
}

impl ApiTransDomain {
    /// 处理已生成raw_tx的交易恢复逻辑
    ///
    /// 职责：
    /// - 只负责从链上恢复交易状态
    /// - 不负责构建交易
    /// - 使用链上时间设置 transaction_time_ms
    ///
    /// 显式不变量：
    /// - transaction_time MUST come from on-chain confirmation (chain timestamp)
    /// - last_broadcast_at MUST be backfilled with the same value as transaction_time
    /// - This ensures both fields reflect the same chain-based timestamp
    ///
    /// 返回语义约定：
    /// Ok(Some(resp))  -> 链上已确认成功，立即落成
    /// Err(_)          -> 调用方须区分明确链上失败与查询准备错误
    /// Ok(None)        -> 不确定态（RPC/网络/索引问题），等待 scanner/recover
    pub async fn process_recovered_tx(
        ctx: &'static Context,
        chain_code: &str,
        from_addr: &str,
        tx_hash: &str,
        // raw_tx: &str,
        nonce: i64,
        transaction_fee: &str,
    ) -> Result<Option<TransferResp>, ServiceError> {
        Self::process_recovered_tx_with_ctx(
            ctx,
            chain_code,
            from_addr,
            tx_hash,
            nonce,
            transaction_fee,
        )
        .await
    }

    pub async fn process_recovered_tx_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        from_addr: &str,
        tx_hash: &str,
        nonce: i64,
        transaction_fee: &str,
    ) -> Result<Option<TransferResp>, ServiceError> {
        // Preserve the public optional "no definitive result" contract for resource
        // callers. Collect/withdraw/fee workers consume the richer observation below.
        Ok(
            match Self::observe_recovered_tx_with_ctx(
                ctx,
                chain_code,
                from_addr,
                tx_hash,
                nonce,
                transaction_fee,
            )
            .await?
            {
                RecoveredTxObservation::Confirmed(resp) => Some(resp),
                RecoveredTxObservation::Missing | RecoveredTxObservation::QueryUncertain => None,
            },
        )
    }

    pub(crate) async fn observe_recovered_tx_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        from_addr: &str,
        tx_hash: &str,
        // raw_tx: &str,
        nonce: i64,
        transaction_fee: &str,
    ) -> Result<RecoveredTxObservation, ServiceError> {
        tracing::info!(tx_hash=?tx_hash, "检测到已有raw_tx和tx_hash，执行恢复检查");

        let _breaker_probe = match chain_rpc_guard::admit_breaker_for_attempt_with_ctx(
            ctx,
            chain_code,
            tx_hash,
            RECOVER_OP_QUERY_TX_RES,
            "recover",
        )
        .await
        {
            chain_rpc_guard::ChainRpcBreakerAttempt::Proceed(probe) => probe,
            chain_rpc_guard::ChainRpcBreakerAttempt::Skip => {
                return Ok(RecoveredTxObservation::QueryUncertain);
            }
        };

        let mut auth_retry = RecoverAuthRetryState::default();
        'recover_auth_retry: loop {
            let adapter =
                match Self::acquire_recover_adapter(ctx, &mut auth_retry, chain_code).await? {
                    RecoverAdapterAcquire::Ready(adapter) => adapter,
                    RecoverAdapterAcquire::RetryAuth => continue 'recover_auth_retry,
                    RecoverAdapterAcquire::StayUncertain => {
                        return Ok(RecoveredTxObservation::QueryUncertain);
                    }
                };
            let rpc = adapter.rpc_endpoint_for_log().unwrap_or_else(|| "<unknown>".to_string());

            match recover_query_outcome(adapter.query_tx_res(tx_hash).await) {
                RecoverQueryOutcome::Found(tx_result) => {
                    Self::log_recover_auth_retry_success(chain_code, &rpc, &auth_retry);
                    return Self::handle_recovered_tx_result(
                        ctx,
                        chain_code,
                        tx_hash,
                        transaction_fee,
                        tx_result,
                    )
                    .await
                    .map(|result| match result {
                        Some(resp) => RecoveredTxObservation::Confirmed(resp),
                        None => RecoveredTxObservation::QueryUncertain,
                    });
                }
                RecoverQueryOutcome::Missing => {
                    return Self::handle_missing_recovered_tx(
                        ctx,
                        chain_code,
                        from_addr,
                        tx_hash,
                        nonce,
                        &rpc,
                        adapter.as_ref(),
                    )
                    .await
                    .map(|result| match result {
                        Some(resp) => RecoveredTxObservation::Confirmed(resp),
                        None => RecoveredTxObservation::Missing,
                    });
                }
                RecoverQueryOutcome::QueryError(service_err) => {
                    match Self::handle_recover_query_error_action(
                        ctx,
                        &mut auth_retry,
                        chain_code,
                        tx_hash,
                        &rpc,
                        &service_err,
                    )
                    .await?
                    {
                        RecoverQueryErrorAction::RetryAuth => continue 'recover_auth_retry,
                        RecoverQueryErrorAction::StayUncertain => {
                            return Ok(RecoveredTxObservation::QueryUncertain);
                        }
                    }
                }
            }
        }
    }

    fn log_recover_auth_retry_success(
        chain_code: &str,
        rpc: &str,
        auth_retry: &RecoverAuthRetryState,
    ) {
        if auth_retry.attempted() {
            tracing::info!(chain_code=%chain_code, rpc=%rpc, op=RECOVER_OP_QUERY_TX_RES, "auth retry succeeded");
        }
    }

    async fn acquire_recover_adapter(
        ctx: &'static Context,
        auth_retry: &mut RecoverAuthRetryState,
        chain_code: &str,
    ) -> Result<RecoverAdapterAcquire, ServiceError> {
        match ApiChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, chain_code).await {
            Ok(adapter) => Ok(RecoverAdapterAcquire::Ready(adapter)),
            Err(e) => match auth_retry.classify_adapter_error(&e) {
                RecoverAdapterErrorDecision::RefreshAuthAndRetry => {
                    Self::prepare_recover_auth_retry(
                        ctx,
                        auth_retry,
                        chain_code,
                        RECOVER_OP_GET_ADAPTER,
                        None,
                        &e,
                    )
                    .await?;
                    Ok(RecoverAdapterAcquire::RetryAuth)
                }
                RecoverAdapterErrorDecision::StayUncertain => {
                    tracing::warn!("process_recovered_tx: 延迟重试, 适配器创建失败: {}", e);
                    Ok(RecoverAdapterAcquire::StayUncertain)
                }
                RecoverAdapterErrorDecision::Fatal => {
                    tracing::error!("process_recovered_tx: 非延迟重试, 适配器创建失败: {}", e);
                    Err(e)
                }
            },
        }
    }

    async fn prepare_recover_auth_retry(
        ctx: &'static Context,
        auth_retry: &mut RecoverAuthRetryState,
        chain_code: &str,
        op: &str,
        rpc: Option<&str>,
        err: &ServiceError,
    ) -> Result<(), ServiceError> {
        auth_retry.mark_attempted();
        rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(ctx, chain_code, op, rpc, err)
            .await
    }

    async fn handle_recover_query_error_action(
        ctx: &'static Context,
        auth_retry: &mut RecoverAuthRetryState,
        chain_code: &str,
        tx_hash: &str,
        rpc: &str,
        service_err: &ServiceError,
    ) -> Result<RecoverQueryErrorAction, ServiceError> {
        match auth_retry.classify_query_error(service_err) {
            RecoverQueryErrorDecision::RefreshAuthAndRetry => {
                Self::prepare_recover_auth_retry(
                    ctx,
                    auth_retry,
                    chain_code,
                    RECOVER_OP_QUERY_TX_RES,
                    Some(rpc),
                    service_err,
                )
                .await?;
                Ok(RecoverQueryErrorAction::RetryAuth)
            }
            RecoverQueryErrorDecision::StayUncertain => {
                log_recover_query_error_with_ctx(
                    ctx,
                    chain_code,
                    tx_hash,
                    rpc,
                    service_err,
                    auth_retry.attempted(),
                )
                .await;
                Ok(RecoverQueryErrorAction::StayUncertain)
            }
        }
    }

    async fn handle_recovered_tx_result(
        ctx: &'static Context,
        chain_code: &str,
        tx_hash: &str,
        transaction_fee: &str,
        tx_result: wallet_chain_interact::QueryTransactionResult,
    ) -> Result<Option<TransferResp>, ServiceError> {
        let is_success = Self::is_recovered_tx_status_success(tx_hash, tx_result.status);
        let time_ms = shared_mechanism::normalize_chain_time_to_ms(tx_result.transaction_time)?;

        if is_success {
            let consumer =
                Self::recovered_resource_consumer(chain_code, &tx_result.resource_consume)?;
            tracing::info!(
                %tx_hash,
                chain_code,
                query_resource_consume = %tx_result.resource_consume,
                consumer_net_used = consumer.as_ref().map(|value| value.net_used),
                consumer_energy_used = consumer.as_ref().map(|value| value.energy_used),
                "Recovered transaction resource prepared"
            );
            tracing::info!(trade_no=?tx_hash, "链上确认成功，直接落成");
            record_recover_rpc_success(ctx, chain_code).await;
            Ok(Some(Self::recovered_transfer_resp(tx_hash, transaction_fee, time_ms, consumer)))
        } else {
            tracing::warn!(trade_no=?tx_hash, "链上失败，直接标记失败");
            Err(ServiceError::System(crate::error::system::SystemError::Internal(
                "broadcasted tx failed".into(),
            )))
        }
    }

    fn recovered_transfer_resp(
        tx_hash: &str,
        transaction_fee: &str,
        transaction_time_ms: u128,
        consumer: Option<BillResourceConsume>,
    ) -> TransferResp {
        let mut resp = TransferResp {
            tx_hash: tx_hash.to_string(),
            fee: transaction_fee.to_string(),
            consumer,
            transaction_time_ms: None,
        };
        resp.with_transaction_time(transaction_time_ms);
        resp
    }

    fn recovered_resource_consumer(
        chain_code: &str,
        resource_consume: &str,
    ) -> Result<Option<BillResourceConsume>, ServiceError> {
        if shared_mechanism::is_tron_chain(chain_code) {
            return Ok(Some(BillResourceConsume::from_json_str(resource_consume)?));
        }
        Ok(None)
    }

    fn is_recovered_tx_status_success(tx_hash: &str, status: i8) -> bool {
        match status {
            2 => true,
            3 => false,
            _ => {
                tracing::warn!(trade_no=?tx_hash, "链上交易状态异常: {}", status);
                false
            }
        }
    }

    async fn handle_missing_recovered_tx(
        ctx: &'static Context,
        chain_code: &str,
        from_addr: &str,
        tx_hash: &str,
        nonce: i64,
        rpc: &str,
        adapter: &(dyn Tx + Sync),
    ) -> Result<Option<TransferResp>, ServiceError> {
        tracing::info!(trade_no=?tx_hash, "链上未找到该交易，执行恢复判定");

        match classify_recover_missing_tx_route(chain_code) {
            RecoverMissingTxRoute::SolanaVisibilityProbe => {
                Self::probe_sol_visibility_during_recover(ctx, chain_code, tx_hash, rpc, adapter)
                    .await;
                Self::log_recover_missing_tx_wait(
                    tx_hash,
                    RecoverMissingTxRoute::SolanaVisibilityProbe,
                );
                Ok(None)
            }
            RecoverMissingTxRoute::TronPendingPool => {
                Self::handle_tron_missing_recovered_tx(ctx, chain_code, tx_hash, rpc, adapter).await
            }
            RecoverMissingTxRoute::EvmNonceCheck => {
                Self::handle_evm_missing_recovered_tx(ctx, chain_code, from_addr, tx_hash, nonce)
                    .await
            }
            RecoverMissingTxRoute::WaitForScanner => {
                Self::log_recover_missing_tx_wait(tx_hash, RecoverMissingTxRoute::WaitForScanner);
                Ok(None)
            }
        }
    }

    fn log_recover_missing_tx_wait(tx_hash: &str, route: RecoverMissingTxRoute) {
        if let Some(reason) = route.wait_reason() {
            tracing::info!(trade_no=?tx_hash, "{}", reason);
        }
    }

    async fn probe_sol_visibility_during_recover(
        ctx: &'static Context,
        chain_code: &str,
        tx_hash: &str,
        rpc: &str,
        adapter: &(dyn Tx + Sync),
    ) {
        match adapter.tx_visibility_probe(tx_hash).await {
            Ok(Some(probe)) => {
                shared_mechanism::log_visibility_probe(
                    chain_code, tx_hash, rpc, "sol", &probe, None, None, "recover",
                );
            }
            Ok(None) => {
                tracing::info!(
                    trade_no = ?tx_hash,
                    rpc = %rpc,
                    "Solana visibility probe unavailable during recover"
                );
            }
            Err(e) => {
                log_sol_visibility_probe_error_with_ctx(ctx, chain_code, tx_hash, rpc, &e).await;
            }
        }
    }

    async fn handle_tron_missing_recovered_tx(
        ctx: &'static Context,
        chain_code: &str,
        tx_hash: &str,
        rpc: &str,
        adapter: &(dyn Tx + Sync),
    ) -> Result<Option<TransferResp>, ServiceError> {
        match adapter.has_pending_tx(tx_hash).await {
            Ok(true) => {
                tracing::info!(
                    trade_no=?tx_hash,
                    "Tron pending pool still sees tx hash; keep observing"
                );
                Ok(None)
            }
            Ok(false) => {
                tracing::warn!(
                    trade_no=?tx_hash,
                    "Tron tx missing from both confirmed and pending pools"
                );
                Err(ServiceError::System(crate::error::system::SystemError::Internal(
                    "tron tx missing from confirmed and pending pools".into(),
                )))
            }
            Err(err) => {
                log_tron_pending_pool_query_error_with_ctx(ctx, chain_code, tx_hash, rpc, &err)
                    .await;
                Ok(None)
            }
        }
    }

    async fn handle_evm_missing_recovered_tx(
        ctx: &'static Context,
        chain_code: &str,
        from_addr: &str,
        tx_hash: &str,
        nonce: i64,
    ) -> Result<Option<TransferResp>, ServiceError> {
        let chain_nonce = match Self::nonce_with_ctx(ctx, from_addr, chain_code).await {
            Ok(chain_nonce) => chain_nonce,
            Err(e) => {
                if e.is_network_error() {
                    tracing::warn!(trade_no=?tx_hash, "恢复判定获取链上nonce失败(网络瞬时)，等待下一轮: {}", e);
                    return Ok(None);
                }
                tracing::error!(trade_no=?tx_hash, "恢复判定获取链上nonce失败: {}", e);
                return Err(e);
            }
        };

        let local_nonce = nonce as u64;
        tracing::warn!(
            trade_no=?tx_hash,
            chain_nonce = chain_nonce,
            local_nonce = local_nonce,
            "EVM recover: tx hash missing on-chain, comparing nonce state"
        );

        log_evm_missing_tx_hash_state(tx_hash, chain_nonce, local_nonce);
        Ok(None)
    }
}

#[cfg(test)]
mod recover_tx_status_tests {
    use super::ApiTransDomain;
    use wallet_chain_interact::BillResourceConsume;

    #[test]
    fn recovered_tx_status_two_is_success() {
        assert!(ApiTransDomain::is_recovered_tx_status_success("0xtx", 2));
    }

    #[test]
    fn recovered_tx_status_three_is_failure() {
        assert!(!ApiTransDomain::is_recovered_tx_status_success("0xtx", 3));
    }

    #[test]
    fn recovered_tx_unknown_status_is_failure() {
        assert!(!ApiTransDomain::is_recovered_tx_status_success("0xtx", 1));
    }

    #[test]
    fn recovered_transfer_resp_keeps_fee_chain_time_and_resource_consume() {
        let resp = ApiTransDomain::recovered_transfer_resp(
            "0xtx",
            "1.23",
            1_746_040_371_123,
            Some(BillResourceConsume { net_used: 321, energy_used: 13_532 }),
        );

        assert_eq!(resp.tx_hash, "0xtx");
        assert_eq!(resp.fee, "1.23");
        assert_eq!(resp.transaction_time_ms, Some(1_746_040_371_123));
        let consumer = resp.consumer.expect("TRON resource consume should be retained");
        assert_eq!(consumer.net_used, 321);
        assert_eq!(consumer.energy_used, 13_532);
    }

    #[test]
    fn recovered_resource_consumer_reuses_tron_query_result() {
        let consumer = ApiTransDomain::recovered_resource_consumer(
            "TRON",
            r#"{"net_used":0,"energy_used":13532}"#,
        )
        .expect("TRON resource consume should parse")
        .expect("TRON resource consume should be present");

        assert_eq!(consumer.net_used, 0);
        assert_eq!(consumer.energy_used, 13_532);
    }
}
