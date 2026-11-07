use crate::{CoreDbPool, dao::wallet::WalletDao, entities::wallet::WalletEntity};
use sqlx::{Sqlite, Transaction};

pub struct WalletRepo;

impl WalletRepo {
    pub async fn detail(
        pool: CoreDbPool,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        let wallet = WalletDao::detail(pool.read_ref(), address).await?;

        Ok(wallet)
    }

    pub async fn wallet_list(pool: CoreDbPool) -> Result<Vec<WalletEntity>, crate::Error> {
        let wallet = WalletDao::list(pool.read_ref()).await?;
        Ok(wallet)
    }

    pub async fn uid_list(pool: CoreDbPool) -> Result<Vec<(String,)>, crate::Error> {
        let wallet = WalletDao::uid_list(pool.read_ref()).await?;
        Ok(wallet)
    }

    pub async fn uid_list_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Vec<(String,)>, crate::Error> {
        let wallet = WalletDao::uid_list(tx.as_mut()).await?;
        Ok(wallet)
    }

    pub async fn upsert_wallet(
        pool: CoreDbPool,
        address: &str,
        uid: &str,
        name: &str,
    ) -> Result<WalletEntity, crate::Error> {
        WalletDao::upsert(pool.write_ref(), address, uid, name, 1).await
    }

    pub async fn detail_all_status(
        pool: CoreDbPool,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::detail_all_status(pool.read_ref(), address).await
    }

    pub async fn detail_all_status_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::detail_all_status(tx.as_mut(), address).await
    }

    pub async fn update_wallet_update_at(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::update_wallet_update_at(pool.write_ref(), wallet_address).await
    }

    pub async fn wallet_init(pool: CoreDbPool, uid: &str) -> Result<WalletEntity, crate::Error> {
        WalletDao::init(pool.write_ref(), uid).await
    }

    pub async fn edit_wallet_name(
        pool: CoreDbPool,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::edit_wallet_name(pool.write_ref(), wallet_address, name).await
    }

    pub async fn wallet_latest(pool: CoreDbPool) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_latest(pool.read_ref()).await
    }

    pub async fn wallet_latest_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_latest(tx.as_mut()).await
    }

    pub async fn wallet_detail_by_name(
        pool: CoreDbPool,
        name: Option<String>,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_detail_by_wallet_name(pool.read_ref(), name).await
    }

    pub async fn wallet_detail_by_address(
        pool: CoreDbPool,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_detail_by_wallet_address(pool.read_ref(), address).await
    }

    pub async fn wallet_detail_by_address_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        address: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_detail_by_wallet_address(tx.as_mut(), address).await
    }

    pub async fn wallet_detail_by_uid(
        pool: CoreDbPool,
        uid: &str,
    ) -> Result<Option<WalletEntity>, crate::Error> {
        WalletDao::wallet_detail_by_uid(pool.read_ref(), uid).await
    }

    pub async fn reset(
        pool: CoreDbPool,
        wallet_address: &str,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::reset_wallet(pool.write_ref(), wallet_address).await
    }

    pub async fn reset_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &str,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::reset_wallet(tx.as_mut(), wallet_address).await
    }

    pub async fn reset_all_wallet(pool: CoreDbPool) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::reset_all_wallet(pool.write_ref()).await
    }

    pub async fn reset_all_wallet_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::reset_all_wallet(tx.as_mut()).await
    }

    pub async fn restart(
        pool: CoreDbPool,
        wallet_addresses: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::restart_wallet(pool.write_ref(), wallet_addresses).await
    }

    pub async fn restart_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_addresses: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::restart_wallet(tx.as_mut(), wallet_addresses).await
    }

    pub async fn physical_delete(
        pool: CoreDbPool,
        wallet_address: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::delete_wallet(pool.write_ref(), wallet_address).await
    }

    pub async fn physical_delete_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        wallet_address: &[&str],
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::delete_wallet(tx.as_mut(), wallet_address).await
    }

    pub async fn physical_delete_all(pool: CoreDbPool) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::delete_all_wallet(pool.write_ref()).await
    }

    pub async fn physical_delete_all_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Vec<WalletEntity>, crate::Error> {
        WalletDao::delete_all_wallet(tx.as_mut()).await
    }
}

