#[path = "support/concurrency.rs"]
mod concurrency;
#[path = "support/mixed_scale.rs"]
mod mixed_scale;
#[path = "support/normal_scale.rs"]
mod normal_scale;
#[path = "support/performance.rs"]
mod performance;
#[path = "support/scale.rs"]
mod scale;
#[path = "support/wal.rs"]
mod wal;
pub use normal_scale::normal_scale_parameters;
use performance::DeletionPhase;
pub use performance::deletion_phase_trace;

use crate::harness::{self, ApiWalletBackendCall, TestEnv, WalletPair};
use wallet_database::{
    entities::{api_account::CreateApiAccountVo, api_wallet::ApiWalletType},
    repositories::api_wallet::{account::ApiAccountRepo, wallet::ApiWalletRepo},
};

pub struct DeletionScenario {
    env: &'static TestEnv,
    pair: WalletPair,
    other_uid: String,
    other_address: String,
    other_accounts: usize,
    inactive_addresses: Vec<String>,
}

impl DeletionScenario {
    pub async fn given() -> Self {
        let env = harness::ensure_env().await;
        harness::reset_fake(env);
        let pair = harness::prepare_wallet_pair(env).await;
        let pool = harness::open_api_wallet_pool(&env.db_dir).await;
        ApiWalletRepo::bind_withdraw_and_subaccount_relation(
            &pool,
            &pair.recharge_address,
            &pair.withdrawal_address,
        )
        .await
        .unwrap();
        let other_uid = harness::next_tag("deletion-other");
        let other_address = harness::upsert_wallet(
            &env.db_dir,
            &env.sn,
            &other_uid,
            ApiWalletType::SubAccount,
            None,
        )
        .await;
        for (address, uid, kind) in [
            (&pair.recharge_address, &pair.recharge_uid, ApiWalletType::SubAccount),
            (&pair.withdrawal_address, &pair.withdrawal_uid, ApiWalletType::Withdrawal),
            (&other_address, &other_uid, ApiWalletType::SubAccount),
        ] {
            ApiAccountRepo::upsert_account_multi(
                &pool,
                vec![CreateApiAccountVo::new(
                    0,
                    address,
                    "fixture-public-key",
                    address,
                    uid,
                    "m/44'/60'/0'/0/0",
                    0,
                    "base",
                    "delete-fixture",
                    kind,
                )],
            )
            .await
            .unwrap();
        }
        let mut inactive_addresses = Vec::new();
        for (address, uid, kind) in [
            (&pair.recharge_address, &pair.recharge_uid, ApiWalletType::SubAccount),
            (&pair.withdrawal_address, &pair.withdrawal_uid, ApiWalletType::Withdrawal),
        ] {
            let inactive = format!("0x{:040x}", harness::next_unique_id() + 1_000_000);
            ApiAccountRepo::upsert_account_multi(
                &pool,
                vec![CreateApiAccountVo::new(
                    1,
                    &inactive,
                    "fixture-public-key",
                    address,
                    uid,
                    "m/44'/60'/0'/0/1",
                    1,
                    "base",
                    "inactive-delete-fixture",
                    kind,
                )],
            )
            .await
            .unwrap();
            sqlx::query("UPDATE api_account SET status=0 WHERE address=?")
                .bind(&inactive)
                .execute(pool.write_ref())
                .await
                .unwrap();
            inactive_addresses.push(inactive);
        }
        Self { env, pair, other_uid, other_address, other_accounts: 1, inactive_addresses }
    }

