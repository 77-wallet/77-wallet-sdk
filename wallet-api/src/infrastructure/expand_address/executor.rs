// executor.rs
use wallet_utils::{RetryableError as _, error::RetryPolicy};

use crate::{error::service::ServiceError, infrastructure::expand_address::service::ExpandService};

/// 执行结果类型 - 明确失败是否可重试
///
/// 核心设计：
/// - 明确区分成功、可重试失败和致命失败
/// - 让 Executor → Worker → Scanner 的失败语义显式、不可误解
/// - 职责分离：Executor 决定"能不能重试"，Scanner 决定"何时重试"
#[derive(Debug)]
pub enum ExecOutcome {
    /// 成功完成，无需额外处理
    Success,

    /// 执行失败，但**可安全重试**
    Retryable { reason: RetryReason },

    /// 执行失败，**重试没有意义**
    Fatal { reason: FatalReason },
}

/// 可重试失败原因
#[derive(Debug)]
#[allow(dead_code)]
pub enum RetryReason {
    /// 网络错误
    Network,
    /// 超时错误
    Timeout,
    /// 后端服务不可用
    BackendUnavailable,
    /// 临时错误
    Temporary,
}

/// 致命失败原因
#[derive(Debug)]
pub enum FatalReason {
    /// 钱包不存在
    WalletNotFound,
    /// 批次不存在
    BatchNotFound,
    /// 无效索引
    InvalidIndex,
    /// 不变量违反
    InvariantViolation,
    /// 后端拒绝请求
    BackendRejected,
}

/// ExpandExecutor - 执行具体的create/init操作
///
/// 核心设计：
/// - 无状态：不保存任何状态信息
/// - 不修改DB：只执行操作，不更新数据库状态
/// - 明确返回结果：不决定状态流转，只返回执行成功/失败及可重试性
/// - 职责单一：仅负责执行具体的扩容操作
/// - 语义清晰：将业务错误翻译成系统可理解的执行结果
/// 🔒 明确边界声明：
/// 🔒 ExecOutcome 不是 Scanner 的状态来源，只是 Worker 的"执行态语义"
/// 🔒 Scanner 的状态推进只能基于 DB 事实，禁止基于 ExecOutcome 直接推进状态
/// 🔒 ExecOutcome 只影响 Worker 内部是否 retry，不允许直接修改 Item / Batch 状态
/// 🔒 禁止在 Executor 中修改 DB 状态，必须由 Scanner 基于 DB 事实推进状态
pub struct ExpandExecutor {
    ctx: &'static crate::context::Context,
}

impl ExpandExecutor {
    pub fn new(ctx: &'static crate::context::Context) -> Self {
        Self { ctx }
    }

