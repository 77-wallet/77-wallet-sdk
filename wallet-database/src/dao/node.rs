use sqlx::{Executor, QueryBuilder, Sqlite};

use crate::entities::node::{NodeCreateVo, NodeEntity};

impl NodeCreateVo {
    pub fn new(
        node_id: &str,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        http_url: Option<String>,
    ) -> NodeCreateVo {
        Self {
            node_id: node_id.to_string(),
            name: name.to_string(),
            chain_code: chain_code.to_string(),
            rpc_url: rpc_url.to_string(),
            ws_url: "".to_string(),
            http_url: http_url.unwrap_or_default(),
            network: "mainnet".to_string(),
            status: 1,
            is_local: 0,
        }
    }

    pub fn with_status(mut self, status: u8) -> Self {
        self.status = status;
        self
    }

    pub fn with_http_url(mut self, http_url: &str) -> Self {
        self.http_url = http_url.to_string();
        self
    }

    pub fn with_network(mut self, network: &str) -> Self {
        self.network = network.to_string();
        self
    }

    pub fn with_is_local(mut self, is_local: u8) -> Self {
        self.is_local = is_local;
        self
    }
}

pub struct NodeDao;

impl NodeDao {
    pub async fn detail_by_node_id<'a, E>(
        exec: E,
        node_id: &str,
    ) -> Result<Option<NodeEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
                    SELECT *
                    FROM node
                    WHERE node_id = $1;"#;
        sqlx::query_as::<sqlx::Sqlite, NodeEntity>(sql)
            .bind(node_id)
            .fetch_optional(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn insert_missing<'a, E>(exec: E, req: NodeCreateVo) -> Result<bool, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let result = sqlx::query("INSERT INTO node (node_id, name, chain_code, status, is_local, rpc_url, ws_url, http_url, network, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) ON CONFLICT(node_id) DO NOTHING")
            .bind(req.node_id).bind(req.name).bind(req.chain_code).bind(req.status).bind(req.is_local)
            .bind(req.rpc_url).bind(req.ws_url).bind(req.http_url).bind(req.network)
            .execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(result.rows_affected() != 0)
    }

    pub async fn upsert<'a, E>(exec: E, req: NodeCreateVo) -> Result<NodeEntity, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"Insert into node 
                (node_id, name, chain_code, status, is_local, 
                    rpc_url, ws_url, http_url, network, 
                    created_at, updated_at)
                values ($1, $2, $3, $4, $5, 
                        $6, $7, $8, $9,
                        strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                on conflict (node_id)
                do update set
                    rpc_url = excluded.rpc_url,
                    ws_url = excluded.ws_url,
                    http_url = excluded.http_url,
                    name = excluded.name,
                    status =  excluded.status,
                    network = excluded.network,
                    updated_at = excluded.updated_at
                returning *"#;

        // let params = vec![req.name.as_str(), req.chain_code.as_str()];
        // let node_id = wallet_utils::snowflake::gen_hash_uid(params);

        let mut rec = sqlx::query_as::<_, NodeEntity>(sql)
            .bind(&req.node_id)
            .bind(&req.name)
            .bind(&req.chain_code)
            .bind(req.status)
            .bind(req.is_local)
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

    pub async fn disable_backend_not_in<'a, E>(
        exec: E,
        chain_code: &str,
        backend_ids: &[String],
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        if backend_ids.is_empty() {
            // 后端返回空：直接禁用所有 backend 节点
            let res = sqlx::query(
                r#"
            update node
            set status = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
            where is_local = 0
              and chain_code = ?
            "#,
            )
            .bind(chain_code)
            .execute(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

            return Ok(res.rows_affected());
        }

        // 构造 (?, ?, ?, ...)
        let placeholders = backend_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

        let sql = format!(
            r#"
        update node
        set status = 0,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
        where is_local = 0
          and chain_code = ?
          and node_id not in ({})
        "#,
            placeholders
        );

        let mut q = sqlx::query(&sql);
        q = q.bind(chain_code);
        for id in backend_ids {
            q = q.bind(id);
        }

        let res = q.execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(res.rows_affected())
    }

    pub async fn refresh_visibility_for_networks<'a, E>(
        exec: E,
        visible_networks: &[&str],
    ) -> Result<u64, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let status_sql =
            if visible_networks.iter().any(|network| network.eq_ignore_ascii_case("testnet")) {
                "case \
                when network in ('mainnet', 'testnet') or network is null or network = '' then 1 \
                else 0 \
            end"
            } else {
                "case \
                when network = 'mainnet' or network is null or network = '' then 1 \
                else 0 \
            end"
            };

        let sql = format!(
            "update node set status = {status_sql}, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') where is_local = 1"
        );
        let res =
            sqlx::query(&sql).execute(exec).await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(res.rows_affected())
    }

    pub async fn list<'a, E>(
        exec: E,
        chain_codes: &[String],
        is_local: Option<u8>,
        status: Option<u8>,
    ) -> Result<Vec<NodeEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        NodeDao::list_with_network(exec, chain_codes, is_local, status, None).await
    }

    pub async fn list_with_network<'a, E>(
        exec: E,
        chain_codes: &[String],
        is_local: Option<u8>,
        status: Option<u8>,
        network: Option<&str>,
    ) -> Result<Vec<NodeEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT * FROM node WHERE 1 = 1");
        if !chain_codes.is_empty() {
            query.push(" AND chain_code IN (");
            let mut chains = query.separated(", ");
            for chain_code in chain_codes {
                chains.push_bind(chain_code);
            }
            chains.push_unseparated(")");
        }

        if let Some(is_local) = is_local {
            query.push(" AND is_local = ").push_bind(is_local);
        }
        if let Some(status) = status {
            query.push(" AND status = ").push_bind(status);
        }
        if let Some(network) = network {
            if network == "mainnet" {
                query
                    .push(" AND (network = ")
                    .push_bind(network)
                    .push(" OR network IS NULL OR network = '')");
            } else {
                query.push(" AND network = ").push_bind(network);
            }
        }
        query
            .build_query_as::<NodeEntity>()
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn delete<'a, E>(
        exec: E,
        node_id: &str,
        // rpc_url: &str,
        // chain_code: &str,
    ) -> Result<Vec<NodeEntity>, crate::Error>
    where
        E: Executor<'a, Database = Sqlite>,
    {
        let sql = r#"
            DELETE FROM node
            WHERE node_id = $1
            RETURNING *
            "#;

        sqlx::query_as::<sqlx::Sqlite, NodeEntity>(sql)
            // .bind(rpc_url)
            // .bind(chain_code)
            .bind(node_id)
            .fetch_all(exec)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }
}
