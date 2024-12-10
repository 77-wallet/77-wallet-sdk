use sqlx::{Executor, Sqlite};

use crate::entities::node::{NodeCreateVo, NodeEntity};

impl NodeCreateVo {
    pub fn new(name: &str, chain_code: &str, rpc_url: &str) -> NodeCreateVo {
        Self {
            name: name.to_string(),
            chain_code: chain_code.to_string(),
            rpc_url: rpc_url.to_string(),
            ws_url: "".to_string(),
            http_url: "".to_string(),
            network: "mainnet".to_string(),
            status: 1,
        }
    }

    pub fn with_http_url(mut self, http_url: &str) -> Self {
        self.http_url = http_url.to_string();
        self
    }

    pub fn with_network(mut self, network: &str) -> Self {
        self.network = network.to_string();
        self
    }
}

impl NodeEntity {
    pub async fn upsert<'a, E>(exec: E, req: NodeCreateVo) -> Result<NodeEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"Insert into node 
            (node_id, name, chain_code, status, rpc_url, ws_url,http_url, network, created_at, updated_at)
                values ($1, $2, $3, $4, $5, $6,$7,$8, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                on conflict (node_id)
                do update set
                    rpc_url = excluded.rpc_url,
                    ws_url = excluded.ws_url,
                    http_url = excluded.http_url,
                    name = excluded.name,
                    network = excluded.network,
                    updated_at = excluded.updated_at
                returning *"#;

        let params = vec![req.name.as_str(), req.chain_code.as_str()];
        let node_id = wallet_utils::snowflake::gen_hash_uid(params);

        let mut rec = sqlx::query_as::<_, NodeEntity>(sql)
            .bind(node_id)
            .bind(&req.name)
            .bind(&req.chain_code)
            .bind(req.status)
            .bind(&req.rpc_url)
            .bind(&req.ws_url)
            .bind(&req.http_url)
            .bind(&req.network)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(rec.pop().ok_or(crate::DatabaseError::ReturningNone)?)
    }

    // pub async fn detail(
    //     db: std::sync::Arc<sqlx::Pool<sqlx::Sqlite>>,
    //     id: i64,
    // ) -> Result<Option<Self>, crate::Error> {
    //     let sql = r#"
    //                     SELECT *
    //                     FROM node
    //                     WHERE id = $1;"#;
    //     sqlx::query_as::<sqlx::Sqlite, NodeEntity>(sql)
    //         .bind(id)
    //         .fetch_optional(db.as_ref())
    //         .await
    //         .map_err(|e| crate::Error::Database(e.into()))
    // }

    pub async fn list<'a, E>(exec: E) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = "SELECT * FROM node;";

        sqlx::query_as::<sqlx::Sqlite, NodeEntity>(sql)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_node_list_in_chain_codes<'a, E>(
        exec: E,
        chain_codes: Vec<&str>,
    ) -> Result<Vec<Self>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let chain_codes = crate::any_in_collection(chain_codes, "','");
        let sql = format!(
            "SELECT * FROM node
        WHERE chain_code in ('{}');",
            chain_codes
        );
        sqlx::query_as::<sqlx::Sqlite, NodeEntity>(&sql)
            .bind(chain_codes)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete<'a, E>(exec: E, rpc_url: &str, chain_code: &str) -> Result<(), crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            DELETE FROM node
            WHERE rpc_url = $1 AND
            chain_code = $2
            "#;

        sqlx::query(sql)
            .bind(rpc_url)
            .bind(chain_code)
            .execute(exec)
            .await
            .map(|_| ())
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
