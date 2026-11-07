use super::{ApiTransDomain, broadcast_mechanism, shared_mechanism};
use crate::{
    context::Context,
    domain::{
        api_wallet::{adapter::tx::RawTx, adapter_factory::ApiChainAdapterFactory},
        chain::{TransferResp, tx_visibility::VisibilityAttempts},
    },
    error::service::ServiceError,
    infrastructure::{api_trans::rpc_auth_retry_mechanism, chain_rpc_guard},
};
use std::time::Instant;
use wallet_types::chain::chain::ChainCode;
use wallet_utils::RetryableError as _;

impl ApiTransDomain {
    /// ⚠️ 注意：
    /// 网络错误不等于广播失败。
    /// broadcast_transfer 在网络异常时返回 Ok(None)，
    /// 由 scanner / recover 决定最终状态。
    pub async fn broadcast_transfer(
        ctx: &'static Context,
        chain_code: &str,
        raw: RawTx,
        expected_tx_hash: Option<&str>,
    ) -> Result<Option<TransferResp>, ServiceError> {
        Self::broadcast_transfer_with_ctx(ctx, chain_code, raw, expected_tx_hash).await
    }

    /// ⚠️ 注意：
    /// 网络错误不等于广播失败。
    /// broadcast_transfer 在网络异常时返回 Ok(None)，
    /// 由 scanner / recover 决定最终状态。
    pub async fn broadcast_transfer_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        raw: RawTx,
        expected_tx_hash: Option<&str>,
    ) -> Result<Option<TransferResp>, ServiceError> {
        Self::broadcast_transfer_with_policy(ctx, chain_code, raw, expected_tx_hash, false).await
    }

    /// A matching node acknowledgment is submission, not a successful receipt.
    /// Only the ordinary signed EVM initial-send owner selects this policy;
    /// duplicate errors and unknown send results retain the visibility guard.
    pub(crate) async fn broadcast_signed_self_transfer_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        raw: RawTx,
        expected_tx_hash: &str,
    ) -> Result<Option<TransferResp>, ServiceError> {
        Self::broadcast_transfer_with_policy(ctx, chain_code, raw, Some(expected_tx_hash), true)
            .await
    }

    async fn broadcast_transfer_with_policy(
        ctx: &'static Context,
        chain_code: &str,
        raw: RawTx,
        expected_tx_hash: Option<&str>,
        accept_matching_evm_ack: bool,
    ) -> Result<Option<TransferResp>, ServiceError> {
        let is_evm_raw = matches!(&raw, RawTx::Evm(..));
        let start_time = Instant::now();
        tracing::info!("broadcast_transfer (开始): 链: {}, 时间: {:?}", chain_code, start_time);

        let tx_hash_hint =
            broadcast_mechanism::select_broadcast_tx_hash_hint(&raw, expected_tx_hash);
        if let RawTx::Evm(raw, ..) = &raw {
            tracing::info!(
                chain_code = %chain_code,
                local_raw_hash = %broadcast_mechanism::evm_raw_hash_hint(raw),
                raw_len = raw.len(),
                "broadcast_transfer prepared EVM raw tx hash hint"
            );
        }

        let _breaker_probe = match chain_rpc_guard::admit_breaker_for_attempt_with_ctx(
            ctx,
            chain_code,
            tx_hash_hint.as_deref().unwrap_or(""),
            "broadcast_transfer",
            "api_wallet",
        )
        .await
        {
            chain_rpc_guard::ChainRpcBreakerAttempt::Proceed(probe) => probe,
            chain_rpc_guard::ChainRpcBreakerAttempt::Skip => return Ok(None),
        };

        let adapter_time = Instant::now();
        let mut auth_retry_attempted = false;
        let mut first_raw_attempt = Some(raw);
        let mut retry_raw_once =
            first_raw_attempt.as_ref().and_then(broadcast_mechanism::clone_raw_for_single_retry);
        'auth_retry: loop {
            let adapter =
                match ApiChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, chain_code)
                    .await
                {
                    Ok(adapter) => adapter,
                    Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                        auth_retry_attempted = true;
                        rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                            ctx,
                            chain_code,
                            "broadcast_transfer:get_adapter",
                            None,
                            &e,
                        )
                        .await?;
                        continue 'auth_retry;
                    }
                    Err(e) => {
                        if e.is_delay_retryable() {
                            tracing::warn!("broadcast_transfer: 延迟重试, 适配器创建失败: {}", e);
                            return Ok(None);
                        }
                        tracing::error!("broadcast_transfer: 非延迟重试, 适配器创建失败: {}", e);
                        return Err(e);
                    }
                };
            tracing::info!(
                "broadcast_transfer (适配器创建): 完成, 耗时: {:?}",
                adapter_time.elapsed()
            );
            let rpc = adapter.rpc_endpoint_for_log().unwrap_or_else(|| "<unknown>".to_string());

            let raw_for_broadcast = if let Some(raw_once) = first_raw_attempt.take() {
                raw_once
            } else if let Some(raw_retry) = retry_raw_once.take() {
                raw_retry
            } else {
                tracing::warn!(
                    chain_code = %chain_code,
                    rpc = %rpc,
                    "auth retry requested but raw tx is not cloneable for this chain; treat as uncertain"
                );
                return Ok(None);
            };

            let resp = match adapter.broadcast_transfer(raw_for_broadcast).await {
                Ok(resp) => {
                    if accept_matching_evm_ack && is_evm_raw {
                        let expected = expected_tx_hash.expect("signed submission requires hash");
                        if !resp.tx_hash.eq_ignore_ascii_case(expected) {
                            return Err(ServiceError::Parameter(format!(
                                "transfer_broadcast_hash_mismatch: expected={expected}"
                            )));
                        }
                        chain_rpc_guard::record_success_for_chain_code_with_ctx(ctx, chain_code)
                            .await;
                        tracing::info!(chain_code=%chain_code, tx_hash=%resp.tx_hash,
                            "ordinary signed EVM transaction submitted; receipt pending");
                        return Ok(Some(resp));
                    }
                    resp
                }
                Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                    auth_retry_attempted = true;
                    rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                        ctx,
                        chain_code,
                        "broadcast_transfer:broadcast",
                        Some(&rpc),
                        &e,
                    )
                    .await?;
                    continue 'auth_retry;
                }
                Err(e) => {
                    // A refresh can follow a successful send's visibility probe.
                    // A later nonce-too-high response cannot reject that first send.
                    use crate::infrastructure::nonce::nonce_engine::{
                        ChainErrorNormalizer, NonceErrorKind,
                    };
                    if auth_retry_attempted
                        && is_evm_raw
                        && matches!(
                            ChainErrorNormalizer::normalize(&e.to_string()),
                            NonceErrorKind::NonceTooHigh
                        )
                    {
                        tracing::warn!(chain_code, error = %e, "EVM nonce rejection after auth retry retains uncertain submission");
                        return Ok(None);
                    }
                    if broadcast_mechanism::is_duplicate_broadcast_error(&e) {
                        let tx_hash = if let Some(tx_hash) =
                            broadcast_mechanism::resolve_duplicate_tx_hash(
                                expected_tx_hash,
                                tx_hash_hint.as_deref(),
                            ) {
                            tx_hash
                        } else {
                            tracing::warn!(
                                chain_code = %chain_code,
                                error = %e,
                                "broadcast duplicate/exists but missing tx_hash; treat as uncertain"
                            );
                            return Ok(None);
                        };
                        let synthetic = TransferResp::new(tx_hash, String::new());
                        if shared_mechanism::need_broadcast_visibility_check(chain_code) {
                            tracing::warn!(
                                chain_code = %chain_code,
                                tx_hash = %synthetic.tx_hash,
                                error = %e,
                                "broadcast duplicate/exists; will verify same-rpc visibility before treating as success"
                            );
                        } else {
                            tracing::warn!(
                                chain_code = %chain_code,
                                tx_hash = %synthetic.tx_hash,
                                error = %e,
                                "broadcast duplicate/exists; treat as idempotent success"
                            );
                            return Ok(Some(synthetic));
                        }
                        synthetic
                    } else if chain_code.eq_ignore_ascii_case(&ChainCode::Tron.to_string())
                        && broadcast_mechanism::is_transaction_expired_error(&e)
                    {
                        chain_rpc_guard::warn_failure_and_record_with_ctx(
                            ctx,
                            chain_code,
                            &e,
                            chain_rpc_guard::ChainRpcFailureLog::new()
                                .chain_code(chain_code)
                                .rpc(&rpc)
                                .op("broadcast_transfer"),
                            "broadcast transaction expired; return error so raw tx can be rebuilt",
                        )
                        .await;
                        return Err(e);
                    } else if e.is_network_error() {
                        chain_rpc_guard::error_failure_and_record_with_ctx(
                            ctx,
                            chain_code,
                            &e,
                            chain_rpc_guard::ChainRpcFailureLog::new()
                                .chain_code(chain_code)
                                .rpc(&rpc)
                                .op("broadcast_transfer"),
                            "broadcast_transfer: 网络错误, 交易广播失败",
                        )
                        .await;
                        if auth_retry_attempted {
                            tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", error=%e, "auth retry failed");
                        }
                        return Ok(None);
                    } else {
                        if auth_retry_attempted {
                            tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", error=%e, "auth retry failed");
                        }
                        return Err(e);
                    }
                }
            };

            tracing::info!("broadcast_transfer: 转账操作完成, 耗时: {:?}", start_time.elapsed());
            tracing::info!(
                chain_code = %chain_code,
                tx_hash = %resp.tx_hash,
                rpc = %rpc,
                "broadcast_transfer response received"
            );

            if shared_mechanism::need_broadcast_visibility_check(chain_code) {
                let visibility_kind =
                    if shared_mechanism::is_sol_chain(chain_code) { "sol" } else { "evm" };
                let visibility_probe = match adapter.tx_visibility_probe(&resp.tx_hash).await {
                    Ok(probe) => probe,
                    Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                        auth_retry_attempted = true;
                        rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                            ctx,
                            chain_code,
                            "broadcast_transfer:visibility_probe",
                            Some(&rpc),
                            &e,
                        )
                        .await?;
                        continue 'auth_retry;
                    }
                    Err(e) => {
                        if e.is_delay_retryable() {
                            chain_rpc_guard::warn_failure_and_record_with_ctx(
                                ctx,
                                chain_code,
                                &e,
                                chain_rpc_guard::ChainRpcFailureLog::new()
                                    .chain_code(chain_code)
                                    .rpc(&rpc)
                                    .tx_hash(&resp.tx_hash)
                                    .visibility_kind(visibility_kind)
                                    .op("broadcast_transfer:visibility_probe"),
                                "broadcast visibility probe failed with delay retryable error",
                            )
                            .await;
                        } else {
                            chain_rpc_guard::error_failure_and_record_with_ctx(
                                ctx,
                                chain_code,
                                &e,
                                chain_rpc_guard::ChainRpcFailureLog::new()
                                    .chain_code(chain_code)
                                    .rpc(&rpc)
                                    .tx_hash(&resp.tx_hash)
                                    .visibility_kind(visibility_kind)
                                    .op("broadcast_transfer:visibility_probe"),
                                "broadcast visibility probe failed",
                            )
                            .await;
                        }
                        if auth_retry_attempted {
                            tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", error=%e, "auth retry failed");
                        }
                        return Ok(None);
                    }
                };

                if let Some(probe) = &visibility_probe {
                    shared_mechanism::log_visibility_probe(
                        chain_code,
                        &resp.tx_hash,
                        &rpc,
                        visibility_kind,
                        probe,
                        None,
                        None,
                        "start",
                    );
                } else {
                    tracing::info!(
                        chain_code = %chain_code,
                        tx_hash = %resp.tx_hash,
                        rpc = %rpc,
                        visibility_kind = %visibility_kind,
                        "broadcast visibility check start"
                    );
                }

                let mut attempts = VisibilityAttempts::default();
                while let Some((attempt, delay_ms)) = attempts.next().await {
                    if let Some(probe) = &visibility_probe {
                        if probe.seen_on_node {
                            shared_mechanism::log_visibility_probe(
                                chain_code,
                                &resp.tx_hash,
                                &rpc,
                                visibility_kind,
                                probe,
                                Some(attempt),
                                Some(*delay_ms),
                                "hit",
                            );
                            chain_rpc_guard::record_success_for_chain_code_with_ctx(
                                ctx, chain_code,
                            )
                            .await;
                            if auth_retry_attempted {
                                tracing::info!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", "auth retry succeeded");
                            }
                            return Ok(Some(resp));
                        }

                        shared_mechanism::log_visibility_probe(
                            chain_code,
                            &resp.tx_hash,
                            &rpc,
                            visibility_kind,
                            probe,
                            Some(attempt),
                            Some(*delay_ms),
                            "pending miss",
                        );
                    } else {
                        match adapter.query_tx_seen_on_node(&resp.tx_hash).await {
                            Ok(true) => {
                                tracing::info!(
                                    chain_code = %chain_code,
                                    tx_hash = %resp.tx_hash,
                                    rpc = %rpc,
                                    attempt = attempt,
                                    delay_ms = *delay_ms,
                                    visibility_kind = %visibility_kind,
                                    "broadcast visibility check hit"
                                );
                                chain_rpc_guard::record_success_for_chain_code_with_ctx(
                                    ctx, chain_code,
                                )
                                .await;
                                if auth_retry_attempted {
                                    tracing::info!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", "auth retry succeeded");
                                }
                                return Ok(Some(resp));
                            }
                            Ok(false) => {
                                tracing::info!(
                                    chain_code = %chain_code,
                                    tx_hash = %resp.tx_hash,
                                    rpc = %rpc,
                                    attempt = attempt,
                                    delay_ms = *delay_ms,
                                    visibility_kind = %visibility_kind,
                                    "broadcast visibility check pending miss"
                                );
                            }
                            Err(e) if !auth_retry_attempted && e.is_rpc_auth_unauthorized() => {
                                auth_retry_attempted = true;
                                rpc_auth_retry_mechanism::refresh_rpc_auth_and_prepare_retry(
                                    ctx,
                                    chain_code,
                                    "broadcast_transfer:visibility_check",
                                    Some(&rpc),
                                    &e,
                                )
                                .await?;
                                continue 'auth_retry;
                            }
                            Err(e) => {
                                chain_rpc_guard::warn_failure_and_record_with_ctx(
                                    ctx,
                                    chain_code,
                                    &e,
                                    chain_rpc_guard::ChainRpcFailureLog::new()
                                        .chain_code(chain_code)
                                        .rpc(&rpc)
                                        .tx_hash(&resp.tx_hash)
                                        .visibility_kind(visibility_kind)
                                        .op("broadcast_transfer:visibility_check")
                                        .attempt(attempt as u32)
                                        .delay_ms(*delay_ms as u64),
                                    "broadcast visibility check miss (uncertain)",
                                )
                                .await;
                                if auth_retry_attempted {
                                    tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", error=%e, "auth retry failed");
                                }
                                return Ok(None);
                            }
                        }
                    }
                }

                tracing::warn!(
                    chain_code = %chain_code,
                    tx_hash = %resp.tx_hash,
                    rpc = %rpc,
                    visibility_kind = %visibility_kind,
                    "broadcast visibility check miss (uncertain)"
                );
                if auth_retry_attempted {
                    tracing::warn!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", "auth retry failed");
                }
                return Ok(None);
            }

            chain_rpc_guard::record_success_for_chain_code_with_ctx(ctx, chain_code).await;
            if auth_retry_attempted {
                tracing::info!(chain_code=%chain_code, rpc=%rpc, op="broadcast_transfer", "auth retry succeeded");
            }
            return Ok(Some(resp));
        }
    }
}
