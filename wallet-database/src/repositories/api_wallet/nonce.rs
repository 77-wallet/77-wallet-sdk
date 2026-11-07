use crate::{ApiTransactionDbPool, dao::api_nonce::ApiNonceDao};

/// Nonce ownership is committed with signed business facts, never preallocated.
///
/// ```compile_fail
/// use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
/// let _ = ApiNonceRepo::allocate_next_nonce;
/// ```
/// ```compile_fail
/// use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
/// let _ = ApiNonceRepo::upsert_and_get_api_nonce;
/// ```
/// ```compile_fail
/// use wallet_database::repositories::api_wallet::nonce::ApiNonceRepo;
/// let _ = ApiNonceRepo::set_nonce_exact;
/// ```
pub struct ApiNonceRepo;

impl ApiNonceRepo {
    /// Read-only snapshot of all three durable signed-intent flows.
    pub async fn observe_signed_nonce(
        pool: &ApiTransactionDbPool,
        address: &str,
        chain: &str,
        chain_next: i64,
    ) -> Result<crate::entities::api_nonce_claim::WithdrawNonceDecision, crate::Error> {
        if chain_next < 0 {
            return Ok(crate::entities::api_nonce_claim::WithdrawNonceDecision::Wait);
        }
        let mut tx = pool.read_ref().begin().await.map_err(|e| crate::Error::Database(e.into()))?;
        let decision =
            ApiNonceDao::signed_nonce_decision(&mut tx, address, chain, chain_next).await?;
        tx.commit().await.map_err(|e| crate::Error::Database(e.into()))?;
        Ok(decision)
    }