    /// 执行账户创建操作
    // #[instrument(skip(self))]
    pub async fn execute_create(
        &self,
        uid: &str,
        chain: &str,
        indices: &[i32],
        batch_id: &str,
    ) -> Result<ExecOutcome, ServiceError> {
        tracing::info!(uid=%uid, chain=%chain, batch_id=%batch_id, indices_count=indices.len(), "ExpandExecutor: executing create account");

        // 执行实际的账户创建操作
        match ExpandService::create_account_with_ctx(self.ctx, uid, chain, indices, batch_id).await
        {
            Ok(_) => {
                tracing::info!(uid=%uid, chain=%chain, batch_id=%batch_id, indices_count=indices.len(), "ExpandExecutor: create account succeeded");
                Ok(ExecOutcome::Success)
            }
            Err(e) => {
                // 明确区分可重试和不可重试的错误
                // 可重试：网络、超时、后端不可用等临时错误
                // 不可重试：钱包不存在、无效索引、不变量违反等永久性错误
                tracing::error!(error = %e, uid=%uid, chain=%chain, batch_id=%batch_id, "account create failed");

                // 根据错误类型返回不同的ExecOutcome
                match e {
                    // 可重试错误（网络错误、限流、上游不可用等）
                    e if matches!(e.retry_policy(), RetryPolicy::Delay) => {
                        tracing::warn!(error = %e, "account create failed with retryable error, retryable");
                        Ok(ExecOutcome::Retryable { reason: RetryReason::Temporary })
                    }
                    // 不可重试的错误
                    ServiceError::Business(ref biz_err) => {
                        // 业务错误通常不可重试，但需细分类型
                        tracing::error!(error = %e, "account create failed with business error, not retryable");

                        // 细分类业务错误到具体的FatalReason
                        let reason = match biz_err {
                            // 钱包不存在
                            crate::error::business::BusinessError::Wallet(
                                crate::error::business::wallet::WalletError::NotFound
                            ) => FatalReason::WalletNotFound,
                            // API钱包账户不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::NotFoundAccount
                            ) => FatalReason::WalletNotFound,
                            // API账户不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::Account(
                                    crate::error::business::api_wallet::account::AccountError::NotFound
                                )
                            ) => FatalReason::WalletNotFound,
                            // 扩容批次不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::Account(
                                    crate::error::business::api_wallet::account::AccountError::ExpandBatchNotFound
                                )
                            ) => FatalReason::BatchNotFound,
                            // 其他业务错误视为后端拒绝
                            _ => FatalReason::BackendRejected,
                        };

