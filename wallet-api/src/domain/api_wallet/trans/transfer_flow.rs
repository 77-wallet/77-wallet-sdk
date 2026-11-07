use super::ApiTransDomain;
use crate::{
    context::Context,
    domain::{
        api_wallet::{
            account::ApiAccountDomain,
            adapter::tx::{RawTx, Tx},
            adapter_factory::ApiChainAdapterFactory,
        },
        chain::TransferResp,
    },
    error::service::ServiceError,
    request::api_wallet::trans::ApiTransferReq,
};
use std::{sync::Arc, time::Instant};
use wallet_chain_interact::types::ChainPrivateKey;

struct PreparedTransferExecution {
    chain_code: String,
    private_key: ChainPrivateKey,
    adapter: Arc<dyn Tx + Send + Sync>,
}

impl ApiTransDomain {
    async fn prepare_transfer_execution(
        ctx: &'static Context,
        params: &ApiTransferReq,
        preloaded_private_key: Option<ChainPrivateKey>,
    ) -> Result<PreparedTransferExecution, ServiceError> {
        tracing::info!("transfer: 获取私钥");
        let private_key_time = Instant::now();
        let private_key = match preloaded_private_key {
            Some(pk) => pk,
            None => {
                ApiAccountDomain::get_private_key_with_ctx(
                    ctx,
                    &params.base.from,
                    &params.base.chain_code,
                )
                .await?
            }
        };
        tracing::info!("transfer: 获取私钥完成, 耗时: {:?}", private_key_time.elapsed());

        tracing::info!("transfer: 原始链代码: {}", params.base.chain_code);
        let chain_code_time = Instant::now();
        let chain_code = params.base.chain_code.clone();
        tracing::info!(
            "transfer: 转换后链代码: {}, 耗时: {:?}",
            chain_code,
            chain_code_time.elapsed()
        );

        let adapter_time = Instant::now();
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(ctx, &chain_code).await?;
        tracing::info!("transfer (适配器创建): 完成, 耗时: {:?}", adapter_time.elapsed());

        Ok(PreparedTransferExecution { chain_code, private_key, adapter })
    }

    // transfer（显式上下文签名）
    pub async fn transfer(
        ctx: &'static Context,
        params: ApiTransferReq,
        preloaded_private_key: Option<ChainPrivateKey>,
    ) -> Result<TransferResp, ServiceError> {
        Self::transfer_with_ctx(ctx, params, preloaded_private_key).await
    }

    /// transfer（显式上下文签名）
    pub async fn transfer_with_ctx(
        ctx: &'static Context,
        params: ApiTransferReq,
        preloaded_private_key: Option<ChainPrivateKey>,
    ) -> Result<TransferResp, ServiceError> {
        let start_time = Instant::now();
        tracing::info!(
            "transfer (开始): 请求ID: {:?}, 链: {}, 时间: {:?}",
            params.base.request_resource_id,
            params.base.chain_code,
            start_time
        );

        let prepared =
            Self::prepare_transfer_execution(ctx, &params, preloaded_private_key).await?;

        tracing::info!("transfer: 执行转账");
        let transfer_time = Instant::now();
        let resp = prepared.adapter.transfer_with_ctx(ctx, &params, prepared.private_key).await?;
        tracing::info!("transfer: 转账操作完成, 耗时: {:?}", transfer_time.elapsed());

        if let Some(request_id) = params.base.request_resource_id {
            tracing::info!("transfer (委托完成): 开始, request_id: {}", request_id);
            let delegate_time = Instant::now();
            let backend = ctx.get_global_backend_api();
            let _ = backend.delegate_complete(&request_id).await;
            tracing::info!("transfer (委托完成): 结束, 耗时: {:?}", delegate_time.elapsed());
        }

        tracing::info!("transfer (结束): 总耗时: {:?}", start_time.elapsed());
        Ok(resp)
    }

    /// 构建原始交易
    ///
    /// 职责：
    /// - 只负责构建交易，生成raw_tx和tx_hash
    /// - 不处理Recover逻辑
    /// - 不负责从链上恢复交易状态
    pub async fn build_transfer_raw(
        ctx: &'static Context,
        params: ApiTransferReq,
        preloaded_private_key: Option<ChainPrivateKey>,
    ) -> Result<(String, RawTx, String), ServiceError> {
        Self::build_transfer_raw_with_ctx(ctx, params, preloaded_private_key).await
    }

    pub async fn build_transfer_raw_with_ctx(
        ctx: &'static Context,
        params: ApiTransferReq,
        preloaded_private_key: Option<ChainPrivateKey>,
    ) -> Result<(String, RawTx, String), ServiceError> {
        let start_time = Instant::now();
        tracing::info!(
            "transfer (开始): 请求ID: {:?}, 链: {}, 时间: {:?}",
            params.base.request_resource_id,
            params.base.chain_code,
            start_time
        );

        let prepared =
            Self::prepare_transfer_execution(ctx, &params, preloaded_private_key).await?;

        tracing::info!("transfer: 执行转账");
        let transfer_time = Instant::now();
        tracing::info!(
            chain_code = %params.base.chain_code,
            from = %params.base.from,
            to = %params.base.to,
            value = %params.base.value,
            token_address = ?params.base.token_address,
            request_resource_id = ?params.base.request_resource_id,
            nonce = params.nonce,
            "transfer: 构建原始交易"
        );
        let resp = prepared.adapter.build_transfer_raw(&params, prepared.private_key).await?;
        tracing::info!("transfer: 转账操作完成, 耗时: {:?}", transfer_time.elapsed());
        tracing::info!(
            chain_code = %prepared.chain_code,
            request_resource_id = ?params.base.request_resource_id,
            "transfer: 构建原始交易完成"
        );

        Ok(resp)
    }
}