    pub async fn given_all_addresses_belong_to_pair(&mut self) {
        // Keep the other wallet identity for cross-wallet probes, but no addresses.
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let removed = sqlx::query("DELETE FROM api_account WHERE wallet_address=?")
            .bind(&self.other_address)
            .execute(pool.write_ref())
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(removed, self.other_accounts as u64);
        let outside: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_account WHERE wallet_address NOT IN (?,?)",
        )
        .bind(&self.pair.recharge_address)
        .bind(&self.pair.withdrawal_address)
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        assert_eq!(outside, 0, "exclusive fixture requires a fresh process inventory");
        self.other_accounts = 0;
    }

    pub fn given_backend_fails(&self) {
        self.env.fake_backend.set_device_delete_error(Some("delete backend unavailable"));
    }

    pub fn given_backend_recovers(&self) {
        self.env.fake_backend.set_device_delete_timeout(false);
        self.env.fake_backend.set_device_delete_error(None);
    }

    pub async fn given_cleanup_persistence_fails(&self) {
        sqlx::query("CREATE TRIGGER fail_deletion_cleanup BEFORE INSERT ON task_queue BEGIN SELECT RAISE(FAIL,'cleanup persistence failed'); END")
            .execute(self.task_pool().await.write_ref()).await.unwrap();
    }

    pub async fn given_cleanup_persistence_recovers(&self) {
        sqlx::query("DROP TRIGGER fail_deletion_cleanup")
            .execute(self.task_pool().await.write_ref())
            .await
            .unwrap();
    }

    pub async fn given_partner_points_elsewhere(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        sqlx::query("UPDATE api_wallet SET binding_address=? WHERE address=?")
            .bind(&self.other_address)
            .bind(&self.pair.withdrawal_address)
            .execute(pool.write_ref())
            .await
            .unwrap();
    }

    async fn task_pool(&self) -> wallet_database::TaskDbPool {
        wallet_database::SqliteContext::new(&self.env.db_dir.to_string_lossy(), Some("task.db"))
            .await
            .unwrap()
            .into_task_db_pool()
            .unwrap()
    }

    pub async fn given_device_uid_write_fails(&self) {
        let pool = harness::open_core_pool(&self.env.db_dir).await;
        wallet_database::repositories::device::DeviceRepo::update_uid(
            pool.clone(),
            &self.env.sn,
            Some(&self.pair.recharge_uid),
        )
        .await
        .unwrap();
        sqlx::query("CREATE TRIGGER fail_delete_device_uid BEFORE UPDATE OF uid ON device BEGIN SELECT RAISE(FAIL,'device UID update failed'); END")
            .execute(pool.write_ref()).await.unwrap();
    }

    pub async fn given_device_uid_write_recovers(&self) {
        let pool = harness::open_core_pool(&self.env.db_dir).await;
        sqlx::query("DROP TRIGGER fail_delete_device_uid").execute(pool.write_ref()).await.unwrap();
    }

    pub async fn then_device_uid_references_surviving_wallet(&self) {
        let core_pool = harness::open_core_pool(&self.env.db_dir).await;
        let device = wallet_database::repositories::device::DeviceRepo::get_device_info(
            core_pool,
            &self.env.sn,
        )
        .await
        .unwrap()
        .unwrap();
        let uid = device.uid.unwrap();
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        assert!(
            ApiWalletRepo::find_by_uid(&pool, &uid).await.unwrap().is_some(),
            "repeat delete must finish device cleanup after local commit"
        );
    }

    pub async fn when_delete_subaccount(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        self.env.manager.physical_delete_api_wallet(&self.pair.recharge_address).await
    }

    pub async fn then_pair_and_accounts_present(&self) {
        self.assert_pair_and_accounts_present().await;
        assert_eq!(self.cleanup_tasks().await.len(), 0);
    }

    pub async fn then_pair_survives_local_rollback(&self) {
        self.assert_pair_and_accounts_present().await;
        // Backend success already persisted cleanup before the guarded local
        // delete. Keep that retry fact; local rollback must not erase it.
        assert_eq!(self.cleanup_tasks().await.len(), 1);
    }

    async fn assert_pair_and_accounts_present(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address] {
            assert!(
                ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some(),
                "backend failure must retain wallet {address} for retry"
            );
            assert_eq!(
                ApiAccountRepo::list_by_wallet_address(&pool, address, None, None)
                    .await
                    .unwrap()
                    .len(),
                1
            );
        }
        self.then_other_unchanged().await;
    }

    pub async fn then_pair_deleted_and_other_unchanged(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address] {
            assert!(
                ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_none(),
                "paired wallet {address} must also be deleted"
            );
            assert!(
                ApiAccountRepo::list_by_wallet_address(&pool, address, None, None)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        self.then_other_unchanged().await;
        let tasks = self.cleanup_tasks().await;
        assert_eq!(tasks.len(), 1, "cleanup authority must survive local deletion");
        let addresses = tasks[0]["body"]["address"].as_array().unwrap();
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address]
            .into_iter()
            .chain(self.inactive_addresses.iter())
        {
            assert!(
                addresses.iter().any(|item| item["address"] == *address),
                "cleanup task must retain every active/inactive address"
            );
        }
        assert_eq!(addresses.len(), 4);
    }

    async fn then_other_unchanged(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        assert!(
            ApiWalletRepo::find_by_address(&pool, &self.other_address).await.unwrap().is_some()
        );
        assert_eq!(
            ApiAccountRepo::list_by_wallet_address(&pool, &self.other_address, None, None)
                .await
                .unwrap()
                .len(),
            self.other_accounts
        );
    }

    pub fn then_backend_calls(&self, expected: usize) {
        self.env.fake_backend.with_calls(|calls| {
            let calls: Vec<_> = calls
                .iter()
                .filter_map(|call| match call {
                    ApiWalletBackendCall::DeviceDelete(call) => Some(call),
                    _ => None,
                })
                .collect();
            assert_eq!(calls.len(), expected, "backend cleanup must be retried after failure");
            for call in calls {
                assert!(call.uid_list.contains(&self.other_uid));
                assert!(!call.uid_list.contains(&self.pair.recharge_uid));
                assert!(!call.uid_list.contains(&self.pair.withdrawal_uid));
            }
        });
    }

    async fn cleanup_tasks(&self) -> Vec<serde_json::Value> {
        wallet_database::repositories::task_queue::TaskQueueRepo::all_tasks_queue(
            &self.task_pool().await,
        )
        .await
        .unwrap()
        .into_iter()
        .filter_map(|task| {
            let body: serde_json::Value = serde_json::from_str(&task.request_body).ok()?;
            let addresses = body["body"]["address"].as_array()?;
            addresses
                .iter()
                .any(|item| item["address"] == self.pair.recharge_address)
                .then_some(body)
        })
        .collect()
    }
}
