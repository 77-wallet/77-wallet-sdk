use crate::{
    entities::multisig_queue::{
        MultisigQueueEntity, MultisigQueueSimpleEntity, MultisigQueueStatus,
        NewMultisigQueueEntity,
        fail_reason::{EXPIRED, PERMISSION_CHANGE},
    },
    pagination::Pagination,
};
use chrono::SecondsFormat;
use sqlx::{Executor, Pool, QueryBuilder, Sqlite};
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
            (id, from_addr,to_addr,value,expiration,symbol,chain_code,token_addr,msg_hash,
             tx_hash,raw_data,status,notes,fail_reason,created_at,account_id,transfer_type,permission_id)
                values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,?)
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
            .bind(params.transfer_type.to_i8())
            .bind(&params.permission_id)
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
        let res =
            sqlx::query_as::<_, MultisigQueueEntity>(sql).bind(id).fetch_optional(pool).await?;
        Ok(res)
    }

    pub async fn find_with_extra<'a, E>(
        id: &str,
        pool: E,
    ) -> Result<Option<MultisigQueueSimpleEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_queue where id = ?"#;
        let res = sqlx::query_as::<_, MultisigQueueSimpleEntity>(sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(res)
    }

    pub async fn find_by_id_with_account<'a, E>(
        id: &str,
        pool: E,
    ) -> Result<Option<MultisigQueueSimpleEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                select q.*,a.name,a.threshold,a.member_num,a.owner,a.initiator_addr
                from multisig_queue as q  
                join multisig_account a on q.account_id = a.id
                where q.id = ?"#;
        let res = sqlx::query_as::<_, MultisigQueueSimpleEntity>(sql)
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(res)
    }

    // 0 待签名 1已签名 2待执行，确认中，4成功 5失败;待执行需要查询2中状态
    pub async fn lists(
        from: Option<&str>,
        chain_code: Option<&str>,
        status: i32,
        page: i64,
        page_size: i64,
        pool: Arc<Pool<Sqlite>>,
    ) -> Result<Pagination<MultisigQueueSimpleEntity>, crate::DatabaseError> {
        let mut paginate = Pagination::<MultisigQueueSimpleEntity>::init(page, page_size);
        let build_filters = |query: &mut QueryBuilder<'_, Sqlite>| {
            query.push(" WHERE ");
            if status == 2 {
                query
                    .push("status in (")
                    .push_bind(MultisigQueueStatus::PendingExecution.to_i8())
                    .push(",")
                    .push_bind(MultisigQueueStatus::InConfirmation.to_i8())
                    .push(")");
            } else {
                query.push("status = ").push_bind(status);
            }
            if let Some(from_addr) = from {
                query.push(" AND from_addr = ").push_bind(from_addr.to_owned());
            }
            if let Some(chain) = chain_code {
                query.push(" AND chain_code = ").push_bind(chain.to_owned());
            }
        };

        let mut count_query = QueryBuilder::<Sqlite>::new("SELECT count(*) FROM multisig_queue");
        build_filters(&mut count_query);
        paginate.total_count = count_query.build_query_scalar::<i64>().fetch_one(&*pool).await?;

        let mut data_query = QueryBuilder::<Sqlite>::new("SELECT * FROM multisig_queue");
        build_filters(&mut data_query);
        data_query
            .push(" ORDER BY created_at DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page * page_size);
        paginate.data =
            data_query.build_query_as::<MultisigQueueSimpleEntity>().fetch_all(&*pool).await?;

        Ok(paginate)
    }

    pub async fn list_by_account_ids<'a, E>(
        account_ids: &[String],
        pool: E,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if account_ids.is_empty() {
            return Ok(None);
        }

        let mut query =
            QueryBuilder::<Sqlite>::new("select * from multisig_queue where account_id in (");
        {
            let mut separated = query.separated(",");
            for account_id in account_ids {
                separated.push_bind(account_id);
            }
        }
        query.push(") order by created_at desc limit 1");

        let res = query.build_query_as::<MultisigQueueEntity>().fetch_optional(pool).await?;

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
        let sql = r#"update multisig_queue set status = ? where id = ? and status not in (?,?)"#;

        let _res = sqlx::query(sql)
            .bind(status.to_i8())
            .bind(id)
            .bind(MultisigQueueStatus::Fail.to_i8())
            .bind(MultisigQueueStatus::Success.to_i8())
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

        let _rs = sqlx::query(sql)
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

        let _res =
            sqlx::query(sql).bind(status.to_i8()).bind(tx_hash).bind(id).execute(exec).await?;

        Ok(())
    }

    pub async fn get_latest<'a, E>(
        exec: E,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"select * from multisig_queue order by created_at desc limit 1"#;

        let row = sqlx::query_as::<_, MultisigQueueEntity>(sql).fetch_optional(exec).await?;

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
        let rows = sqlx::query_as::<_, MultisigQueueEntity>(sql).bind(id).fetch_all(exec).await?;

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
        let rows = sqlx::query_as::<_, MultisigQueueEntity>(sql).bind(id).fetch_all(exec).await?;
        Ok(rows)
    }

    pub async fn delete_by_id<'a, E>(id: &str, exec: E) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM multisig_queue WHERE id = $1";
        sqlx::query(sql).bind(id).execute(exec).await?;
        Ok(())
    }

    pub async fn delete_by_permission<'a, E>(
        permission_id: &str,
        exec: E,
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM multisig_queue WHERE permission_id = $1";

        let rows = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(permission_id)
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
        let mut query = if ids.is_empty() {
            QueryBuilder::<Sqlite>::new("DELETE FROM multisig_queue RETURNING *")
        } else {
            let mut query =
                QueryBuilder::<Sqlite>::new("DELETE FROM multisig_queue WHERE account_id IN (");
            {
                let mut separated = query.separated(",");
                for id in ids {
                    separated.push_bind(id);
                }
            }
            query.push(") RETURNING *");
            query
        };

        Ok(query.build_query_as::<MultisigQueueEntity>().fetch_all(exec).await?)
    }

    pub async fn ongoing_queue<'a, E>(
        exec: E,
        chain_code: &str,
        address: &str,
    ) -> Result<Option<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from multisig_queue where from_addr = ? and chain_code = ? and status in (?,?,?,?)";

        let res = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(address)
            .bind(chain_code)
            .bind(MultisigQueueStatus::PendingSignature.to_i8())
            .bind(MultisigQueueStatus::HasSignature.to_i8())
            .bind(MultisigQueueStatus::PendingExecution.to_i8())
            .bind(MultisigQueueStatus::InConfirmation.to_i8())
            .fetch_optional(exec)
            .await?;

        Ok(res)
    }

    pub async fn permission_fail<'a, E>(
        address: &str,
        exec: E,
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"update multisig_queue set status = ?,fail_reason = ? where status in (?,?,?) and from_addr = ?  RETURNING *"#;

        let res = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(MultisigQueueStatus::Fail.to_i8())
            .bind(PERMISSION_CHANGE)
            .bind(MultisigQueueStatus::PendingSignature.to_i8())
            .bind(MultisigQueueStatus::HasSignature.to_i8())
            .bind(MultisigQueueStatus::PendingExecution.to_i8())
            .bind(address)
            .fetch_all(exec)
            .await?;
        Ok(res)
    }

    pub async fn pending_handle<'a, E>(
        exec: E,
    ) -> Result<Vec<MultisigQueueEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select * from multisig_queue where status in (?,?)";

        let res = sqlx::query_as::<_, MultisigQueueEntity>(sql)
            .bind(MultisigQueueStatus::PendingSignature.to_i8())
            .bind(MultisigQueueStatus::PendingExecution.to_i8())
            .fetch_all(exec)
            .await?;

        Ok(res)
    }
}
