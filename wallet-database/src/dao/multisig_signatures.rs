use crate::entities::multisig_signatures::{
    MultisigSignatureEntities, MultisigSignatureEntity, NewSignatureEntity,
};
use sqlx::{Executor, Pool, Sqlite};
use std::sync::Arc;

pub struct MultisigSignatureDaoV1;

impl MultisigSignatureDaoV1 {
    pub async fn create_or_update(
        params: &NewSignatureEntity,
        pool: crate::DbPool,
    ) -> Result<(), crate::DatabaseError> {
        let queue = MultisigSignatureDaoV1::find_by_address_and_queue_id(
            &params.queue_id,
            &params.address,
            pool.as_ref(),
        )
        .await?;

        match queue {
            Some(_) => MultisigSignatureDaoV1::update_status(params, pool.as_ref()).await?,
            None => MultisigSignatureDaoV1::create_signature(params, pool.as_ref()).await?,
        };
        Ok(())
    }

    pub async fn create_signature<'a, E>(
        params: &NewSignatureEntity,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"INSERT INTO multisig_signatures 
        (queue_id, address, signature, status, is_del, created_at) 
        VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        ON CONFLICT(id) DO UPDATE SET
            is_del = EXCLUDED.is_del,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')"#;

        let _ = sqlx::query(sql)
            .bind(&params.queue_id)
            .bind(&params.address)
            .bind(&params.signature)
            .bind(params.status.to_i8())
            .bind(0)
            .execute(exec)
            .await?;

        Ok(())
    }
    pub async fn update_status<'a, E>(
        params: &NewSignatureEntity,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query(
            r#"
                    UPDATE multisig_signatures
                    SET status = ?, signature = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                    WHERE queue_id = ? AND address = ?
                "#,
        )
        .bind(params.status.to_i8())
        .bind(&params.signature)
        .bind(&params.queue_id)
        .bind(&params.address)
        .execute(exec)
        .await?;

        Ok(())
    }

    // 某一个地址是否进行了签名
    pub async fn find_by_address_and_queue_id<'a, E>(
        queue_id: &str,
        address: &str,
        pool: E,
    ) -> Result<Option<MultisigSignatureEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_signatures where queue_id = ? and address = ?"#;
        let res = sqlx::query_as::<_, MultisigSignatureEntity>(sql)
            .bind(queue_id)
            .bind(address)
            .fetch_optional(pool)
            .await?;
        Ok(res)
    }

    pub async fn find_by_queue_id(
        queue_id: &str,
        pool: Arc<Pool<Sqlite>>,
    ) -> Result<Vec<MultisigSignatureEntity>, crate::DatabaseError> {
        let sql = r#"select * from multisig_signatures where queue_id = ?"#;
        let res = sqlx::query_as::<_, MultisigSignatureEntity>(sql)
            .bind(queue_id)
            .fetch_all(&*pool)
            .await?;
        Ok(res)
    }

    pub async fn get_signed_count<'a, E>(
        queue_id: &str,
        pool: E,
    ) -> Result<i64, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"SELECT COUNT(*) FROM multisig_signatures WHERE queue_id = ? AND status = 1"#;
        let count: (i64,) = sqlx::query_as(sql).bind(queue_id).fetch_one(pool).await?;
        Ok(count.0)
    }

    // 获取已签名的数据
    pub async fn get_signed_list<'a, E>(
        queue_id: &str,
        pool: E,
    ) -> Result<MultisigSignatureEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_signatures where queue_id = ? and status = 1"#;

        let res = sqlx::query_as::<_, MultisigSignatureEntity>(sql)
            .bind(queue_id)
            .fetch_all(pool)
            .await?;

        Ok(MultisigSignatureEntities(res))
    }

    pub async fn logic_del_multisig_signatures<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE multisig_signatures SET is_del = 1 WHERE account_id = $1".to_string();
        sqlx::query(&sql).bind(id).execute(exec).await?;

        Ok(())
    }

    pub async fn logic_del_multi_multisig_signatures<'a, E>(
        ids: Vec<String>,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let ids = crate::any_in_collection(ids, "','");

        let sql = format!(
            "UPDATE multisig_signatures SET is_del = 1 WHERE queue_id IN ('{}')",
            ids
        );

        let query = sqlx::query(&sql);

        query.execute(exec).await?;

        Ok(())
    }

    pub async fn physical_del_multi_multisig_signatures<'a, E>(
        exec: E,
        ids: Vec<String>,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if ids.is_empty() {
            "DELETE FROM multisig_signatures RETURNING *".to_string()
        } else {
            let addresses = crate::any_in_collection(ids, "','");
            format!(
                r#"
                DELETE FROM multisig_signatures
                WHERE queue_id IN ('{}')
                RETURNING *
                "#,
                addresses
            )
        };
        sqlx::query(&sql).execute(exec).await?;

        Ok(())
    }
}
