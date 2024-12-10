use crate::entities::chain::{ChainCreateVo, ChainEntity, ChainWithNode};
use sqlx::{Executor, Sqlite};

pub struct Id {
    pub chain_code: String,
    pub node_id: String,
}

impl ChainEntity {
    pub async fn upsert<'c, E>(
        executor: E,
        input: ChainCreateVo,
    ) -> Result<ChainEntity, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"Insert into chain 
            (name, chain_code, node_id, protocols, main_symbol, status, created_at, updated_at)
                values ($1, $2, $3, $4, $5, $6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                on conflict (chain_code)
                do update set
                    updated_at = excluded.updated_at
                returning *"#;
        let protocols = wallet_utils::serde_func::serde_to_string(&input.protocols)?;

        let mut rec = sqlx::query_as::<_, ChainEntity>(sql)
            .bind(&input.name)
            .bind(&input.chain_code)
            .bind(&input.node_id)
            .bind(protocols)
            .bind(&input.main_symbol)
            .bind(input.status)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(rec.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    pub async fn set_chain_node<'a, E>(
        executor: E,
        chain_code: &str,
        node_id: &str,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update chain set 
                node_id = $2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where chain_code = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .bind(chain_code)
            .bind(node_id)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // 把指定的链status设置为1，其他设置为0
    pub async fn toggle_chains_status<'a, E>(
        executor: E,
        chain_codes: Vec<String>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let chain_codes = crate::any_in_collection(chain_codes, "','");
        // 用逗号分隔的列表，用于 IN 查询
        let sql = format!(
            "UPDATE chain
            SET 
                status = CASE 
                            WHEN chain_code IN ('{}') THEN 1 
                            ELSE 0 
                        END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            RETURNING *",
            chain_codes
        );

        // 为 `IN` 子句绑定参数，确保 chain_codes 被正确处理
        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(&sql)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail<'a, E>(exec: E, chain_code: &str) -> Result<Option<Self>, crate::Error>
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

    pub async fn detail_by_id<'a, E>(exec: E, node_id: &str) -> Result<Option<Self>, crate::Error>
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
    ) -> Result<Option<Self>, crate::Error>
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

    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM chain WHERE status = 1;";

        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn list_with_node_info<'a, E>(exec: E) -> Result<Vec<ChainWithNode>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "select q.*, a.rpc_url, a.ws_url, a.http_url, a.network, 
        a.name as node_name
                            from chain as q  
                            join node a on q.node_id = a.node_id WHERE q.status = 1;";

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

    pub async fn get_chain_list_in_chain_code(
        db: std::sync::Arc<sqlx::Pool<sqlx::Sqlite>>,
        chain_codes: Vec<&str>,
    ) -> Result<Vec<Self>, crate::Error> {
        let chain_codes = crate::any_in_collection(chain_codes, "','");
        let sql = format!(
            "SELECT name, chain_code, node_id, protocols, main_symbol, status, created_at, updated_at FROM chain
        WHERE chain_code in ('{}') and status = 1;",
            chain_codes
        );

        sqlx::query_as::<sqlx::Sqlite, ChainEntity>(&sql)
            .bind(chain_codes)
            .fetch_all(db.as_ref())
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
