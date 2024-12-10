use crate::{
    entities::multisig_queue::{
        fail_reason::EXPIRED, MultisigQueueEntity, MultisigQueueStatus,
        MultisigQueueWithAccountEntity, NewMultisigQueueEntity,
    },
    pagination::Pagination,
};
use chrono::SecondsFormat;
use sqlx::{Executor, Pool, Sqlite};
use std::sync::Arc;

pub struct MultisigQueueDaoV1;

impl MultisigQueueDaoV1 {
    pub async fn create_queue<'a, E>(
        params: &NewMultisigQueueEntity,
        exec: E,
    ) -> Result<MultisigQueueEntity, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"Insert into multisig_queue 
            (id, from_addr, to_addr, value,expiration, symbol, chain_code, token_addr, msg_hash,
             tx_hash, raw_data, status,notes,fail_reason,created_at,account_id)
                values (?, ?, ?, ?, ?, ?, ?,?,?, ?, ?, ?, ?,?,?,?)
                on conflict (id)
                do update set
                    is_del = excluded.is_del,
                    updated_at = excluded.updated_at
                returning *"#;

        let mut rec = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(&params.id)
            .bind(&params.from_addr)
            .bind(&params.to_addr)
            .bind(&params.value)
            .bind(params.expiration)
            .bind(&params.symbol)
            .bind(&params.chain_code)
            .bind(&params.token_addr)
            .bind(&params.msg_hash)
            .bind(&params.tx_hash)
            .bind(&params.raw_data)
            .bind(params.status.to_i8())
            .bind(&params.notes)
            .bind(&params.fail_reason)
            .bind(params.create_at.to_rfc3339_opts(SecondsFormat::Secs, true))
            .bind(&params.account_id)
            .bind(0)
            .fetch_all(exec)
            .await?;

        rec.pop().ok_or(crate::DatabaseError::ReturningNone)
    }

    pub async fn find_by_id<'a, E>(
        id: &str,
        pool: E,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_queue where id = ?"#;
        let res = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(res)
    }

    pub async fn find_by_id_with_account<'a, E>(
        id: &str,
        pool: E,
    ) -> Result<Option<MultisigQueueWithAccountEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                select q.*,a.name,a.threshold,a.member_num,a.owner,a.initiator_addr
                from multisig_queue as q  
                join multisig_account a on q.account_id = a.id
                where q.id = ?"#;
        let res = sqlx::query_as::<_, MultisigQueueWithAccountEntity>(sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(res)
    }

    // 0 待签名 1已签名 2待执行，确认中，4成功 5失败;待执行需要查询2中状态
    pub async fn queue_list(
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
        pool: Arc<Pool<Sqlite>>,
    ) -> Result<Pagination<MultisigQueueWithAccountEntity>, crate::DatabaseError> {
        let mut sql = "SELECT q.*, a.name, a.threshold, a.member_num, a.owner,a.initiator_addr 
                   FROM multisig_queue AS q 
                   JOIN multisig_account AS a ON q.account_id = a.id"
            .to_string();
        let mut conditions = Vec::new();

        if status == 2 {
            conditions.push(format!(
                "q.status in ({},{}) ",
                MultisigQueueStatus::PendingExecution.to_i8(),
                MultisigQueueStatus::InConfirmation.to_i8()
            ));
        } else {
            conditions.push(format!("q.status = {}", status));
        }

        if let Some(from_addr) = from {
            conditions.push(format!("q.from_addr = '{}'", from_addr));
        }

        if let Some(chain) = chain_code {
            conditions.push(format!("q.chain_code = '{}'", chain));
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY q.created_at DESC");
        let pagination = Pagination::<MultisigQueueWithAccountEntity>::init(page, page_size);

        pagination.page(&*pool, &sql).await
    }

    pub async fn list_by_account_ids<'a, E>(
        account_ids: &[String],
        pool: E,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let account_ids = crate::any_in_collection(account_ids, "','");

        let sql = format!(
            "select * from multisig_queue where account_id in ('{}') order by created_at desc limit 1",
            account_ids
        );

        let res = sqlx::query_as::<_, MultisigQueueEntity>(&sql)
            .bind(account_ids)
            .fetch_optional(pool)
            .await?;

        Ok(res)
    }

    pub async fn update_expired_queue<'a, E>(exec: E) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ?,fail_reason = ? where status in (?,?,?) and expiration < ?"#;

        let time = sqlx::types::chrono::Utc::now().timestamp();

        let _res = sqlx::query(sql)
            .bind(MultisigQueueStatus::Fail.to_i8())
            .bind(EXPIRED)
            .bind(MultisigQueueStatus::PendingSignature.to_i8())
            .bind(MultisigQueueStatus::HasSignature.to_i8())
            .bind(MultisigQueueStatus::PendingExecution.to_i8())
            .bind(time)
            .execute(exec)
            .await?;
        Ok(())
    }

    pub async fn update_status<'a, E>(
        id: &str,
        status: MultisigQueueStatus,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ? where id = ?"#;

        let _res = sqlx::query(sql)
            .bind(status.to_i8())
            .bind(id)
            .execute(exec)
            .await?;

        Ok(())
    }

    pub async fn update_fail<'a, E>(
        id: &str,
        reason: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ?,fail_reason = ? where id = ?"#;

        sqlx::query(sql)
            .bind(MultisigQueueStatus::Fail.to_i8())
            .bind(reason)
            .bind(id)
            .execute(exec)
            .await?;

        Ok(())
    }

    pub async fn rollback_update_fail<'a, E>(
        id: &str,
        status: i8,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ?,fail_reason = '' where id = ?"#;

        let _res = sqlx::query(sql).bind(status).bind(id).execute(exec).await?;

        Ok(())
    }

    pub async fn update_status_and_tx_hash<'a, E>(
        id: &str,
        status: MultisigQueueStatus,
        tx_hash: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ?,tx_hash = ? where id = ?"#;

        let _res = sqlx::query(sql)
            .bind(status.to_i8())
            .bind(tx_hash)
            .bind(id)
            .execute(exec)
            .await?;

        Ok(())
    }

    pub async fn get_latest<'a, E>(
        exec: E,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_queue order by created_at desc limit 1"#;

        let row = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .fetch_optional(exec)
            .await?;

        Ok(row)
    }

    pub async fn logic_del_multisig_queue<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE multisig_queue SET is_del = 1 WHERE account_id = $1 RETURNING *";
        let rows = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(id)
            .fetch_all(exec)
            .await?;

        Ok(rows)
    }

    pub async fn physical_del_multisig_queue<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM multisig_queue WHERE account_id = $1 RETURNING *";
        let rows = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(id)
            .fetch_all(exec)
            .await?;
        Ok(rows)
    }

    pub async fn physical_del_multi_multisig_queue<'a, E>(
        exec: E,
        ids: &[&str],
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if ids.is_empty() {
            "DELETE FROM multisig_queue RETURNING *".to_string()
        } else {
            let ids = crate::any_in_collection(ids, "','");
            format!(
                "DELETE FROM multisig_queue WHERE account_id IN ('{}') RETURNING *",
                ids
            )
        };

        Ok(sqlx::query_as::<sqlx::Sqlite, MultisigQueueEntity>(&sql)
            .fetch_all(exec)
            .await?)
    }

    pub async fn ongoing_queue<'a, E>(
        exec: E,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from multisig_queue where from_addr = ? and chain_code = ? and status in (?,?,?)";

        let res = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(address)
            .bind(chain_code)
            .bind(MultisigQueueStatus::PendingSignature.to_i8())
            .bind(MultisigQueueStatus::HasSignature.to_i8())
            .bind(MultisigQueueStatus::PendingExecution.to_i8())
            .fetch_optional(exec)
            .await?;

        Ok(res)
    }
}
