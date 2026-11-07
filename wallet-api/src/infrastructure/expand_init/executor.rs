// expand_init/executor.rs

use crate::{domain::app::config::ConfigDomain, error::service::ServiceError};
use wallet_database::repositories::api_wallet::account::ApiAccountRepo;
use wallet_transport_backend::request::api_wallet::address::ApiAddressInitReq;
/// 异步执行INIT任务
/// 执行逻辑：
/// 1. 检查keys_reset_status
/// 2. 调用backend.expand_address接口
/// 3. 更新数据库中account的init状态
/// 4. 记录执行结果
/// 参数：init_req - INIT请求
/// 返回：Result<(), ServiceError> - 执行结果
pub async fn do_init_with_ctx(
    ctx: &'static crate::context::Context,
    init_req: ApiAddressInitReq,
) -> Result<(), ServiceError> {
    tracing::info!(
        batch_id_hash = %crate::domain::api_wallet::chain::recovery::log_id(init_req.batch_id.as_deref().unwrap_or("")),
        address_count = init_req.address_list.0.len(),
        epoch = init_req.epoch,
        "INIT_EXECUTOR: starting init task"
    );

    // 1. 检查epoch是否为0（安全防护）
    if init_req.epoch == 0 {
        tracing::warn!(
            batch_id_hash = %crate::domain::api_wallet::chain::recovery::log_id(init_req.batch_id.as_deref().unwrap_or("")),
            "INIT_EXECUTOR: INIT without valid epoch detected, dropping task"
        );
        // 直接返回成功，不执行后续逻辑
        return Ok(());
    }

    // 2. 检查Epoch有效性（核心校验）
    let is_valid = ConfigDomain::check_epoch_validity(ctx, init_req.epoch).await?;
    if !is_valid {
        tracing::info!(
            batch_id_hash = %crate::domain::api_wallet::chain::recovery::log_id(init_req.batch_id.as_deref().unwrap_or("")),
            task_epoch = init_req.epoch,
            "INIT_EXECUTOR: Task epoch mismatch, discarding old task"
        );
        // 直接返回成功，不执行后续逻辑
        return Ok(());
    }

    // 2. 检查keys_reset_status（向后兼容）
    let status = ConfigDomain::get_keys_reset_status(ctx).await?;
    if let Some(status) = status
        && let Some(false) = status.status
    {
        tracing::error!("INIT_EXECUTOR: Keys not reset, cannot process address init request");
        return Err(crate::error::business::BusinessError::Config(
            crate::error::business::config::ConfigError::KeysNotReset,
        )
        .into());
    }

    // 2. 获取backend实例并调用expand_address接口
    let backend = ctx.get_global_backend_api();

    tracing::info!(
        "INIT_EXECUTOR: calling backend.expand_address, address_count={}, batch_id_hash={}",
        init_req.address_list.0.len(),
        crate::domain::api_wallet::chain::recovery::log_id(
            init_req.batch_id.as_deref().unwrap_or("")
        )
    );

    let result = backend.expand_address(&init_req).await;

    match result {
        Ok(_) => {
            tracing::info!(
                "INIT_EXECUTOR: backend.expand_address completed successfully, address_count={}",
                init_req.address_list.0.len()
            );

            // A reset can invalidate an in-flight backend response.
            if !ConfigDomain::check_epoch_validity(ctx, init_req.epoch).await? {
                return Ok(());
            }
            // 3. 更新数据库中account的init状态
            let pool = ctx.api_wallet_pool()?;

            tracing::info!("INIT_EXECUTOR: starting database operations for address init");
            let mut pairs = Vec::new();

            for address in init_req.address_list.0.iter() {
                pairs.push((address.address.clone(), address.chain_code.clone()));
            }
            tracing::info!("INIT_EXECUTOR: init_many pairs: {:?}", pairs.len());
            ApiAccountRepo::init_many(&pool, &pairs).await?;
            tracing::info!(
                batch_id_hash = %crate::domain::api_wallet::chain::recovery::log_id(init_req.batch_id.as_deref().unwrap_or("")),
                address_count = init_req.address_list.0.len(),
                "INIT_EXECUTOR: init task completed successfully"
            );
            Ok(())
        }
        Err(e) => {
            let backend_code = match &e {
                wallet_transport_backend::Error::ApiBackend(code, _) => Some(*code),
                _ => None,
            };
            tracing::error!(
                batch_id_hash = %crate::domain::api_wallet::chain::recovery::log_id(init_req.batch_id.as_deref().unwrap_or("")),
                address_count = init_req.address_list.0.len(),
                reason = "backend",
                backend_code = ?backend_code,
                "INIT_EXECUTOR: init task failed"
            );
            Err(ServiceError::from(e))
        }
    }
}
