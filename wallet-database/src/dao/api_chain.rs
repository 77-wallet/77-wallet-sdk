use crate::entities::api_chain::{ApiChainCreateVo, ApiChainEntity, NodeBindType};
use sqlx::{Executor, QueryBuilder, Sqlite};

pub(crate) struct ApiChainDao;

impl ApiChainDao {
    pub async fn list<'a, E>(
        exec: E,
        status: Option<u8>,
    ) -> Result<Vec<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut sql = "SELECT * FROM api_chain".to_string();
        let mut conditions = Vec::new();

        if status.is_some() {
            conditions.push("status = ?".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let mut query = sqlx::query_as::<_, ApiChainEntity>(&sql);

        if let Some(status) = status {
            query = query.bind(status);
        }
        query.fetch_all(exec).await.map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn detail<'a, E>(
        exec: E,
        chain_code: &str,
    ) -> Result<Option<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                        SELECT *
                        FROM api_chain
                        WHERE chain_code = $1 AND status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ApiChainEntity>(sql)
            .bind(chain_code)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert<'c, E>(executor: E, input: ApiChainCreateVo) -> Result<(), crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"Insert into api_chain
            (name, chain_code, protocols, main_symbol, node_bind_type, status, created_at, updated_at)
                values ($1, $2, $3, $4, $5, $6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                on conflict (chain_code)
                do update set
                    status = excluded.status,
                    name = excluded.name,
                    protocols = excluded.protocols,
                    updated_at = excluded.updated_at"#;
        let protocols = wallet_utils::serde_func::serde_to_string(&input.protocols)?;

        sqlx::query_as::<_, ApiChainEntity>(sql)
            .bind(&input.name)
            .bind(&input.chain_code)
            .bind(protocols)
            .bind(&input.main_symbol)
            .bind(input.node_bind_type)
            .bind(input.status)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(())
    }

    /// Local defaults never update an existing backend/user-owned chain row.
    pub async fn insert_missing<'a, E>(
        executor: E,
        input: ApiChainCreateVo,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        sqlx::query("INSERT INTO api_chain (name, chain_code, protocols, main_symbol, node_bind_type, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) ON CONFLICT(chain_code) DO NOTHING")
            .bind(input.name).bind(input.chain_code)
            .bind(wallet_utils::serde_func::serde_to_string(&input.protocols)?)
            .bind(input.main_symbol).bind(input.node_bind_type).bind(input.status)
            .execute(executor).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(())
    }

    pub async fn detail_with_main_symbol<'a, E>(
        exec: E,
        main_symbol: &str,
    ) -> Result<Option<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                        SELECT *
                        FROM api_chain
                        WHERE main_symbol = $1 AND status = 1;"#;
        sqlx::query_as::<sqlx::Sqlite, ApiChainEntity>(sql)
            .bind(main_symbol)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn upsert_multi_chain<'a, E>(
        executor: E,
        input: Vec<ApiChainCreateVo>,
    ) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if input.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "INSERT INTO api_chain (name, chain_code, protocols, status, main_symbol, created_at, updated_at) ",
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

    pub async fn toggle_chains_status<'a, E>(
        executor: E,
        chain_codes: &[String],
    ) -> Result<Vec<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        Self::toggle_chains_status_except(executor, chain_codes, &[]).await
    }

    pub async fn toggle_chains_status_except<'a, E>(
        executor: E,
        chain_codes: &[String],
        preserved: &[String],
    ) -> Result<Vec<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new(
            "UPDATE api_chain
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
            ",
        );
        if !preserved.is_empty() {
            query.push(" WHERE chain_code NOT IN (");
            let mut values = query.separated(", ");
            for code in preserved {
                values.push_bind(code);
            }
            values.push_unseparated(")");
        }
        query.push(" RETURNING *");

        query
            .build_query_as::<ApiChainEntity>()
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // pub async fn set_chain_node_id_empty<'a, E>(
    //     executor: E,
    //     node_id: &str,
    // ) -> Result<Vec<ApiChainEntity>, crate::Error>
    // where
    //     E: Executor<'a, Database = Sqlite>,
    // {
    //     let sql = r#"
    //         update api_chain set
    //             node_id = null,
    //             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
    //         where node_id = $1
    //         RETURNING *
    //         "#;

    //     sqlx::query_as::<sqlx::Sqlite, ApiChainEntity>(sql)
    //         .bind(node_id)
    //         .fetch_all(executor)
    //         .await
    //         .map_err(|e| crate::Error::Database(e.into()))
    // }

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
            update api_chain set
                node_id = $2,
                node_bind_type = $3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where chain_code = $1
        "#;
        sqlx::query(sql)
            .bind(chain_code)
            .bind(node_id)
            .bind(NodeBindType::ManualUser as u8)
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
        update api_chain set 
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

    pub async fn set_api_chain_node<'a, E>(
        executor: E,
        chain_code: &str,
        node_id: &str,
    ) -> Result<Vec<ApiChainEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            update api_chain set 
                node_id = $2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where chain_code = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, ApiChainEntity>(sql)
            .bind(chain_code)
            .bind(node_id)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    // NOTE: Do not add `api_chain` ↔ `node` joins here.
    // `api_chain` lives in `api_wallet.db`, while `node` lives in `data.db` (core db).
    // Compose chain+node data at the repository/service layer where both pools are available.
}
