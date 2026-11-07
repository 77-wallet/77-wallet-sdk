use crate::{
    ApiWalletDbPool,
    dao::expand_notify_state::ExpandNotifyStateDao,
    entities::expand_notify_state::{CreateExpandNotifyStateEntity, ExpandNotifyStateEntity},
};

pub struct ExpandNotifyStateRepo;

impl ExpandNotifyStateRepo {
    pub async fn get_by_uid_and_chain(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Option<ExpandNotifyStateEntity>, crate::Error> {
        ExpandNotifyStateDao::get_by_uid_and_chain(pool.read_ref(), uid, chain_code).await
    }

    pub async fn update_last_notified_page(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        last_notified_page: i64,
    ) -> Result<(), crate::Error> {
        let req = CreateExpandNotifyStateEntity::new(uid, chain_code, last_notified_page);
        ExpandNotifyStateDao::upsert_last_notified_page(pool.write_ref(), req).await
    }
}

#[cfg(test)]
mod tests {
    use super::ExpandNotifyStateRepo;
    use crate::{
        dao::expand_notify_state::ExpandNotifyStateDao,
        entities::expand_notify_state::CreateExpandNotifyStateEntity,
        repositories::test_helper::setup_api_wallet_pool,
    };

    #[tokio::test]
    async fn expand_notify_state_repo_upsert_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_expand_notify_success").await;
        let uid = "expand_notify_uid_s";
        let chain = wallet_types::constant::chain_code::ETHEREUM;

        ExpandNotifyStateRepo::update_last_notified_page(&pool, uid, chain, 5).await.unwrap();
        let got = ExpandNotifyStateRepo::get_by_uid_and_chain(&pool, uid, chain).await.unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.uid, uid);
        assert_eq!(got.chain_code, chain);
        assert_eq!(got.last_notified_page, 5);

        ExpandNotifyStateRepo::update_last_notified_page(&pool, uid, chain, 6).await.unwrap();
        let got = ExpandNotifyStateRepo::get_by_uid_and_chain(&pool, uid, chain).await.unwrap();
        let got = got.unwrap();
        assert_eq!(got.uid, uid);
        assert_eq!(got.chain_code, chain);
        assert_eq!(got.last_notified_page, 6);
    }

    #[tokio::test]
    async fn expand_notify_state_repo_missing_key_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_expand_notify_edge").await;
        let got = ExpandNotifyStateRepo::get_by_uid_and_chain(
            &pool,
            "expand_notify_uid_missing",
            wallet_types::constant::chain_code::ETHEREUM,
        )
        .await
        .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn expand_notify_state_repo_tx_rollback_keeps_page_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_expand_notify_rollback").await;
        let uid = "expand_notify_uid_rb";
        let chain = wallet_types::constant::chain_code::ETHEREUM;

        ExpandNotifyStateRepo::update_last_notified_page(&pool, uid, chain, 3).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        let req = CreateExpandNotifyStateEntity::new(uid, chain, 99);
        ExpandNotifyStateDao::upsert_last_notified_page(tx.as_mut(), req).await.unwrap();
        tx.rollback().await.unwrap();

        let got = ExpandNotifyStateRepo::get_by_uid_and_chain(&pool, uid, chain).await.unwrap();
        let got = got.unwrap();
        assert_eq!(got.uid, uid);
        assert_eq!(got.chain_code, chain);
        assert_eq!(got.last_notified_page, 3);
    }
}
