use crate::{
    ApiWalletDbPool,
    dao::address_query_state::AddressQueryStateDao,
    entities::address_query_state::{
        AddressQueryStateEntity, AddressQueryStatus, CreateAddressQueryStateEntity,
    },
};

pub struct AddressQueryStateRepo {}

impl AddressQueryStateRepo {
    pub fn build_create_state(
        uid: &str,
        chain_code: &str,
        status: AddressQueryStatus,
    ) -> CreateAddressQueryStateEntity {
        CreateAddressQueryStateEntity::new(uid, chain_code, status)
    }

    pub async fn upsert(
        pool: &ApiWalletDbPool,
        req: CreateAddressQueryStateEntity,
    ) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::upsert(pool.write_ref(), req).await?)
    }

    pub async fn get_by_uid_and_chain(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<Option<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::get_by_uid_and_chain(pool.read_ref(), uid, chain_code).await?)
    }

    pub async fn update_status(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        status: AddressQueryStatus,
    ) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::update_status(pool.write_ref(), uid, chain_code, status).await?)
    }

    pub async fn delete(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::delete(pool.write_ref(), uid, chain_code).await?)
    }

    pub async fn delete_by_uid(pool: &ApiWalletDbPool, uid: &str) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::delete_by_uid(pool.write_ref(), uid).await?)
    }

    /// 删除所有记录
    pub async fn delete_all(pool: &ApiWalletDbPool) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::delete_all(pool.write_ref()).await?)
    }

    pub async fn list_by_uid(
        pool: &ApiWalletDbPool,
        uid: &str,
    ) -> Result<Vec<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::list_by_uid(pool.read_ref(), uid).await?)
    }

    pub async fn list_by_status(
        pool: &ApiWalletDbPool,
        status: AddressQueryStatus,
    ) -> Result<Vec<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::list_by_status(pool.read_ref(), status).await?)
    }

    /// 获取需要恢复的任务（Failed + 长时间未更新的Running）
    /// 长时间指：updated_at < now - 10 minutes
    pub async fn list_recoverable_tasks(
        pool: &ApiWalletDbPool,
        include_stuck_running: bool,
    ) -> Result<Vec<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::list_recoverable_tasks(pool.read_ref(), include_stuck_running)
            .await?)
    }

    pub async fn list_running_by_uid(
        pool: &ApiWalletDbPool,
        uid: &str,
    ) -> Result<Vec<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::list_running_by_uid(pool.read_ref(), uid).await?)
    }

    pub async fn is_running(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
    ) -> Result<bool, crate::Error> {
        Ok(AddressQueryStateDao::is_running(pool.read_ref(), uid, chain_code).await?)
    }

    pub async fn count_by_status(
        pool: &ApiWalletDbPool,
        status: AddressQueryStatus,
    ) -> Result<i64, crate::Error> {
        Ok(AddressQueryStateDao::count_by_status(pool.read_ref(), status).await?)
    }

    pub async fn get_all(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<AddressQueryStateEntity>, crate::Error> {
        Ok(AddressQueryStateDao::get_all(pool.read_ref()).await?)
    }

    /// 更新最后处理的页码
    pub async fn update_last_page(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        last_page: i64,
    ) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::update_last_page(pool.write_ref(), uid, chain_code, last_page)
            .await?)
    }

    /// 更新总远程地址数
    pub async fn update_total_remote(
        pool: &ApiWalletDbPool,
        uid: &str,
        chain_code: &str,
        total_remote: i64,
    ) -> Result<(), crate::Error> {
        Ok(AddressQueryStateDao::update_total_remote(
            pool.write_ref(),
            uid,
            chain_code,
            total_remote,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::AddressQueryStateRepo;
    use crate::{
        dao::address_query_state::AddressQueryStateDao,
        entities::address_query_state::{AddressQueryStatus, CreateAddressQueryStateEntity},
        repositories::test_helper::setup_api_wallet_pool,
    };

    #[test]
    fn address_query_state_repo_build_create_state_sets_defaults() {
        let entity =
            AddressQueryStateRepo::build_create_state("uid-1", "tron", AddressQueryStatus::Running);
        assert_eq!(entity.uid, "uid-1");
        assert_eq!(entity.chain_code, "tron");
        assert_eq!(entity.status, AddressQueryStatus::Running);
        assert_eq!(entity.last_page, -1);
        assert_eq!(entity.total_remote, 0);
    }

    #[test]
    fn address_query_state_repo_build_create_state_supports_field_overrides() {
        let entity =
            AddressQueryStateRepo::build_create_state("uid-2", "btc", AddressQueryStatus::Failed)
                .with_last_page(7)
                .with_total_remote(99);

        assert_eq!(entity.uid, "uid-2");
        assert_eq!(entity.chain_code, "btc");
        assert_eq!(entity.status, AddressQueryStatus::Failed);
        assert_eq!(entity.last_page, 7);
        assert_eq!(entity.total_remote, 99);
    }

    #[tokio::test]
    async fn address_query_state_repo_upsert_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_address_query_state_repo_success").await;
        let req = AddressQueryStateRepo::build_create_state(
            "uid_success",
            "tron",
            AddressQueryStatus::Running,
        )
        .with_last_page(3)
        .with_total_remote(12);
        AddressQueryStateRepo::upsert(&pool, req).await.unwrap();

        let found = AddressQueryStateRepo::get_by_uid_and_chain(&pool, "uid_success", "tron")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.uid, "uid_success");
        assert_eq!(found.chain_code, "tron");
        assert_eq!(found.status, AddressQueryStatus::Running);
        assert_eq!(found.last_page, 3);
        assert_eq!(found.total_remote, 12);
    }

    #[tokio::test]
    async fn address_query_state_repo_missing_record_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_address_query_state_repo_edge").await;
        let found = AddressQueryStateRepo::get_by_uid_and_chain(&pool, "uid_missing", "tron")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn address_query_state_repo_tx_rollback_keeps_record_absent() {
        let pool = setup_api_wallet_pool("wallet_db_address_query_state_repo_rollback").await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        AddressQueryStateDao::upsert(
            tx.as_mut(),
            CreateAddressQueryStateEntity::new("uid_rb", "tron", AddressQueryStatus::Running),
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let found =
            AddressQueryStateRepo::get_by_uid_and_chain(&pool, "uid_rb", "tron").await.unwrap();
        assert!(found.is_none());

        let req =
            AddressQueryStateRepo::build_create_state("uid_rb", "tron", AddressQueryStatus::Failed)
                .with_last_page(8)
                .with_total_remote(101);
        AddressQueryStateRepo::upsert(&pool, req).await.unwrap();

        let found = AddressQueryStateRepo::get_by_uid_and_chain(&pool, "uid_rb", "tron")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, AddressQueryStatus::Failed);
        assert_eq!(found.last_page, 8);
        assert_eq!(found.total_remote, 101);
    }
}
