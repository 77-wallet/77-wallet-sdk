// service.rs
use wallet_database::repositories::api_wallet::{
    account::ApiAccountRepo, expand_batch::ExpandBatchRepo, wallet::ApiWalletRepo,
};
use wallet_transport_backend::request::{
    AddressInitReq,
    api_wallet::address::{ApiAddressInitReq, ExpandAddressCompleteReq},
};

use crate::{
    domain::{
        api_wallet::{account::ApiAccountDomain, wallet::ApiWalletDomain},
        app::config::ConfigDomain,
    },
    error::service::ServiceError,
};

pub struct ExpandService;

impl ExpandService {
    pub(crate) async fn create_account_with_ctx(
        ctx: &'static crate::context::Context,
        uid: &str,
        chain: &str,
        to_create: &[i32],
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;
        let wallet = ApiWalletRepo::find_by_uid(&pool, uid).await?.ok_or(
            ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            )),
        )?;

        ApiAccountDomain::create_sub_account_with_ctx(
            ctx,
            &wallet.address,
            uid,
            &unlock_token,
            chain,
            "账户",
            true,
            to_create.len() as u32,
            to_create.to_vec(),
            Some(batch_id.to_string()),
            false,
        )
        .await?;

        Ok(())
    }

    pub(crate) async fn init_account_with_ctx(
        ctx: &'static crate::context::Context,
        uid: &str,
        chain: &str,
        to_init: &[i32],
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        const INIT_CHUNK: usize = 40;

        let sn = ctx.get_sn();
        let pool = ctx.api_wallet_pool()?;
        let api_wallet = ApiWalletRepo::find_by_uid(&pool, uid).await?.ok_or(
            ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            )),
        )?;

        let accounts =
            ApiAccountRepo::list_by_wallet_address(&pool, &api_wallet.address, None, Some(chain))
                .await?;

        // 获取当前 epoch，所有任务共用同一个 epoch
        let current_epoch = ConfigDomain::get_keys_reset_epoch(&ctx).await?;

        // 循环处理每个 chunk
        for chunk in to_init.chunks(INIT_CHUNK) {
            let mut chunk_req = ApiAddressInitReq::new(current_epoch).with_batch_id(batch_id);

            // 为当前 chunk 构建请求
            for account in accounts.iter() {
                if let Ok(map) =
                    wallet_utils::address::AccountIndexMap::from_account_id(account.account_id)
                {
                    let idx = map.input_index;
                    if chunk.contains(&idx) {
                        chunk_req.address_list.add_address(AddressInitReq::new(
                            uid,
                            &account.address,
                            idx,
                            chain,
                            sn,
                            vec!["".to_string()],
                            &account.name,
                        ));
                    }
                }
            }

            // 将当前 chunk 请求推送到 INIT_POOL
            // ⚠️ 注意：这条路径绕过了 TaskQueue / 重启恢复
            // 优点：执行更快，无需等待任务调度
            // 缺点：如果进程重启，未完成的Init请求会丢失
            // 适用于：同步请求场景，调用方会处理重试
            if !chunk_req.address_list.0.is_empty() {
                let req_clone = chunk_req;
                let ctx = ctx;
                crate::infrastructure::expand_init::INIT_POOL
                    .push(async move {
                        crate::infrastructure::expand_init::do_init_with_ctx(ctx, req_clone).await
                    })
                    .await;

                tracing::info!(
                    uid=%uid, chain=%chain, batch_id=%batch_id,
                    chunk_size=chunk.len(),
                    "ExpandService: init_account chunk dispatched to INIT_POOL"
                );
            }
        }

        Ok(())
    }

    pub(crate) async fn expand_complete_with_ctx(
        ctx: &'static crate::context::Context,
        uid: &str,
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let backend = ctx.get_global_backend_api();
        let batch = ExpandBatchRepo::get_batch(&pool, batch_id).await?.ok_or(
            ServiceError::Business(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::account::AccountError::ExpandBatchNotFound
                    .into(),
            )),
        )?;
        backend
            .expand_address_complete(ExpandAddressCompleteReq::new(
                uid,
                batch_id,
                &batch.serial_no,
                true,
                None,
            ))
            .await?;
        Ok(())
    }
}
