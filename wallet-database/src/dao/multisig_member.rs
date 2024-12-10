use crate::entities::multisig_member::{
    MultisigMemberEntities, MultisigMemberEntity, NewMemberEntity,
};
use sqlx::{Executor, Sqlite};

pub struct MultisigMemberDaoV1;

impl MultisigMemberDaoV1 {
    pub async fn batch_add<'a, E>(params: &[NewMemberEntity], exec: E) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = String::from(
            "INSERT INTO multisig_member (account_id, address, name, confirmed,is_self,pubkey,uid,created_at) VALUES ",
        );

        for (i, param) in params.iter().enumerate() {
            if i != 0 {
                query.push_str(", ");
            }
            query.push_str(&format!(
                "('{}', '{}', '{}', {}, {},'{}','{}', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
                param.account_id,
                param.address,
                param.name,
                param.confirmed,
                param.is_self,
                param.pubkey,
                param.uid,
            ));
        }

        query.push_str(
            "ON CONFLICT (account_id, address)
        DO UPDATE SET
            is_self = EXCLUDED.is_self,
            is_del = EXCLUDED.is_del,
            updated_at = EXCLUDED.updated_at;",
        );

        sqlx::query(&query)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    // get list by account_id
    pub async fn list_by_account_id<'a, E>(
        account_id: &str,
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"
            SELECT *
            FROM multisig_member
            WHERE account_id =?
        "#;

        let res = sqlx::query_as::<_, MultisigMemberEntity>(query)
            .bind(account_id)
            .fetch_all(exec)
            .await?;

        Ok(MultisigMemberEntities(res))
    }

    pub async fn get_self_by_id<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"
                SELECT m.account_id, m.address, m.name, m.uid, m.confirmed, m.is_self,m.pubkey,m.created_at, m.updated_at
                FROM multisig_member m
                JOIN multisig_account a ON m.account_id = a.id
                WHERE a.id = ? and m.is_self = 1
            "#;

        let res = sqlx::query_as(query).bind(id).fetch_all(exec).await?;

        Ok(MultisigMemberEntities(res))
    }

    // 同步多签账户确认状态
    pub async fn sync_confirmed_status<'a, E>(
        account_id: &str,
        address: &str,
        confirmed: i8,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query =
            r#"update multisig_member set confirmed = ? where account_id = ? and address = ?"#;

        let _ = sqlx::query(query)
            .bind(confirmed)
            .bind(account_id)
            .bind(address)
            .execute(exec)
            .await?;

        Ok(())
    }

    // 同步多签账户确认状态和公钥
    pub async fn sync_confirmed_and_pubkey_status<'a, E>(
        account_id: &str,
        address: &str,
        pubkey: &str,
        confirmed: i8,
        uid: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"update multisig_member set pubkey = $1, confirmed = $2,uid = $3, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') where account_id = $4 and address = $5"#;
        let _res = sqlx::query(query)
            .bind(pubkey)
            .bind(confirmed)
            .bind(uid)
            .bind(account_id)
            .bind(address)
            .execute(exec)
            .await?;
        Ok(())
    }

    pub async fn find_records_by_id<'a, E>(
        account_id: &str,
        pool: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let query = r#"select * from multisig_member where account_id = ?"#;

        let res = sqlx::query_as::<_, MultisigMemberEntity>(query)
            .bind(account_id)
            .fetch_all(pool)
            .await?;

        Ok(MultisigMemberEntities(res))
    }

    pub async fn list_by_uid<'a, E>(
        uid: &str,
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT *
            FROM multisig_member
            WHERE uid = ?
        "#;

        let res = sqlx::query_as::<_, MultisigMemberEntity>(sql)
            .bind(uid)
            .fetch_all(exec)
            .await?;
        Ok(MultisigMemberEntities(res))
    }

    pub async fn list_by_uids<'a, E>(
        uids: &Vec<String>,
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let uids = crate::any_in_collection(uids, "','");

        let sql = format!(
            "SELECT *
            FROM multisig_member
            WHERE uid IN ('{}')",
            uids
        );
        let res = sqlx::query_as::<_, MultisigMemberEntity>(&sql)
            .fetch_all(exec)
            .await?;
        Ok(MultisigMemberEntities(res))
    }

    pub async fn list_by_addresses<'a, E>(
        addresses: &[String],
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(addresses, "','");

        let sql = format!(
            "SELECT *
            FROM multisig_member
            WHERE address IN ('{}')",
            addresses
        );
        let res = sqlx::query_as::<_, MultisigMemberEntity>(&sql)
            .fetch_all(exec)
            .await?;
        Ok(MultisigMemberEntities(res))
    }

    pub async fn list_by_account_ids_not_addresses<'a, E>(
        account_ids: &[String],
        addresses: &[String],
        exec: E,
    ) -> Result<MultisigMemberEntities, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let addresses = crate::any_in_collection(addresses, "','");
        let account_ids = crate::any_in_collection(account_ids, "','");

        let sql = format!(
            "SELECT *
            FROM multisig_member
            WHERE account_id IN ('{}')
            AND address NOT IN ('{}')",
            account_ids, addresses
        );

        tracing::warn!("sql: {}", sql);
        let res = sqlx::query_as::<_, MultisigMemberEntity>(&sql)
            .fetch_all(exec)
            .await?;
        Ok(MultisigMemberEntities(res))
    }

    pub async fn logic_del_multisig_member<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<(), crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "UPDATE multisig_member SET is_del = 1 WHERE account_id = $1".to_string();
        sqlx::query(&sql).bind(id).execute(exec).await?;

        Ok(())
    }

    pub async fn physical_del_multisig_member<'a, E>(
        id: &str,
        exec: E,
    ) -> Result<Vec<MultisigMemberEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "DELETE FROM multisig_member WHERE account_id = $1 RETURNING *";
        let rows = sqlx::query_as::<_, MultisigMemberEntity>(sql)
            .bind(id)
            .fetch_all(exec)
            .await?;
        Ok(rows)
    }

    pub async fn physical_del_multi_multisig_member<'a, E>(
        exec: E,
        ids: &[&str],
    ) -> Result<Vec<MultisigMemberEntity>, crate::DatabaseError>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = if ids.is_empty() {
            "DELETE FROM multisig_member RETURNING *".to_string()
        } else {
            let ids = crate::any_in_collection(ids, "','");
            format!(
                "DELETE FROM multisig_member WHERE account_id IN ('{}') RETURNING *",
                ids
            )
        };

        Ok(sqlx::query_as::<sqlx::Sqlite, MultisigMemberEntity>(&sql)
            .fetch_all(exec)
            .await?)
    }
}
