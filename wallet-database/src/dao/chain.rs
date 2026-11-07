use crate::entities::{
    api_chain::NodeBindType,
    chain::{ChainCreateVo, ChainEntity, ChainWithNode},
};

pub struct ChainDao;

use crate::sql_utils::{SqlExecutableReturn as _, query_builder::DynamicQueryBuilder};
use sqlx::{Executor, QueryBuilder, Sqlite};

impl ChainDao {
    /// Default initialization adds missing rows without replacing saved chain state.
    /// Keep the same INSERT columns and node-binding default as the legacy upsert.
    pub async fn insert_missing<'c, E>(
        executor: E,
        input: ChainCreateVo,
    ) -> Result<bool, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let protocols = wallet_utils::serde_func::serde_to_string(&input.protocols)?;
        let result = sqlx::query("INSERT INTO chain (name, chain_code, protocols, main_symbol, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) ON CONFLICT(chain_code) DO NOTHING")
            .bind(input.name).bind(input.chain_code).bind(protocols)
            .bind(input.main_symbol).bind(input.status)
            .execute(executor).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected() != 0)
    }

    pub async fn upsert<'c, E>(
        executor: E,
        input: ChainCreateVo,
    ) -> Result<ChainEntity, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"Insert into chain 
            (name, chain_code, protocols, main_symbol, status, created_at, updated_at)
                values ($1, $2, $3, $4, $5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                on conflict (chain_code)
                do update set
                    status = excluded.status,
                    name = excluded.name,
                    protocols = excluded.protocols,
                    updated_at = excluded.updated_at
                returning *"#;
        let protocols = wallet_utils::serde_func::serde_to_string(&input.protocols)?;

        let mut rec = sqlx::query_as::<_, ChainEntity>(sql)
            .bind(&input.name)
            .bind(&input.chain_code)
            .bind(protocols)
            .bind(&input.main_symbol)
            .bind(input.status)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(rec.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    /// 用户选择节点
    pub async fn user_select<'a, E>(
        executor: E,
        chain_code: &str,
        node_id: &str,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        // 直接写成 ManualUser
        let sql = r#"
            update chain set
                node_id = $2,
                node_bind_type = $3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where chain_code = $1
        "#;
        sqlx::query(sql)
            .bind(chain_code)
            .bind(node_id)
            .bind(NodeBindType::ManualUser)
            .execute(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    pub async fn set_chain_node_with_type<'a, E>(
        executor: E,
        chain_code: &str,
        node_id: &str,
        bind_type: NodeBindType,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
        update chain set 
            node_id = $2,
            node_bind_type = $3,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        where chain_code = $1
    "#;

        sqlx::query(sql)
            .bind(chain_code)
            .bind(node_id)
            .bind(bind_type)
            .execute(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    // 把指定的链status设置为1，其他设置为0
    pub async fn upsert_multi_chain<'a, E>(
        executor: E,
        input: Vec<ChainCreateVo>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if input.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO chain (name, chain_code, protocols, status, main_symbol, created_at, updated_at) ",
        );

        query_builder.push_values(input, |mut b, chain| {
            let protocols =
                wallet_utils::serde_func::serde_to_string(&chain.protocols).unwrap_or_default();
            b.push_bind(chain.name.clone())
                .push_bind(chain.chain_code)
                .push_bind(protocols)
                .push_bind(chain.status)
                .push_bind(chain.main_symbol)
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
                .push("strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
        });

        query_builder.push(
            " ON CONFLICT (chain_code)
              DO UPDATE SET
                  name = excluded.name,
                  status = excluded.status,
                  main_symbol = excluded.main_symbol,
                  updated_at = excluded.updated_at",
        );
        let query = query_builder.build();
        query.execute(executor).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    // 把指定的链status设置为1，其他设置为0
    pub async fn toggle_chains_status<'a, E>(
        executor: E,
        chain_codes: &[String],
    ) -> Result<Vec<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new(
            "UPDATE chain
            SET 
                status = CASE 
                            WHEN chain_code IN (",
        );
        let mut values = query.separated(", ");
        if chain_codes.is_empty() {
            values.push_bind("");
        } else {
            for chain_code in chain_codes {
                values.push_bind(chain_code);
            }
        }
        values.push_unseparated(
            ") THEN 1
                            ELSE 0 
                        END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            RETURNING *",
        );

        query
            .build_query_as::<ChainEntity>()
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                        SELECT *
                        FROM chain
                        WHERE chain_code = $1 AND status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                        DELETE
                        FROM chain
                        WHERE chain_code = $1 
                        RETURNING * "#;
        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail_by_id<'a, E>(
        exec: E,
        node_id: &str,
    ) -> Result<Option<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            SELECT *
            FROM chain
            WHERE node_id = $1 AND status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .bind(node_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail_with_main_symbol<'a, E>(
        exec: E,
        main_symbol: &str,
    ) -> Result<Option<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                        SELECT *
                        FROM chain
                        WHERE main_symbol = $1 AND status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .bind(main_symbol)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_v2<'a, E>(
        executor: E,
        status: Option<u8>,
    ) -> Result<Vec<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut builder = DynamicQueryBuilder::new("SELECT * FROM chain");

        if let Some(status) = status {
            builder = builder.and_where_eq("status", status);
        }

        builder.fetch_all(executor).await
    }

    pub async fn list<'a, E>(exec: E, status: Option<u8>) -> Result<Vec<ChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "SELECT * FROM chain".to_string();
        let mut conditions = Vec::new();

        if status.is_some() {
            conditions.push("status = ?".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let mut query = sqlx::query_as::<_, ChainEntity>(&sql);

        if let Some(status) = status {
            query = query.bind(status);
        }
        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_with_node_info<'a, E>(exec: E) -> Result<Vec<ChainWithNode>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select q.*, a.rpc_url, a.ws_url, a.http_url, a.network, a.name as node_name
                            from chain as q  
                            left join node a on q.node_id = a.node_id WHERE q.status = 1;";

        sqlx::query_as::<sqlx::Sqlite, ChainWithNode>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn chain_node_info<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<Option<ChainWithNode>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                            select q.*, a.rpc_url, a.ws_url, a.http_url, a.network, a.name as node_name
                            from chain as q  
                            join node a on q.node_id = a.node_id
                            where q.chain_code = ? and q.status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ChainWithNode>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn chain_node_info_left_join<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<Option<ChainWithNode>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                            select q.*, a.rpc_url, a.ws_url, a.http_url, a.network, a.name as node_name
                            from chain as q  
                            left join node a on q.node_id = a.node_id
                            where q.chain_code = ? and q.status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ChainWithNode>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_chain_list_in_chain_code(
        db: std::sync::Arc<sqlx::Pool<sqlx::Sqlite>>,
        chain_codes: Vec<&str>,
    ) -> Result<Vec<ChainEntity>, crate::Error> {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM chain WHERE chain_code IN (");
        let mut values = query.separated(", ");
        if chain_codes.is_empty() {
            values.push_bind("");
        } else {
            for chain_code in chain_codes {
                values.push_bind(chain_code);
            }
        }
        values.push_unseparated(") AND status = 1");

        query
            .build_query_as::<ChainEntity>()
            .fetch_all(db.as_ref())
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
