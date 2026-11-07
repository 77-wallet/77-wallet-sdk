use super::DeletionScenario;
use crate::harness::{self, ApiWalletBackendCall};
use wallet_database::{
    entities::{api_account::CreateApiAccountVo, api_wallet::ApiWalletType},
    repositories::api_wallet::{account::ApiAccountRepo, wallet::ApiWalletRepo},
};

impl DeletionScenario {
    pub async fn when_account_arrives_during_backend(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        // Runtime writers reuse initialized pools; opening SqliteContext here
        // would run startup WAL truncate, which is not an account-write action.
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        self.env.fake_backend.gate_next_device_delete(gate.clone());
        let add = async {
            gate.wait().await;
            ApiAccountRepo::upsert_account_multi(
                &pool,
                vec![CreateApiAccountVo::new(
                    2,
                    &format!("late-{}", self.pair.recharge_uid),
                    "pub",
                    &self.pair.recharge_address,
                    &self.pair.recharge_uid,
                    "late-path",
                    2,
                    "base",
                    "late-account",
                    ApiWalletType::SubAccount,
                )],
            )
            .await
            .unwrap();
            gate.wait().await;
        };
        let (result, ()) = wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(10),
            async { tokio::join!(self.when_delete_subaccount(), add) },
        )
        .await
        .expect("controlled backend/account interleaving timed out");
        result
    }

    pub async fn when_binding_changes_during_backend(
        &self,
    ) -> Result<(), wallet_api::error::service::ServiceError> {
        // Prepare fixture resources before the snapshot; actual rebind writes
        // remain inside the gated backend window and still must survive rollback.
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        let replacement = harness::upsert_wallet(
            &self.env.db_dir,
            &self.env.sn,
            &harness::next_tag("replacement"),
            ApiWalletType::Withdrawal,
            None,
        )
        .await;
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        self.env.fake_backend.gate_next_device_delete(gate.clone());
        let change = async {
            gate.wait().await;
            ApiWalletRepo::clear_binding_relation(
                &pool,
                &self.pair.recharge_address,
                &self.pair.withdrawal_address,
            )
            .await
            .unwrap();
            ApiWalletRepo::bind_withdraw_and_subaccount_relation(
                &pool,
                &self.pair.recharge_address,
                &replacement,
            )
            .await
            .unwrap();
            gate.wait().await;
            replacement
        };
        let (result, replacement) = wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(10),
            async { tokio::join!(self.when_delete_subaccount(), change) },
        )
        .await
        .expect("controlled rebinding timed out");
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        assert_eq!(
            ApiWalletRepo::find_by_address(&pool, &replacement)
                .await
                .unwrap()
                .unwrap()
                .binding_address
                .as_deref(),
            Some(self.pair.recharge_address.as_str())
        );
        result
    }

    pub async fn when_public_rebinding_waits_for_delete(&self) {
        let gate = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        self.env.fake_backend.gate_next_device_delete(gate.clone());
        let change = async {
            gate.wait().await;
            let attempt = self
                .env
                .manager
                .change_withdrawal_wallet(&self.pair.recharge_uid, "missing-target");
            tokio::pin!(attempt);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut attempt)
                    .await
                    .is_err(),
                "public rebinding must wait behind deletion before reading or calling backend"
            );
            gate.wait().await;
            assert!(attempt.await.is_err(), "deleted origin cannot be rebound");
        };
        let (result, ()) = wallet_api::infrastructure::runtime::time::with_timeout(
            std::time::Duration::from_secs(10),
            async { tokio::join!(self.when_delete_subaccount(), change) },
        )
        .await
        .expect("controlled public rebinding timed out");
        result.unwrap();
        self.env.fake_backend.with_calls(|calls| {
            assert!(
                !calls.iter().any(|call| matches!(
                    call,
                    ApiWalletBackendCall::AppIdWithdrawalWalletChange(_)
                ))
            )
        });
    }

    pub async fn then_late_account_and_pair_survive(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address] {
            assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_some());
        }
        let accounts =
            ApiAccountRepo::list_by_wallet_address(&pool, &self.pair.recharge_address, None, None)
                .await
                .unwrap();
        assert!(
            accounts.iter().any(|row| row.account_id == 2),
            "address absent from cleanup snapshot must not be lost"
        );
        self.then_other_unchanged().await;
    }

    pub async fn then_retry_retains_cleanup_for_late_account(&self) {
        let pool = harness::open_api_wallet_pool(&self.env.db_dir).await;
        for address in [&self.pair.recharge_address, &self.pair.withdrawal_address] {
            assert!(ApiWalletRepo::find_by_address(&pool, address).await.unwrap().is_none());
            assert!(
                ApiWalletRepo::accounts_for_deletion(&pool, &[address]).await.unwrap().is_empty()
            );
        }
        let late = format!("late-{}", self.pair.recharge_uid);
        assert!(
            self.cleanup_tasks().await.iter().any(|body| {
                body["body"]["address"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["address"] == late)
            }),
            "retry must durably include the newly observed address"
        );
        self.then_other_unchanged().await;
    }
}