                        Ok(ExecOutcome::Fatal { reason })
                    }
                    ServiceError::System(_) => {
                        // 系统错误，特别是不变量违反，不可重试
                        tracing::error!(error = %e, "account create failed with system error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::InvariantViolation })
                    }
                    ServiceError::Parameter(_) => {
                        // 参数错误，不可重试
                        tracing::error!(error = %e, "account create failed with parameter error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::InvalidIndex })
                    }
                    ServiceError::Database(_) => {
                        // 数据库错误，某些情况下可能不可重试
                        // 这里简化处理，将所有数据库错误视为不可重试
                        tracing::error!(error = %e, "account create failed with database error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::BackendRejected })
                    }
                    // 其他非网络错误，默认不可重试
                    _ => {
                        tracing::error!(error = %e, "account create failed with unexpected error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::BackendRejected })
                    }
                }
            }
        }
    }

    /// 执行账户初始化操作
    // #[instrument(skip(self))]
    pub async fn execute_init(
        &self,
        uid: &str,
        chain: &str,
        indices: &[i32],
        batch_id: &str,
    ) -> Result<ExecOutcome, ServiceError> {
        tracing::info!(uid=%uid, chain=%chain, batch_id=%batch_id, indices_count=indices.len(), "ExpandExecutor: executing init account");

        // 执行实际的账户初始化操作
        match ExpandService::init_account_with_ctx(self.ctx, uid, chain, indices, batch_id).await {
            Ok(_) => {
                tracing::info!(uid=%uid, chain=%chain, batch_id=%batch_id, indices_count=indices.len(), "ExpandExecutor: init account succeeded");
                Ok(ExecOutcome::Success)
            }
            Err(e) => {
                // 明确区分可重试和不可重试的错误
                // 可重试：网络、超时、后端不可用等临时错误
                // 不可重试：钱包不存在、无效索引、不变量违反等永久性错误
                tracing::error!(error = %e, uid=%uid, chain=%chain, batch_id=%batch_id, "account init failed");

                // 根据错误类型返回不同的ExecOutcome
                match e {
                    // 可重试错误（网络错误、限流、上游不可用等）
                    e if matches!(e.retry_policy(), RetryPolicy::Delay) => {
                        tracing::warn!(error = %e, "account init failed with retryable error, retryable");
                        Ok(ExecOutcome::Retryable { reason: RetryReason::Temporary })
                    }
                    // 不可重试的错误
                    ServiceError::Business(ref biz_err) => {
                        // 业务错误通常不可重试，但需细分类型
                        tracing::error!(error = %e, "account init failed with business error, not retryable");

                        // 细分类业务错误到具体的FatalReason
                        let reason = match biz_err {
                            // 钱包不存在
                            crate::error::business::BusinessError::Wallet(
                                crate::error::business::wallet::WalletError::NotFound
                            ) => FatalReason::WalletNotFound,
                            // API钱包账户不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::NotFoundAccount
                            ) => FatalReason::WalletNotFound,
                            // API账户不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::Account(
                                    crate::error::business::api_wallet::account::AccountError::NotFound
                                )
                            ) => FatalReason::WalletNotFound,
                            // 扩容批次不存在
                            crate::error::business::BusinessError::ApiWallet(
                                crate::error::business::api_wallet::ApiWalletError::Account(
                                    crate::error::business::api_wallet::account::AccountError::ExpandBatchNotFound
                                )
                            ) => FatalReason::BatchNotFound,
                            // 其他业务错误视为后端拒绝
                            _ => FatalReason::BackendRejected,
                        };

                        Ok(ExecOutcome::Fatal { reason })
                    }
                    ServiceError::System(_) => {
                        // 系统错误，特别是不变量违反，不可重试
                        tracing::error!(error = %e, "account init failed with system error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::InvariantViolation })
                    }
                    ServiceError::Parameter(_) => {
                        // 参数错误，不可重试
                        tracing::error!(error = %e, "account init failed with parameter error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::InvalidIndex })
                    }
                    ServiceError::Database(_) => {
                        // 数据库错误，某些情况下可能不可重试
                        // 这里简化处理，将所有数据库错误视为不可重试
                        tracing::error!(error = %e, "account init failed with database error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::BackendRejected })
                    }
                    // 其他非网络错误，默认不可重试
                    _ => {
                        tracing::error!(error = %e, "account init failed with unexpected error, not retryable");
                        Ok(ExecOutcome::Fatal { reason: FatalReason::BackendRejected })
                    }
                }
            }
        }
    }

    /// 执行扩容完成通知操作
    // #[instrument(skip(self))]
    pub async fn execute_notify(
        &self,
        uid: &str,
        batch_id: &str,
    ) -> Result<ExecOutcome, ServiceError> {
        tracing::info!(uid=%uid, batch_id=%batch_id, "ExpandExecutor: executing notify expand complete");

        // 执行实际的通知操作
        match ExpandService::expand_complete_with_ctx(self.ctx, uid, batch_id).await {
            Ok(_) => Ok(ExecOutcome::Success),
            Err(e) => {
                tracing::error!(error = %e, uid=%uid, batch_id=%batch_id, "expand complete notify failed");

                // 细分类业务错误到具体的FatalReason
                let reason = match e {
                    // 业务错误需细分类型
                    ServiceError::Business(ref biz_err) => match biz_err {
                        // 钱包不存在
                        crate::error::business::BusinessError::Wallet(
                            crate::error::business::wallet::WalletError::NotFound
                        ) => FatalReason::WalletNotFound,
                        // API钱包账户不存在
                        crate::error::business::BusinessError::ApiWallet(
                            crate::error::business::api_wallet::ApiWalletError::NotFoundAccount
                        ) => FatalReason::WalletNotFound,
                        // API账户不存在
                        crate::error::business::BusinessError::ApiWallet(
                            crate::error::business::api_wallet::ApiWalletError::Account(
                                crate::error::business::api_wallet::account::AccountError::NotFound
                            )
                        ) => FatalReason::WalletNotFound,
                        // 扩容批次不存在
                        crate::error::business::BusinessError::ApiWallet(
                            crate::error::business::api_wallet::ApiWalletError::Account(
                                crate::error::business::api_wallet::account::AccountError::ExpandBatchNotFound
                            )
                        ) => FatalReason::BatchNotFound,
                        // 其他业务错误视为后端拒绝
                        _ => FatalReason::BackendRejected,
                    },
                    // 其他错误视为后端拒绝
                    _ => FatalReason::BackendRejected,
                };

                Ok(ExecOutcome::Fatal { reason })
            }
        }
    }
}
