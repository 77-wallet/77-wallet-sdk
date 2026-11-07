use sqlx::{Executor, Sqlite};

use crate::entities::expand_notify_state::{
    CreateExpandNotifyStateEntity, ExpandNotifyStateEntity,
};

pub struct ExpandNotifyStateDao;

impl ExpandNotifyStateDao {
    pub async fn get_by_uid_and_chain<'a, E>(
        exec: E,
        uid: &str,
        chain_code: &str,
    ) -> Result<Option<ExpandNotifyStateEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT uid, chain_code, last_notified_page, updated_at
            FROM expand_notify_state
            WHERE uid = ? AND chain_code = ?
        "#;

        sqlx::query_as::<Sqlite, ExpandNotifyStateEntity>(sql)
            .bind(uid)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert_last_notified_page<'a, E>(
        exec: E,
        req: CreateExpandNotifyStateEntity,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            INSERT INTO expand_notify_state (uid, chain_code, last_notified_page, updated_at)
            VALUES (?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            ON CONFLICT(uid, chain_code) DO UPDATE SET
                last_notified_page = excluded.last_notified_page,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        "#;

        sqlx::query(sql)
            .bind(req.uid)
            .bind(req.chain_code)
            .bind(req.last_notified_page)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
            .map(|_| ())
    }
}
