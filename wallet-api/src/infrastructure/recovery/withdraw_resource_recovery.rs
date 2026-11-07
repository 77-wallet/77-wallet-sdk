use tracing::{info, warn};
use wallet_database::repositories::api_wallet::withdraw::ApiWithdrawRepo;

use crate::{
    domain::chain::adapter::ChainAdapterFactory, error::service::ServiceError,
    response_vo::api_wallet::transaction::ApiWalletResourceConsume,
};

const RECOVERY_PAGE_SIZE: usize = 20;

pub async fn start_withdraw_resource_recover_worker(
    ctx: &'static crate::context::Context,
) -> Result<(), ServiceError> {
    tokio::spawn(async move {
        crate::infrastructure::system_ready::wait_system_ready().await;
        if let Err(error) = recover_withdraw_resources_once(ctx).await {
            warn!(%error, "Withdraw resource startup recovery failed");
        }
    });
    Ok(())
}

async fn recover_withdraw_resources_once(
    ctx: &'static crate::context::Context,
) -> Result<(), ServiceError> {
    let pool = ctx.api_transaction_pool()?;
    let mut after_id: Option<i64> = None;
    let mut total = 0usize;

    loop {
        let candidates =
            ApiWithdrawRepo::scan_withdraw_resource_recovery(&pool, after_id, RECOVERY_PAGE_SIZE)
                .await
                .map_err(|error| ServiceError::Database(error.into()))?;
        if candidates.is_empty() {
            break;
        }

        let last = candidates.last().expect("non-empty recovery page");
        after_id = Some(last.id);
        total += candidates.len();

        for withdraw in candidates {
            if crate::infrastructure::chain_rpc_guard::breaker_open_for_chain_code_with_ctx(
                ctx,
                &withdraw.chain_code,
            )
            .await
            .is_some()
            {
                warn!(trade_no = %withdraw.trade_no, "Skip withdraw resource recovery while chain RPC breaker is open");
                continue;
            }

            let Some(tx_hash) = withdraw.tx_hash.as_deref() else {
                continue;
            };
            let chain = match ChainAdapterFactory::get_tron_adapter_with_ctx(ctx).await {
                Ok(chain) => chain,
                Err(error) => {
                    warn!(trade_no = %withdraw.trade_no, tx_hash = %tx_hash, %error, "Withdraw resource recovery adapter unavailable");
                    continue;
                }
            };
            let _guard = crate::infrastructure::chain_rpc_guard::acquire_if_guarded_with_ctx(
                ctx,
                &withdraw.chain_code,
            )
            .await;
            let tx_info = match chain.get_provider().query_tx_info(tx_hash).await {
                Ok(tx_info) => tx_info,
                Err(error) => {
                    warn!(trade_no = %withdraw.trade_no, tx_hash = %tx_hash, %error, "Withdraw resource recovery receipt query failed");
                    continue;
                }
            };
            let resource = ApiWalletResourceConsume::new(
                tx_info.receipt.net_usage.unwrap_or_default(),
                tx_info
                    .receipt
                    .energy_usage_total
                    .or(tx_info.receipt.energy_usage)
                    .unwrap_or_default(),
            );
            let replacement = resource.to_json_string()?;
            let affected = ApiWithdrawRepo::update_resource_consume_if_unchanged(
                &pool,
                &withdraw.trade_no,
                &withdraw.resource_consume,
                &replacement,
            )
            .await
            .map_err(|error| ServiceError::Database(error.into()))?;
            info!(trade_no = %withdraw.trade_no, affected, "Withdraw resource startup recovery completed");
        }
    }

    info!(total, "Withdraw resource startup recovery pass completed");
    Ok(())
}
