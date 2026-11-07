use super::DeletionScenario;
use crate::harness;
use wallet_database::repositories::api_wallet::wallet::ApiWalletRepo;

impl DeletionScenario {
    pub async fn given_many_addresses(&self, count: usize) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let mut tx = pool.write_ref().begin().await.unwrap();
        sqlx::query("WITH RECURSIVE n(x) AS (VALUES(2) UNION ALL SELECT x+1 FROM n WHERE x < ?+1) INSERT INTO api_account (account_id,name,address,pubkey,address_type,wallet_address,uid,derivation_path,derivation_path_index,chain_code,api_wallet_type,status,is_init,is_expand,is_used,created_at) SELECT x,'scale',printf('scale-%s-%040d',?,x),'public','',?,?,'path',x,'base',1,x%2,0,0,0,'2026-10-30T00:00:00Z' FROM n")
            .bind(count as i64).bind(&self.pair.recharge_uid)
            .bind(&self.pair.recharge_address).bind(&self.pair.recharge_uid)
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }

    pub async fn given_long_addresses(&self, bytes: usize) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        sqlx::query(
            "UPDATE api_account SET address=address||? WHERE wallet_address=? AND account_id>=2",
        )
        .bind("\"\\".repeat(bytes / 2))
        .bind(&self.pair.recharge_address)
        .execute(pool.write_ref())
        .await
        .unwrap();
    }

    pub async fn given_cleanup_persistence_fails_after_16_tasks(&self) {
        let pattern = format!("%scale-{}-%", self.pair.recharge_uid);
        let sql = format!(
            "CREATE TRIGGER fail_deletion_cleanup BEFORE INSERT ON task_queue WHEN NEW.request_body LIKE '{pattern}' AND (SELECT COUNT(*) FROM task_queue WHERE request_body LIKE '{pattern}')>=16 BEGIN SELECT RAISE(FAIL,'second cleanup persistence failed'); END"
        );
        sqlx::query(&sql).execute(self.task_pool().await.write_ref()).await.unwrap();
    }

    pub async fn then_large_pair_present_without_cleanup(&self, expected: i64) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address IN (?,?)")
                .bind(&self.pair.recharge_address)
                .bind(&self.pair.withdrawal_address)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(count, expected);
        assert!(
            ApiWalletRepo::find_by_address(&pool, &self.pair.recharge_address)
                .await
                .unwrap()
                .is_some()
        );
        assert!(self.cleanup_tasks().await.is_empty());
    }

    pub async fn then_large_pair_survives_partial_persistence(&self, expected: i64) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address IN (?,?)")
                .bind(&self.pair.recharge_address)
                .bind(&self.pair.withdrawal_address)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
        assert_eq!(remaining, expected);
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address] {
            assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some());
        }
        let persisted: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM task_queue WHERE request_body LIKE ?")
                .bind(format!("%scale-{}-%", self.pair.recharge_uid))
                .fetch_one(self.task_pool().await.read_ref())
                .await
                .unwrap();
        assert_eq!(persisted, 16, "previous16 durable tasks survive the next write failure");
        assert_eq!(self.cleanup_tasks().await.len(), 1, "first bounded cleanup survives failure");
        let recovered =
            wallet_database::repositories::task_queue::TaskQueueRepo::recoverable_default(
                &self.task_pool().await,
            )
            .await
            .unwrap();
        assert!(
            recovered.iter().any(|row| row.request_body.contains(&self.pair.recharge_address)),
            "reopened queue must rediscover persisted cleanup"
        );
    }

    pub async fn then_bounded_cleanup_covers_addresses(&self, expected: usize) {
        // Stream persisted payloads to keep the verifier itself bounded at scale.
        let pool = self.task_pool().await;
        let mut cursor = String::new();
        let mut total = 0;
        let mut requests = 0;
        sqlx::query("DROP TABLE IF EXISTS wallet2980_coverage")
            .execute(pool.write_ref())
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE wallet2980_coverage(chain TEXT,address TEXT,PRIMARY KEY(chain,address))",
        )
        .execute(pool.write_ref())
        .await
        .unwrap();
        loop {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT id,request_body FROM task_queue WHERE id>? ORDER BY id LIMIT 1",
            )
            .bind(&cursor)
            .fetch_optional(pool.read_ref())
            .await
            .unwrap();
            let Some((id, body)) = row else { break };
            cursor = id;
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            let Some(entries) = body["body"]["address"].as_array() else { continue };
            let owned: Vec<_> = entries
                .iter()
                .filter(|entry| {
                    entry["address"].as_str().is_some_and(|a| {
                        a.starts_with(&format!("scale-{}-", self.pair.recharge_uid))
                            || a == self.pair.recharge_address
                            || a == self.pair.withdrawal_address
                            || self.inactive_addresses.iter().any(|i| i == a)
                    })
                })
                .collect();
            if owned.is_empty() {
                continue;
            }
            assert!(entries.len() <= 1000, "oversized cleanup request: {}", entries.len());
            assert!(serde_json::to_vec(&body["body"]).unwrap().len() <= 128 * 1024);
            assert_eq!(owned.len(), entries.len(), "unrelated address in cleanup");
            let inserted = sqlx::query("INSERT OR IGNORE INTO wallet2980_coverage SELECT json_extract(value,'$.chainCode'),json_extract(value,'$.address') FROM json_each(?,'$.address')")
                .bind(serde_json::to_string(&body["body"]).unwrap()).execute(pool.write_ref()).await.unwrap().rows_affected();
            total += inserted as usize;
            requests += 1;
        }
        println!("WALLET-2980 unique_cleanup_addresses={total} bounded_requests={requests}");
        assert_eq!(total, expected, "all active and inactive addresses must remain durable");
        assert!(requests >= expected.div_ceil(1000));
        sqlx::query("DROP TABLE wallet2980_coverage").execute(pool.write_ref()).await.unwrap();
        let api_pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_account WHERE wallet_address IN (?,?)")
                .bind(&self.pair.recharge_address)
                .bind(&self.pair.withdrawal_address)
                .fetch_one(api_pool.read_ref())
                .await
                .unwrap();
        assert_eq!(remaining, 0);
        self.then_other_unchanged().await;
    }
}
