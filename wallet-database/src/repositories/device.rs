use crate::{CoreDbPool, dao::device::DeviceDao, entities::device::DeviceEntity};
use sqlx::{Sqlite, Transaction};

pub struct DeviceRepo;

impl DeviceRepo {
    pub async fn get_device_info(
        pool: CoreDbPool,
        sn: &str,
    ) -> Result<Option<DeviceEntity>, crate::Error> {
        Ok(DeviceDao::get_device_info(pool.read_ref(), sn).await?)
    }

    pub async fn update_uid(
        pool: CoreDbPool,
        sn: &str,
        uid: Option<&str>,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_uid(pool.write_ref(), sn, uid).await
    }

    pub async fn update_uid_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        sn: &str,
        uid: Option<&str>,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_uid(tx.as_mut(), sn, uid).await
    }

    pub async fn upsert(
        pool: CoreDbPool,
        req: crate::entities::device::CreateDeviceEntity,
    ) -> Result<DeviceEntity, crate::Error> {
        DeviceDao::upsert(pool.write_ref(), req).await
    }

    pub async fn update_password(
        pool: CoreDbPool,
        sn: &str,
        password: Option<&str>,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_password(pool.write_ref(), sn, password).await
    }

    pub async fn update_password_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        sn: &str,
        password: Option<&str>,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_password(tx.as_mut(), sn, password).await
    }

    pub async fn update_password_proof(
        pool: CoreDbPool,
        sn: &str,
        password_proof: Option<&str>,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_password_proof(pool.write_ref(), sn, password_proof).await
    }

    pub async fn update_app_id(
        pool: CoreDbPool,
        sn: &str,
        app_id: &str,
    ) -> Result<(), crate::Error> {
        DeviceDao::update_app_id(pool.write_ref(), sn, app_id).await
    }

    pub async fn device_init(pool: CoreDbPool, sn: &str) -> Result<(), crate::Error> {
        DeviceDao::init(pool.write_ref(), sn).await
    }

    pub async fn language_init(pool: CoreDbPool, sn: &str) -> Result<(), crate::Error> {
        DeviceDao::language_init(pool.write_ref(), sn).await
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceRepo;
    use crate::{entities::device::CreateDeviceEntity, repositories::test_helper::setup_core_pool};

    fn build_create_device(sn: &str) -> CreateDeviceEntity {
        CreateDeviceEntity {
            device_type: "cold".to_string(),
            sn: sn.to_string(),
            code: "code_1".to_string(),
            system_ver: "1.0.0".to_string(),
            iemi: Some("iemi".to_string()),
            meid: None,
            iccid: None,
            mem: None,
            app_id: Some("app_v1".to_string()),
            is_init: 0,
            language_init: 0,
        }
    }

    #[tokio::test]
    async fn device_repo_upsert_and_update_visible() {
        let pool = setup_core_pool("wallet_db_device_success").await;
        let sn = "device_sn_success";

        let inserted = DeviceRepo::upsert(pool.clone(), build_create_device(sn)).await.unwrap();
        assert_eq!(inserted.sn, sn);
        assert_eq!(inserted.uid, None);

        DeviceRepo::update_uid(pool.clone(), sn, Some("uid_1")).await.unwrap();
        let updated = DeviceRepo::get_device_info(pool, sn).await.unwrap().unwrap();
        assert_eq!(updated.uid.as_deref(), Some("uid_1"));
    }

    #[tokio::test]
    async fn device_repo_missing_sn_returns_none() {
        let pool = setup_core_pool("wallet_db_device_edge").await;
        let found = DeviceRepo::get_device_info(pool, "device_sn_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn device_repo_tx_rollback_keeps_uid_unchanged() {
        let pool = setup_core_pool("wallet_db_device_rollback").await;
        let sn = "device_sn_rollback";
        DeviceRepo::upsert(pool.clone(), build_create_device(sn)).await.unwrap();
        DeviceRepo::update_uid(pool.clone(), sn, Some("uid_before")).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        DeviceRepo::update_uid_with_executor(&mut tx, sn, Some("uid_tx")).await.unwrap();
        tx.rollback().await.unwrap();

        let after = DeviceRepo::get_device_info(pool, sn).await.unwrap().unwrap();
        assert_eq!(after.uid.as_deref(), Some("uid_before"));
    }
}