#[cfg(test)]
mod tests {
    use super::WalletRepo;
    use crate::repositories::test_helper::setup_core_pool;

    #[tokio::test]
    async fn wallet_upsert_init_and_detail_success() {
        let pool = setup_core_pool("wallet_db_wallet_success").await;
        let wallet_address = "wallet_s_1";
        let uid = "uid_wallet_s_1";

        WalletRepo::upsert_wallet(pool.clone(), wallet_address, uid, "wallet_name").await.unwrap();
        let inited = WalletRepo::wallet_init(pool.clone(), uid).await.unwrap();
        assert_eq!(inited.address, wallet_address);
        assert_eq!(inited.is_init, 1);

        let detail = WalletRepo::detail(pool.clone(), wallet_address).await.unwrap();
        assert!(detail.is_some());
        let list = WalletRepo::wallet_list(pool).await.unwrap();
        assert!(!list.is_empty());
    }

    #[tokio::test]
    async fn wallet_detail_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_wallet_edge").await;
        let detail = WalletRepo::detail(pool, "wallet_not_exist").await.unwrap();
        assert!(detail.is_none());
    }

    #[tokio::test]
    async fn wallet_restart_only_updates_exact_addresses() {
        let pool = setup_core_pool("wallet_db_wallet_restart_exact").await;
        WalletRepo::upsert_wallet(pool.clone(), "wallet_a", "uid_a", "a").await.unwrap();
        WalletRepo::upsert_wallet(pool.clone(), "wallet_b", "uid_b", "b").await.unwrap();
        WalletRepo::reset_all_wallet(pool.clone()).await.unwrap();

        let restarted = WalletRepo::restart(pool.clone(), &["wallet_a"]).await.unwrap();

        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].address, "wallet_a");
        assert!(WalletRepo::detail(pool.clone(), "wallet_a").await.unwrap().is_some());
        assert!(WalletRepo::detail(pool, "wallet_b").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn wallet_restart_treats_sql_shaped_address_as_data() {
        let pool = setup_core_pool("wallet_db_wallet_restart_bind").await;
        WalletRepo::upsert_wallet(pool.clone(), "wallet_a", "uid_a", "a").await.unwrap();
        WalletRepo::upsert_wallet(pool.clone(), "wallet_b", "uid_b", "b").await.unwrap();
        WalletRepo::reset_all_wallet(pool.clone()).await.unwrap();

        let restarted = WalletRepo::restart(pool.clone(), &["wallet_a') OR 1=1 --"]).await.unwrap();

        assert!(restarted.is_empty());
        assert!(WalletRepo::detail(pool.clone(), "wallet_a").await.unwrap().is_none());
        assert!(WalletRepo::detail(pool, "wallet_b").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn wallet_delete_treats_sql_shaped_address_as_data() {
        let pool = setup_core_pool("wallet_db_wallet_delete_bind").await;
        WalletRepo::upsert_wallet(pool.clone(), "wallet_a", "uid_a", "a").await.unwrap();
        WalletRepo::upsert_wallet(pool.clone(), "wallet_b", "uid_b", "b").await.unwrap();

        let deleted =
            WalletRepo::physical_delete(pool.clone(), &["wallet_a') OR 1=1 --"]).await.unwrap();

        assert!(deleted.is_empty());
        assert_eq!(WalletRepo::wallet_list(pool).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn wallet_reset_with_tx_rollback_keeps_active() {
        let pool = setup_core_pool("wallet_db_wallet_rollback").await;
        let wallet_address = "wallet_rb_1";
        WalletRepo::upsert_wallet(pool.clone(), wallet_address, "uid_wallet_rb_1", "wallet_rb")
            .await
            .unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        let reset = WalletRepo::reset_with_executor(&mut tx, wallet_address).await.unwrap();
        assert!(!reset.is_empty());
        tx.rollback().await.unwrap();

        let detail = WalletRepo::detail(pool, wallet_address).await.unwrap();
        assert!(detail.is_some());
    }
}
