use sqlx::{Executor, Sqlite};

use crate::entities::device::{CreateDeviceEntity, DeviceEntity};

pub struct DeviceDao {}

impl DeviceDao {
    pub async fn upsert<'a, E>(
        exec: E,
        req: CreateDeviceEntity,
    ) -> Result<DeviceEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "INSERT INTO device (sn, device_type, code, system_ver, iemi, meid, iccid, mem, app_id, is_init, language_init, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 
                strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            )
            ON CONFLICT (sn) DO UPDATE SET updated_at = excluded.updated_at
            RETURNING *";
        let mut res = sqlx::query_as(sql)
            .bind(req.sn)
            .bind(req.device_type)
            .bind(req.code)
            .bind(req.system_ver)
            .bind(req.iemi)
            .bind(req.meid)
            .bind(req.iccid)
            .bind(req.mem)
            .bind(req.app_id)
            .bind(req.is_init)
            .bind(req.language_init)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    pub async fn init<'a, E>(exec: E, sn: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET is_init = 1 WHERE sn = ?";
        sqlx::query(sql)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_password<'a, E>(
        exec: E,
        sn: &str,
        password: Option<&str>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET password = ? WHERE sn = ?";
        sqlx::query(sql)
            .bind(password)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_password_proof<'a, E>(
        exec: E,
        sn: &str,
        password_proof: Option<&str>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET password_proof = ? WHERE sn = ?";
        sqlx::query(sql)
            .bind(password_proof)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn language_init<'a, E>(exec: E, sn: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET language_init = 1 WHERE sn = ?";
        sqlx::query(sql)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_uid<'a, E>(exec: E, sn: &str, uid: Option<&str>) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET uid = ? WHERE sn = ?";
        sqlx::query(sql)
            .bind(uid)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn update_app_id<'a, E>(exec: E, sn: &str, app_id: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE device SET app_id = ? WHERE sn = ?";
        sqlx::query(sql)
            .bind(app_id)
            .bind(sn)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_device_info<'a, E>(
        exec: E,
        sn: &str,
    ) -> Result<Option<DeviceEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite> + 'a,
    {
        let sql = "SELECT * FROM device WHERE sn = ? LIMIT 1;";

        sqlx::query_as::<sqlx::Sqlite, DeviceEntity>(sql)
            .bind(sn)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
