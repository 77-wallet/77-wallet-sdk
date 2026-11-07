use crate::{
    ApiWalletDbPool, dao::asset_query_state::AssetQueryStateDao,
    entities::asset_query_state::AssetQueryStateEntity,
};

pub struct AssetQueryStateRepo {}

impl AssetQueryStateRepo {
    pub async fn upsert_pending(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        page: i64,
        index_list_json: &str,
    ) -> Result<(), crate::Error> {
        AssetQueryStateDao::upsert_pending(pool.write_ref(), uid, chain_code, page, index_list_json)
            .await
    }

    pub async fn claim_next(
        pool: &ApiWalletDbPool,
        include_stuck_running: bool,
    ) -> Result<Option<AssetQueryStateEntity>, crate::Error> {
        AssetQueryStateDao::claim_next(pool.write_ref(), include_stuck_running).await
    }

    pub async fn mark_done(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        page: i64,
    ) -> Result<(), crate::Error> {
        AssetQueryStateDao::mark_done(pool.write_ref(), uid, chain_code, page).await
    }

    pub async fn mark_failed(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        page: i64,
        err_msg: &str,
    ) -> Result<(), crate::Error> {
        AssetQueryStateDao::mark_failed(pool.write_ref(), uid, chain_code, page, err_msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::AssetQueryStateRepo;
    use crate::{
        dao::asset_query_state::AssetQueryStateDao,
        repositories::test_helper::setup_api_wallet_pool,
    };

    #[tokio::test]
    async fn asset_query_state_repo_upsert_and_claim_success() {
        let pool = setup_api_wallet_pool("wallet_db_asset_query_state_success").await;
        let uid = "asset_query_uid_s";
        let chain = wallet_types::constant::chain_code::ETHEREUM;

        AssetQueryStateRepo::upsert_pending(&pool, uid, chain, 1, "[1,2]").await.unwrap();
        let claimed = AssetQueryStateRepo::claim_next(&pool, false).await.unwrap();
        assert!(claimed.is_some());
        let task = claimed.unwrap();
        assert_eq!(task.uid, uid);
        assert_eq!(task.chain_code, chain);
        assert_eq!(task.page, 1);
        assert_eq!(task.index_list_json, "[1,2]");
    }

    #[tokio::test]
    async fn asset_query_state_repo_claim_on_empty_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_asset_query_state_edge").await;
        let claimed = AssetQueryStateRepo::claim_next(&pool, false).await.unwrap();
        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn asset_query_state_repo_tx_rollback_keeps_task_claimable() {
        let pool = setup_api_wallet_pool("wallet_db_asset_query_state_rollback").await;
        let uid = "asset_query_uid_rb";
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        let page = 2;

        AssetQueryStateRepo::upsert_pending(&pool, uid, chain, page, "[3,4]").await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        AssetQueryStateDao::mark_done(tx.as_mut(), uid, chain, page).await.unwrap();
        tx.rollback().await.unwrap();

        let claimed = AssetQueryStateRepo::claim_next(&pool, false).await.unwrap();
        assert!(claimed.is_some());
        let task = claimed.unwrap();
        assert_eq!(task.uid, uid);
        assert_eq!(task.chain_code, chain);
        assert_eq!(task.page, page);
        assert_eq!(task.index_list_json, "[3,4]");
    }
}