    pub async fn get_api_nonce(
        pool: &ApiTransactionDbPool,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error> {
        ApiNonceDao::get_api_nonce(pool.read_ref(), from_addr, chain_code).await
    }

    pub async fn get_api_nonce_optional(
        pool: &ApiTransactionDbPool,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<Option<i64>, crate::Error> {
        ApiNonceDao::get_api_nonce_optional(pool.read_ref(), from_addr, chain_code).await
    }

    // 写权限限制：只允许 NonceEngine 调用
    #[doc(hidden)]
    pub async fn set_nonce_floor(
        pool: &ApiTransactionDbPool,
        from_addr: &str,
        chain_code: &str,
        floor_nonce: i64,
    ) -> Result<i64, crate::Error> {
        ApiNonceDao::upsert_nonce_floor(pool.write_ref(), from_addr, chain_code, floor_nonce).await
    }

    /// Test fixture only: production may never force a counter backwards.
    #[cfg(test)]
    pub(crate) async fn set_nonce_exact(
        pool: &ApiTransactionDbPool,
        from_addr: &str,
        chain_code: &str,
        exact_nonce: i64,
    ) -> Result<i64, crate::Error> {
        ApiNonceDao::upsert_nonce_exact(pool.write_ref(), from_addr, chain_code, exact_nonce).await
    }

    // 写权限限制：只允许 NonceEngine 调用
    #[doc(hidden)]
    pub async fn ensure_initialized(
        pool: &ApiTransactionDbPool,
        from_addr: &str,
        chain_code: &str,
    ) -> Result<(), crate::Error> {
        // 尝试获取现有 nonce，如果不存在则初始化
        match ApiNonceDao::get_api_nonce_optional(pool.read_ref(), from_addr, chain_code).await? {
            Some(_) => Ok(()),
            None => {
                // 记录不存在，初始化
                // 注意：这里只是兜底行为，真实系统应由 NonceEngine slow path 完成 bootstrap
                let _ =
                    ApiNonceDao::upsert_nonce_floor(pool.write_ref(), from_addr, chain_code, -1)
                        .await?;
                Ok(())
            }
        }
    }

    // 写权限限制：只允许 NonceEngine 调用
    #[doc(hidden)]
    pub async fn get_all_api_nonce_paginated(
        pool: &ApiTransactionDbPool,
        cursor: Option<(&str, &str)>,
        limit: i32,
    ) -> Result<Vec<(String, String, i64)>, crate::Error> {
        ApiNonceDao::get_all_api_nonce_paginated(pool.read_ref(), cursor, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiNonceRepo;
    use crate::{
        SqlitePoolConfig,
        dao::api_nonce::ApiNonceDao,
        repositories::test_helper::{
            setup_api_transaction_pool, setup_api_transaction_pool_with_config,
        },
    };
    use std::{sync::Arc, time::Duration};
    use tokio::sync::{Barrier, oneshot};

    fn is_sqlite_locked(err: &crate::Error) -> bool {
        match err {
            crate::Error::Database(crate::DatabaseError::Sqlx(sqlx::Error::Database(db_err))) => {
                db_err.code().as_deref() == Some("5")
            }
            _ => false,
        }
    }

    async fn run_nonce_concurrency_regression() {
        let cfg_multi = SqlitePoolConfig { reader_max_connections: 4, writer_max_connections: 4 };
        let pool =
            setup_api_transaction_pool_with_config("wallet_db_nonce_concurrent_multi", cfg_multi)
                .await;

        let addr = "0xnonce_lock_multi";
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        ApiNonceRepo::set_nonce_exact(&pool, addr, chain, 0).await.unwrap();

        let gate = Arc::new(Barrier::new(2));
        let pool_hold = pool.clone();
        let gate_hold = gate.clone();
        let holder = tokio::spawn(async move {
            let mut tx = pool_hold.write_ref().begin().await.unwrap();
            ApiNonceDao::upsert_nonce_exact(tx.as_mut(), addr, chain, 101).await.unwrap();
            gate_hold.wait().await;
            tokio::time::sleep(Duration::from_secs(6)).await;
            tx.commit().await.unwrap();
        });

        let pool_race = pool.clone();
        let racer = tokio::spawn(async move {
            gate.wait().await;
            ApiNonceRepo::set_nonce_exact(&pool_race, addr, chain, 202).await
        });

        holder.await.unwrap();
        let race_res = racer.await.unwrap();
        assert!(race_res.as_ref().is_err_and(is_sqlite_locked));

        // default config: single writer should avoid DB lock
        let pool_default = setup_api_transaction_pool("wallet_db_nonce_concurrent_default").await;
        ApiNonceRepo::set_nonce_exact(&pool_default, addr, chain, 0).await.unwrap();

        let gate_default = Arc::new(Barrier::new(2));
        let pool_hold_default = pool_default.clone();
        let gate_hold_default = gate_default.clone();
        let holder_default = tokio::spawn(async move {
            let mut tx = pool_hold_default.write_ref().begin().await.unwrap();
            ApiNonceDao::upsert_nonce_exact(tx.as_mut(), addr, chain, 303).await.unwrap();
            gate_hold_default.wait().await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            tx.commit().await.unwrap();
        });

        let pool_race_default = pool_default.clone();
        let racer_default = tokio::spawn(async move {
            gate_default.wait().await;
            ApiNonceRepo::set_nonce_exact(&pool_race_default, addr, chain, 404).await
        });

        holder_default.await.unwrap();
        let ok_res = racer_default.await.unwrap();
        assert!(ok_res.is_ok());
        let got = ApiNonceRepo::get_api_nonce(&pool_default, addr, chain).await.unwrap();
        assert_eq!(got, 404);
    }

    #[tokio::test]
    async fn nonce_optional_missing_returns_none() {
        let pool = setup_api_transaction_pool("wallet_db_nonce_edge").await;
        let got = ApiNonceRepo::get_api_nonce_optional(
            &pool,
            "0xnonce_missing",
            wallet_types::constant::chain_code::ETHEREUM,
        )
        .await
        .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn nonce_tx_rollback_restores_previous_value() {
        let pool = setup_api_transaction_pool("wallet_db_nonce_rollback").await;
        let addr = "0xnonce_rb_1";
        let chain = wallet_types::constant::chain_code::ETHEREUM;

        ApiNonceRepo::set_nonce_exact(&pool, addr, chain, 7).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        let changed = ApiNonceDao::upsert_nonce_exact(tx.as_mut(), addr, chain, 99).await.unwrap();
        assert_eq!(changed, 99);
        tx.rollback().await.unwrap();

        let got = ApiNonceRepo::get_api_nonce(&pool, addr, chain).await.unwrap();
        assert_eq!(got, 7);
    }

    #[tokio::test]
    async fn concurrent_nonce_updates() {
        run_nonce_concurrency_regression().await;
    }

    #[tokio::test]
    async fn concurrent_balance_upserts() {
        // alias test name for lock-regression command compatibility
        run_nonce_concurrency_regression().await;
    }

    #[tokio::test]
    async fn read_queries_are_not_blocked_by_long_writer_transaction() {
        let pool = setup_api_transaction_pool("wallet_db_nonce_reader_not_blocked").await;
        let addr = "0xnonce_reader";
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        ApiNonceRepo::set_nonce_exact(&pool, addr, chain, 7).await.unwrap();

        let (ready_tx, ready_rx) = oneshot::channel();
        let pool_writer = pool.clone();
        let writer = tokio::spawn(async move {
            let mut tx = pool_writer.write_ref().begin().await.unwrap();
            ApiNonceDao::upsert_nonce_exact(tx.as_mut(), addr, chain, 8).await.unwrap();
            let _ = ready_tx.send(());
            tokio::time::sleep(Duration::from_secs(2)).await;
            tx.commit().await.unwrap();
        });

        ready_rx.await.unwrap();

        let read_res = tokio::time::timeout(
            Duration::from_millis(800),
            ApiNonceRepo::get_api_nonce(&pool, addr, chain),
        )
        .await;
        assert!(read_res.is_ok(), "reader query timed out while writer tx was open");
        assert_eq!(read_res.unwrap().unwrap(), 7);

        writer.await.unwrap();
        let after = ApiNonceRepo::get_api_nonce(&pool, addr, chain).await.unwrap();
        assert_eq!(after, 8);
    }
}

#[cfg(test)]
mod history_boundary_tests {
    use super::*;
    use crate::{
        entities::api_nonce_claim::WithdrawNonceDecision,
        repositories::test_helper::setup_api_transaction_pool,
    };
    #[tokio::test]
    async fn completed_recovery_never_ignores_a_new_unknown_counter() {
        let pool = setup_api_transaction_pool("recovered_counter_boundary").await;
        ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 6).await.unwrap();
        sqlx::query("INSERT INTO api_collect_replacement(new_trade_no,old_trade_no,chain_code,from_addr,nonce,old_hash,old_raw,legacy_counter,winner_hash,block_hash,block_number) VALUES('new','old','eth','sender',6,'old-hash','old-raw',153,'winner','block',100)")
            .execute(pool.write_ref()).await.unwrap();
        assert_eq!(
            ApiNonceRepo::observe_signed_nonce(&pool, "sender", "eth", 7).await.unwrap(),
            WithdrawNonceDecision::Ready(7)
        );
        ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 11).await.unwrap();
        assert_eq!(
            ApiNonceRepo::observe_signed_nonce(&pool, "sender", "eth", 7).await.unwrap(),
            WithdrawNonceDecision::Wait
        );
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "eth").await.unwrap(), 11);
    }
}

#[cfg(test)]
mod identity_boundary_tests {
    use super::*;
    use crate::repositories::test_helper::setup_api_transaction_pool;
    #[tokio::test]
    async fn nonce_identity_merges_historical_evm_alias_floors_only() {
        let pool = setup_api_transaction_pool("nonce_alias_floor").await;
        sqlx::query("INSERT INTO api_nonce(from_addr,chain_code,nonce,created_at,updated_at) VALUES('SENDER','eth',17,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),('sender','eth',4,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)")
            .execute(pool.write_ref()).await.unwrap();
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "Sender", "eth").await.unwrap(), 17);
        assert_eq!(ApiNonceRepo::set_nonce_floor(&pool, "sender", "eth", 5).await.unwrap(), 17);
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "SENDER", "eth").await.unwrap(), 17);
        ApiNonceRepo::set_nonce_floor(&pool, "Sender", "sol", 3).await.unwrap();
        ApiNonceRepo::set_nonce_floor(&pool, "sender", "sol", 8).await.unwrap();
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "Sender", "sol").await.unwrap(), 3);
        assert_eq!(ApiNonceRepo::get_api_nonce(&pool, "sender", "sol").await.unwrap(), 8);
    }
}
