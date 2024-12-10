use crate::{
    entities::stake::{DelegateEntity, NewDelegateEntity, NewUnFreezeEntity, UnFreezeEntity},
    pagination::Pagination,
};
use sqlx::{Executor, Sqlite};

pub async fn add_unfreeze<'a, E>(
    stake: NewUnFreezeEntity,
    exec: E,
) -> Result<(), crate::error::DatabaseError>
where
    E: Executor<'a, Database = Sqlite>,
{
    let sql = r#"insert into unfreeze 
        (id,tx_hash,owner_address,resource_type,amount,freeze_time,created_at)
     values (?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))"#;

    let id = wallet_utils::snowflake::get_uid()?;
    let _res = sqlx::query(sql)
        .bind(id.to_string())
        .bind(stake.tx_hash)
        .bind(stake.owner_address)
        .bind(stake.resource_type)
        .bind(stake.amount)
        .bind(stake.freeze_time)
        .execute(exec)
        .await?;

    Ok(())
}

pub async fn unfreeze_list(
    owner: &str,
    resource_type: &str,
    page: i64,
    page_size: i64,
    exec: &crate::DbPool,
) -> Result<Pagination<UnFreezeEntity>, crate::error::DatabaseError> {
    let time = wallet_utils::time::now().timestamp();
    let sql = format!(
        "select * FROM unfreeze where owner_address = '{}' and resource_type = '{}' and freeze_time > {} order by created_at desc ",
        owner, resource_type,time
    );

    let pagination = Pagination::init(page, page_size);
    let res = pagination.page(exec, &sql).await?;

    Ok(res)
}
pub async fn add_delegate<'a, E>(
    delegate: NewDelegateEntity,
    exec: E,
) -> Result<(), crate::error::DatabaseError>
where
    E: Executor<'a, Database = Sqlite>,
{
    let sql = r#"insert into delegate 
        (id,tx_hash,owner_address,receiver_address,resource_type,amount,status,lock,lock_period,created_at)
     values (?,?,?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))"#;

    let id = wallet_utils::snowflake::get_uid()?;
    let _res = sqlx::query(sql)
        .bind(id.to_string())
        .bind(delegate.tx_hash)
        .bind(delegate.owner_address)
        .bind(delegate.receiver_address)
        .bind(delegate.resource_type)
        .bind(delegate.amount)
        .bind(0)
        .bind(delegate.lock)
        .bind(delegate.lock_period)
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn delegate_list(
    owner: &str,
    resource_type: &str,
    page: i64,
    page_size: i64,
    exec: crate::DbPool,
) -> Result<Pagination<DelegateEntity>, crate::error::DatabaseError> {
    let sql = format!(
        "select * FROM delegate where owner_address = '{}' and resource_type = '{}' order by created_at desc ",
        owner, resource_type
    );

    let pagination = Pagination::init(page, page_size);
    let res = pagination.page(&exec, &sql).await?;

    Ok(res)
}

pub async fn update_delegate<'a, E>(id: &str, exec: E) -> Result<(), crate::error::DatabaseError>
where
    E: Executor<'a, Database = Sqlite>,
{
    let sql = "update delegate set status = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') where id = ?";
    let _res = sqlx::query(sql).bind(id).execute(exec).await?;
    Ok(())
}

pub async fn find_delegate_by_id<'a, E>(
    id: &str,
    exec: E,
) -> Result<DelegateEntity, crate::error::DatabaseError>
where
    E: Executor<'a, Database = Sqlite>,
{
    let sql = "select * from delegate where id = ?";
    let res = sqlx::query_as::<_, DelegateEntity>(sql)
        .bind(id)
        .fetch_one(exec)
        .await?;
    Ok(res)
}
