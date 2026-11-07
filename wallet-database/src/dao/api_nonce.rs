use sqlx::{Executor, Sqlite};

pub(crate) struct ApiNonceDao;

impl ApiNonceDao {
    pub async fn signed_nonce_decision(
        conn: &mut sqlx::SqliteConnection,
        address: &str,
        chain: &str,
        chain_next: i64,
    ) -> Result<crate::entities::api_nonce_claim::WithdrawNonceDecision, crate::Error> {
        let counter = Self::get_api_nonce_optional(&mut *conn, address, chain).await?;
        let claims = Self::nonce_claims(&mut *conn, address, chain, chain_next, i64::MAX).await?;
        // Completed replacement records are audit facts, not allocation permits.
        let last = claims
            .iter()
            .map(|c| c.nonce)
            .max()
            .unwrap_or(chain_next - 1)
            .max(counter.unwrap_or(chain_next - 1));
        Ok(crate::entities::api_nonce_claim::withdraw_nonce_decision(chain_next, last, &claims))
    }

    pub async fn nonce_claims<'c, E>(
        executor: E,
        address: &str,
        chain: &str,
        first: i64,
        last: i64,
    ) -> Result<Vec<crate::entities::api_nonce_claim::NonceClaim>, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"
          SELECT CASE WHEN trade_type=1 THEN 'withdraw' ELSE 'ordinary' END AS kind,
            nonce,raw_tx,tx_hash,err_code,err_msg,failure_stage,last_broadcast_at,
            (broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL OR broadcast_uncertain_reconciled_at IS NOT NULL) AS uncertain,
            (transaction_time IS NOT NULL OR chain_success_at IS NOT NULL OR chain_failed_at IS NOT NULL) AS executed,
            (err_code IS NOT NULL OR finished_at IS NOT NULL) AS terminal
          FROM api_withdraws WHERE lower(trim(from_addr))=lower(trim($1)) AND chain_code=$2 AND nonce BETWEEN $3 AND $4
            AND (raw_tx IS NOT NULL OR tx_hash IS NOT NULL OR last_broadcast_at IS NOT NULL OR transaction_time IS NOT NULL
             OR broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL)
          UNION ALL
          SELECT 'collect',nonce,raw_tx,tx_hash,err_code,err_msg,NULL,last_broadcast_at,
            (broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL),
            transaction_time IS NOT NULL,(err_code IS NOT NULL OR finished_at IS NOT NULL)
          FROM api_collect WHERE lower(trim(from_addr))=lower(trim($1)) AND chain_code=$2 AND nonce BETWEEN $3 AND $4
            AND (raw_tx IS NOT NULL OR tx_hash IS NOT NULL OR last_broadcast_at IS NOT NULL OR transaction_time IS NOT NULL
             OR broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL)
          UNION ALL
          SELECT 'fee',nonce,raw_tx,tx_hash,err_code,err_msg,NULL,last_broadcast_at,
            (broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL),
            transaction_time IS NOT NULL,(err_code IS NOT NULL OR finished_at IS NOT NULL)
          FROM api_fee WHERE lower(trim(from_addr))=lower(trim($1)) AND chain_code=$2 AND nonce BETWEEN $3 AND $4
            AND (raw_tx IS NOT NULL OR tx_hash IS NOT NULL OR last_broadcast_at IS NOT NULL OR transaction_time IS NOT NULL
             OR broadcast_uncertain_since_at IS NOT NULL OR broadcast_uncertain_retry_count>0
             OR broadcast_uncertain_rebroadcast_count>0 OR broadcast_uncertain_last_checked_at IS NOT NULL)
        "#;
        sqlx::query_as(sql)
            .bind(address)
            .bind(chain)
            .bind(first)
            .bind(last)
            .fetch_all(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))
    }

    pub async fn get_api_nonce<'c, E>(
        executor: E,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        Self::get_api_nonce_optional(executor, from_addr, chain_code)
            .await?
            .ok_or_else(|| crate::Error::Database(sqlx::Error::RowNotFound.into()))
    }

    pub async fn get_api_nonce_optional<'c, E>(
        executor: E,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<Option<i64>, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        // Include historical spelling aliases without modifying their audit rows.
        sqlx::query_scalar("SELECT MAX(nonce) FROM api_nonce WHERE chain_code=$2 AND (from_addr=$1 OR ($3 AND lower(trim(from_addr))=lower(trim($1))))")
            .bind(from_addr).bind(chain_code)
            .bind(crate::entities::api_withdraw::is_signed_evm_chain(chain_code))
            .fetch_one(executor).await.map_err(|e|crate::Error::Database(e.into()))
    }

    /// 单调追平 nonce：只允许将 api_nonce.nonce 提升到 floor_nonce（不会回滚）
    pub async fn upsert_nonce_floor<'c, E>(
        executor: E,
        from_addr: &str,
        chain_code: &str,
        floor_nonce: i64,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"
            Insert into api_nonce
                (from_addr,chain_code,nonce,created_at,updated_at)
            values
                ($1, $2, $3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            on conflict (from_addr,chain_code)
            do update set
                nonce = case
                    when api_nonce.nonce is null then excluded.nonce
                    when api_nonce.nonce < excluded.nonce then excluded.nonce
                    else api_nonce.nonce
                end,
                updated_at = excluded.updated_at
            returning MAX(nonce, COALESCE((SELECT MAX(alias.nonce) FROM api_nonce alias
                WHERE alias.chain_code=$2 AND (alias.from_addr=$1 OR
                  ($4 AND lower(trim(alias.from_addr))=lower(trim($1))))), nonce))
        "#;

        let nonce = sqlx::query_scalar::<_, i64>(sql)
            .bind(crate::entities::api_nonce::nonce_address_key(from_addr, chain_code))
            .bind(chain_code)
            .bind(floor_nonce)
            .bind(crate::entities::api_withdraw::is_signed_evm_chain(chain_code))
            .fetch_one(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(nonce)
    }

    /// Test fixture only: no production force-counter writer.
    #[cfg(test)]
    pub(crate) async fn upsert_nonce_exact<'c, E>(
        executor: E,
        from_addr: &str,
        chain_code: &str,
        exact_nonce: i64,
    ) -> Result<i64, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let sql = r#"
            Insert into api_nonce
                (from_addr,chain_code,nonce,created_at,updated_at)
            values
                ($1, $2, $3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            on conflict (from_addr,chain_code)
            do update set
                nonce = excluded.nonce,
                updated_at = excluded.updated_at
            returning nonce
        "#;

        let nonce = sqlx::query_scalar::<_, i64>(sql)
            .bind(from_addr)
            .bind(chain_code)
            .bind(exact_nonce)
            .fetch_one(executor)
            .await
            .map_err(|e| crate::Error::Database(e.into()))?;

        Ok(nonce)
    }

    /// 分页获取所有 nonce 记录，支持稳定排序和 cursor 分页
    pub async fn get_all_api_nonce_paginated<'c, E>(
        executor: E,
        cursor: Option<(&str, &str)>,
        limit: i32,
    ) -> Result<Vec<(String, String, i64)>, crate::Error>
    where
        E: Executor<'c, Database = Sqlite>,
    {
        let (sql, params) = if let Some((last_addr, last_chain)) = cursor {
            // 使用 cursor 进行分页
            let sql = r#"
                select from_addr, chain_code, nonce
                from api_nonce
                where (from_addr > $1) or (from_addr = $1 and chain_code > $2)
                order by from_addr asc, chain_code asc
                limit $3
            "#;
            (sql, vec![last_addr.to_string(), last_chain.to_string(), limit.to_string()])
        } else {
            // 初始分页
            let sql = r#"
                select from_addr, chain_code, nonce
                from api_nonce
                order by from_addr asc, chain_code asc
                limit $1
            "#;
            (sql, vec![limit.to_string()])
        };

        let mut query = sqlx::query_as::<_, (String, String, i64)>(sql);
        for param in params {
            query = query.bind(param);
        }

        let results =
            query.fetch_all(executor).await.map_err(|e| crate::Error::Database(e.into()))?;

        Ok(results)
    }
}
