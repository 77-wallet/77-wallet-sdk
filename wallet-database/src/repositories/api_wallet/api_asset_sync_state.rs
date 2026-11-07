use crate::{
    ApiWalletDbPool,
    dao::api_asset_sync_state::ApiAssetSyncStateDao,
    entities::{api_asset_sync_state::ApiAssetSyncStateEntity, asset_token_key::AssetTokenKey},
};

pub struct ApiAssetSyncStateRepo;

impl ApiAssetSyncStateRepo {
    pub async fn upsert_pending(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
    ) -> Result<(), crate::Error> {
        ApiAssetSyncStateDao::upsert_pending(pool.write_ref(), address, chain_code, token_address)
            .await
    }

    pub async fn claim_next(
        pool: &ApiWalletDbPool,
        include_stuck_running: bool,
    ) -> Result<Option<ApiAssetSyncStateEntity>, crate::Error> {
        ApiAssetSyncStateDao::claim_next(pool.write_ref(), include_stuck_running).await
    }

    pub async fn mark_done(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
    ) -> Result<(), crate::Error> {
        ApiAssetSyncStateDao::mark_done(pool.write_ref(), address, chain_code, token_address).await
    }

    pub async fn mark_failed(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        token_address: AssetTokenKey,
        err_msg: &str,
    ) -> Result<(), crate::Error> {
        ApiAssetSyncStateDao::mark_failed(
            pool.write_ref(),
            address,
            chain_code,
            token_address,
            err_msg,
        )
        .await
    }

    pub async fn delete_done_older_than(
        pool: &ApiWalletDbPool,
        retention_secs: u64,
    ) -> Result<u64, crate::Error> {
        ApiAssetSyncStateDao::delete_done_older_than(pool.write_ref(), retention_secs).await
    }

    pub async fn trim_done_to_limit(
        pool: &ApiWalletDbPool,
        max_rows: u32,
    ) -> Result<u64, crate::Error> {
        ApiAssetSyncStateDao::trim_done_to_limit(pool.write_ref(), max_rows).await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiAssetSyncStateRepo;
    use crate::{
        entities::asset_token_key::AssetTokenKey, repositories::test_helper::setup_api_wallet_pool,
    };

    #[tokio::test]
    async fn api_asset_sync_state_upsert_and_claim_success() {
        let pool = setup_api_wallet_pool("wallet_db_api_asset_sync_state_success").await;
        let address = "TMgMcdSzdBJgGV3quNCdeBRjBFSZ2jf5eY";
        let chain = wallet_types::constant::chain_code::TRON;
        let token = AssetTokenKey::from_raw(Some("TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3"));

        ApiAssetSyncStateRepo::upsert_pending(&pool, address, chain, token.clone()).await.unwrap();

        let claimed = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap().unwrap();
        assert_eq!(claimed.address, address);
        assert_eq!(claimed.chain_code, chain);
        assert_eq!(claimed.token_address, token);
    }

    #[tokio::test]
    async fn api_asset_sync_state_claim_is_exclusive() {
        let pool = setup_api_wallet_pool("wallet_db_api_asset_sync_state_claim").await;
        let address = "TMgMcdSzdBJgGV3quNCdeBRjBFSZ2jf5eY";
        let chain = wallet_types::constant::chain_code::TRON;
        let token = AssetTokenKey::from_raw(Some("TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3"));

        ApiAssetSyncStateRepo::upsert_pending(&pool, address, chain, token).await.unwrap();

        let first = ApiAssetSyncStateRepo::claim_next(&pool, true).await.unwrap();
        let second = ApiAssetSyncStateRepo::claim_next(&pool, true).await.unwrap();

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn api_asset_sync_state_requeues_done_task() {
        let pool = setup_api_wallet_pool("wallet_db_api_asset_sync_state_requeue").await;
        let address = "TMgMcdSzdBJgGV3quNCdeBRjBFSZ2jf5eY";
        let chain = wallet_types::constant::chain_code::TRON;
        let token = AssetTokenKey::from_raw(Some("TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3"));

        ApiAssetSyncStateRepo::upsert_pending(&pool, address, chain, token.clone()).await.unwrap();
        let claimed = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap().unwrap();
        ApiAssetSyncStateRepo::mark_done(
            &pool,
            &claimed.address,
            &claimed.chain_code,
            claimed.token_address.clone(),
        )
        .await
        .unwrap();

        ApiAssetSyncStateRepo::upsert_pending(&pool, address, chain, token.clone()).await.unwrap();

        let requeued = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap().unwrap();
        assert_eq!(requeued.address, address);
        assert_eq!(requeued.chain_code, chain);
        assert_eq!(requeued.token_address, token);
    }

    #[tokio::test]
    async fn api_asset_sync_state_done_is_not_overwritten_by_failed_worker() {
        let pool = setup_api_wallet_pool("wallet_db_api_asset_sync_state_done_wins").await;
        let address = "TMgMcdSzdBJgGV3quNCdeBRjBFSZ2jf5eY";
        let chain = wallet_types::constant::chain_code::TRON;
        let token = AssetTokenKey::from_raw(Some("TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3"));

        ApiAssetSyncStateRepo::upsert_pending(&pool, address, chain, token.clone()).await.unwrap();
        let claimed = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap().unwrap();
        ApiAssetSyncStateRepo::mark_done(
            &pool,
            &claimed.address,
            &claimed.chain_code,
            claimed.token_address.clone(),
        )
        .await
        .unwrap();

        ApiAssetSyncStateRepo::mark_failed(
            &pool,
            &claimed.address,
            &claimed.chain_code,
            claimed.token_address,
            "late worker failure",
        )
        .await
        .unwrap();

        let next = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap();
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn api_asset_sync_state_trims_done_rows_to_limit() {
        let pool = setup_api_wallet_pool("wallet_db_api_asset_sync_state_trim_done").await;
        let chain = wallet_types::constant::chain_code::TRON;

        for index in 0..3 {
            let address = format!("TTrimDoneAddress{index}");
            let token_raw = format!("TToken{index}");
            let token = AssetTokenKey::from_raw(Some(token_raw.as_str()));
            ApiAssetSyncStateRepo::upsert_pending(&pool, &address, chain, token.clone())
                .await
                .unwrap();
            ApiAssetSyncStateRepo::mark_done(&pool, &address, chain, token).await.unwrap();
        }

        let removed = ApiAssetSyncStateRepo::trim_done_to_limit(&pool, 1).await.unwrap();
        assert_eq!(removed, 2);

        let first = ApiAssetSyncStateRepo::claim_next(&pool, false).await.unwrap();
        assert!(first.is_none());
    }
}
