use super::DeletionScenario;
use crate::harness;

impl DeletionScenario {
    async fn prepare_wal_probe(&self) -> wallet_database::ApiWalletDbPool {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS deletion_wal_probe(id INTEGER PRIMARY KEY,payload BLOB)",
        )
        .execute(pool.write_ref())
        .await
        .unwrap();
        sqlx::query("INSERT OR REPLACE INTO deletion_wal_probe VALUES(1,zeroblob(65536))")
            .execute(pool.write_ref())
            .await
            .unwrap();
        let result: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(pool.write_ref())
            .await
            .unwrap();
        assert_eq!(result, (0, 0, 0));
        pool
    }

    async fn wal_pressure(&self, pool: &wallet_database::ApiWalletDbPool) -> (i64, i64, i64) {
        for byte in 0..64u8 {
            sqlx::query("UPDATE deletion_wal_probe SET payload=? WHERE id=1")
                .bind(vec![byte; 65536])
                .execute(pool.write_ref())
                .await
                .unwrap();
        }
        let result: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)")
            .fetch_one(pool.write_ref())
            .await
            .unwrap();
        let databases: Vec<(i64, String, String)> =
            sqlx::query_as("PRAGMA database_list").fetch_all(pool.write_ref()).await.unwrap();
        let file = &databases.iter().find(|row| row.1 == "main").unwrap().2;
        let bytes = std::fs::metadata(format!("{file}-wal")).unwrap().len();
        println!(
            "WALLET-2980 backend-wait WAL bytes={bytes} log_frames={} checkpointed={}",
            result.1, result.2
        );
        result
    }

    async fn assert_wal_released(&self, pool: &wallet_database::ApiWalletDbPool) {
        wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(2),
            async {
                loop {
                    let progress: (i64, i64, i64) =
                        sqlx::query_as("PRAGMA wal_checkpoint(PASSIVE)")
                            .fetch_one(pool.write_ref())
                            .await
                            .unwrap();
                    if progress.1 == progress.2 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            },
        )
        .await
        .expect("cancelled/failed deletion must release WAL snapshot");
        let result: (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_one(pool.write_ref())
            .await
            .unwrap();
        assert_eq!(result, (0, 0, 0));
        let databases: Vec<(i64, String, String)> =
            sqlx::query_as("PRAGMA database_list").fetch_all(pool.write_ref()).await.unwrap();
        let file = &databases.iter().find(|row| row.1 == "main").unwrap().2;
        assert_eq!(std::fs::metadata(format!("{file}-wal")).unwrap().len(), 0);
        println!("WALLET-2980 backend-finished WAL bytes=0");
    }

    pub async fn when_backend_timeout_releases_snapshot(&self) {
        let pool = self.prepare_wal_probe().await;
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        self.env.fake_backend.set_device_delete_timeout(true);
        self.env.fake_backend.gate_next_device_delete(gate.clone());
        let env = self.env;
        let address = self.pair.recharge_address.clone();
        let deletion =
            tokio::spawn(async move { env.manager.physical_delete_api_wallet(&address).await });
        wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(5),
            gate.wait(),
        )
        .await
        .unwrap();
        let held = self.wal_pressure(&pool).await;
        gate.wait().await;
        let error = deletion.await.unwrap().unwrap_err();
        assert!(matches!(
            error,
            wallet_api::error::service::ServiceError::TransportBackend(
                wallet_transport_backend::Error::Timeout
            )
        ));
        self.assert_wal_released(&pool).await;
        assert!(held.1 > held.2, "backend wait must retain the pre-backend snapshot");
    }

    pub async fn when_caller_cancels_releases_snapshot(&self, abort: bool) {
        let pool = self.prepare_wal_probe().await;
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        self.env.fake_backend.gate_next_device_delete(gate.clone());
        let held = if abort {
            let env = self.env;
            let address = self.pair.recharge_address.clone();
            let deletion =
                tokio::spawn(async move { env.manager.physical_delete_api_wallet(&address).await });
            wallet_api::infrastructure::runtime::time::with_timeout(
                std::time::Duration::from_secs(5),
                gate.wait(),
            )
            .await
            .unwrap();
            let held = self.wal_pressure(&pool).await;
            deletion.abort();
            assert!(deletion.await.unwrap_err().is_cancelled());
            held
        } else {
            let mut deletion = Box::pin(self.when_delete_subaccount());
            tokio::select! {
                _ = gate.wait() => {},
                result = &mut deletion => panic!("deletion ended before controlled timeout: {result:?}"),
            }
            let held = self.wal_pressure(&pool).await;
            assert!(
                wallet_api::infrastructure::runtime::time::with_timeout(
                    std::time::Duration::from_millis(25),
                    deletion
                )
                .await
                .is_err()
            );
            held
        };
        self.assert_wal_released(&pool).await;
        assert!(held.1 > held.2, "cancelled operation must have held a real read snapshot");
    }
}
