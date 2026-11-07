use crate::harness::{self, ApiWalletBackendCall, TestEnv};
use std::sync::Arc;
use tokio::{sync::Barrier, task::JoinHandle};
use wallet_api::{
    error::{
        business::{BusinessError, wallet::WalletError},
        service::ServiceError,
    },
    infrastructure::phrase_package::PhrasePackageCodec,
};
use wallet_database::{
    ApiWalletDbPool,
    entities::api_wallet::ApiWalletType,
    repositories::{api_wallet::wallet::ApiWalletRepo, device::DeviceRepo},
};
use wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus;
const PHRASE: &str =
    "";

pub struct Scenario {
    env: &'static TestEnv,
    pool: ApiWalletDbPool,
    password: &'static str,
}
impl Scenario {
    pub async fn given() -> Self {
        let env = harness::ensure_env().await;
        let core = harness::open_core_pool(&env.db_dir).await;
        wallet_database::repositories::config::ConfigRepo::upsert(
            wallet_database::entities::config::config_key::KEYSTORE_KDF_ALGORITHM,
            &wallet_database::entities::config::KeystoreKdfAlgorithm {
                keystore_kdf_algorithm: wallet_crypto::KdfAlgorithm::Argon2id,
            }
            .to_json_str()
            .unwrap(),
            Some(0),
            &core,
        )
        .await
        .unwrap();
        let result = Self {
            env,
            pool: harness::open_api_wallet_pool(&env.db_dir).await,
            password: harness::SMOKE_WALLET_PASSWORD,
        };
        result
    }
    pub async fn when_same_password_first_creations_overlap(&self) {
        self.backend_accepts();
        let gate = Arc::new(Barrier::new(2));
        self.env.fake_backend.gate_next_keys_uid_check(gate.clone());
        let (first_uid, first) = self.spawn_creation(ApiWalletType::SubAccount, self.password);
        gate.wait().await;
        // The first request has made its proof, but no wallet yet exists.
        assert!(ApiWalletRepo::list(&self.pool, None).await.unwrap().is_empty());
        self.env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
        self.env.fake_backend.enqueue_query_uid_bind_info("", "", false, &self.env.sn);
        let (second_uid, second) = self.spawn_creation(ApiWalletType::SubAccount, self.password);
        assert!(second.await.unwrap().is_ok());
        gate.wait().await;
        assert!(
            first.await.unwrap().is_ok(),
            "same-password first creation must not be falsely rejected"
        );
        assert!(ApiWalletRepo::find_by_uid(&self.pool, &first_uid).await.unwrap().is_some());
        assert!(ApiWalletRepo::find_by_uid(&self.pool, &second_uid).await.unwrap().is_some());
        self.then_wallets_readable("synthetic-wrong-password").await;
        println!("CREDENTIAL_RACE same_password_empty_inventory_concurrency_succeeds=true");
    }
    fn backend_accepts(&self) {
        harness::reset_fake(self.env);
        self.env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
        self.env.fake_backend.enqueue_query_uid_bind_info("", "", false, &self.env.sn);
    }
    fn spawn_creation(
        &self,
        kind: ApiWalletType,
        password: &'static str,
    ) -> (String, JoinHandle<Result<String, ServiceError>>) {
        let salt = harness::next_tag("credential-race");
        let uid = harness::derive_uid(PHRASE, &salt);
        let env = self.env;
        (
            uid,
            tokio::spawn(async move {
                env.manager
                    .create_api_wallet(
                        1,
                        PHRASE,
                        &salt,
                        "synthetic-race",
                        password,
                        None,
                        kind,
                        None,
                    )
                    .await
            }),
        )
    }
    async fn create(&self, kind: ApiWalletType) -> String {
        self.backend_accepts();
        let (uid, task) = self.spawn_creation(kind, self.password);
        let result = task.await.unwrap();
        assert!(result.is_ok(), "fresh creation succeeds");
        assert!(result.ok().unwrap() == uid);
        uid
    }
    async fn rotate_to(&mut self, next: &'static str) {
        assert!(
            self.env.manager.set_all_password(self.password, next).await.is_ok(),
            "global rotation succeeds"
        );
        assert!(self.env.manager.validate_password(next).await.is_ok());
        assert!(self.env.manager.validate_password(self.password).await.is_err());
        self.password = next;
    }
    pub async fn when_rotation_overtakes_creation(
        &mut self,
        kind: ApiWalletType,
        next: &'static str,
    ) {
        self.backend_accepts();
        let gate = Arc::new(Barrier::new(2));
        self.env.fake_backend.gate_next_keys_uid_check(gate.clone());
        let (uid, creation) = self.spawn_creation(kind, self.password);
        gate.wait().await;
        assert!(ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().is_none());
        let old = self.password;
        self.rotate_to(next).await;
        let core = harness::open_core_pool(&self.env.db_dir).await;
        let proof = DeviceRepo::get_device_info(core.clone(), &self.env.sn)
            .await
            .unwrap()
            .unwrap()
            .password_proof;
        gate.wait().await;
        let result = creation.await.unwrap();
        assert!(
            matches!(
                result,
                Err(ServiceError::Business(BusinessError::Wallet(WalletError::PasswordIncorrect)))
            ),
            "stale creation must return PasswordIncorrect"
        );
        assert!(
            ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().is_none(),
            "stale creation must not persist a wallet"
        );
        self.env.fake_backend.with_calls(|calls| {
            assert_eq!(calls.len(), 1, "stale creation stops before init/keys/binding mutations");
            assert!(matches!(calls[0], ApiWalletBackendCall::KeysUidCheck { .. }));
        });
        assert!(
            DeviceRepo::get_device_info(core, &self.env.sn).await.unwrap().unwrap().password_proof
                == proof
        );
        self.then_wallets_readable(old).await;
        // A newly submitted old-password request must also fail before HTTP.
        self.backend_accepts();
        let (_, task) = self.spawn_creation(kind, old);
        assert!(matches!(
            task.await.unwrap(),
            Err(ServiceError::Business(BusinessError::Wallet(WalletError::PasswordIncorrect)))
        ));
        self.env.fake_backend.with_calls(|calls| assert!(calls.is_empty()));
        println!(
            "CREDENTIAL_RACE kind={kind:?} stale_rejected=true no_wallet=true no_mutation=true old_resubmission_rejected=true"
        );
    }
    async fn then_wallets_readable(&self, old: &str) {
        for wallet in ApiWalletRepo::list(&self.pool, None).await.unwrap() {
            assert!(
                PhrasePackageCodec::decrypt_phrase(self.password, &wallet.phrase).await.is_ok()
            );
            assert!(PhrasePackageCodec::decrypt_phrase(old, &wallet.phrase).await.is_err());
            assert!(
                wallet_api::testkit::seed::decrypt_seed(self.password, &wallet.seed).await.is_ok()
            );
            assert!(wallet_api::testkit::seed::decrypt_seed(old, &wallet.seed).await.is_err());
        }
    }
    pub async fn then_new_password_creates_readable_wallet(&self, kind: ApiWalletType) {
        let uid = self.create(kind).await;
        let wallet = ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().unwrap();
        assert_eq!(wallet.is_init, 1);
        assert_eq!(wallet.api_wallet_type, kind);
        assert!(PhrasePackageCodec::decrypt_phrase(self.password, &wallet.phrase).await.is_ok());
        assert!(wallet_api::testkit::seed::decrypt_seed(self.password, &wallet.seed).await.is_ok());
    }
    pub async fn when_failed_rotation_does_not_revoke_creation(&self) {
        self.backend_accepts();
        let gate = Arc::new(Barrier::new(2));
        self.env.fake_backend.gate_next_keys_uid_check(gate.clone());
        let (uid, creation) = self.spawn_creation(ApiWalletType::SubAccount, self.password);
        gate.wait().await;
        assert!(matches!(
            self.env
                .manager
                .set_all_password("synthetic-wrong-password", "unused-new-password")
                .await,
            Err(ServiceError::Business(BusinessError::Wallet(WalletError::PasswordIncorrect)))
        ));
        gate.wait().await;
        assert!(creation.await.unwrap().is_ok());
        let wallet = ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().unwrap();
        assert_eq!(wallet.is_init, 1);
        assert!(PhrasePackageCodec::decrypt_phrase(self.password, &wallet.phrase).await.is_ok());
        assert!(wallet_api::testkit::seed::decrypt_seed(self.password, &wallet.seed).await.is_ok());
        println!(
            "CREDENTIAL_RACE failed_rotation_releases_gate=true valid_pending_creation_succeeds=true"
        );
    }
    pub async fn when_backend_failure_releases_gate(&mut self, next: &'static str) {
        self.backend_accepts();
        self.env.fake_backend.set_query_uid_bind_info_error(Some("synthetic bind failure"));
        let (uid, creation) = self.spawn_creation(ApiWalletType::SubAccount, self.password);
        assert!(creation.await.unwrap().is_err());
        // Preserve existing bind-info failure's partial-persistence semantics.
        assert!(ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().is_some());
        let old = self.password;
        self.rotate_to(next).await;
        self.then_wallets_readable(old).await;
        println!("CREDENTIAL_RACE create_error_releases_gate=true partial_wallet_rotated=true");
    }
    pub async fn when_cancelled_creation_finishes_safely(&mut self, next: &'static str) {
        self.backend_accepts();
        let gate = Arc::new(Barrier::new(2));
        self.env.fake_backend.gate_next_old_keys_init(gate.clone());
        let (uid, creation) = self.spawn_creation(ApiWalletType::SubAccount, self.password);
        gate.wait().await;
        // Wallet storage is already written, but backend/init publication is paused.
        creation.abort();
        match creation.await {
            Err(error) => assert!(error.is_cancelled()),
            Ok(_) => panic!("caller must have been cancelled"),
        }
        assert!(
            wallet_api::testkit::context::api_creation_credential_gate_is_locked(
                self.env.manager.ctx()
            ),
            "owned worker retains credential guard after its caller is cancelled"
        );
        let env = self.env;
        let old = self.password;
        let rotation = tokio::spawn(async move { env.manager.set_all_password(old, next).await });
        // The owned creation worker must still participate after caller cancellation.
        gate.wait().await;
        assert!(rotation.await.unwrap().is_ok());
        self.password = next;
        let wallet = ApiWalletRepo::find_by_uid(&self.pool, &uid).await.unwrap().unwrap();
        assert_eq!(
            wallet.is_init, 1,
            "owned creation completed init and final credentials are consistent"
        );
        self.then_wallets_readable(old).await;
        assert!(self.env.manager.validate_password(next).await.is_ok());
        println!(
            "CREDENTIAL_RACE cancelled_caller_worker_finishes=true rotation_after_cancel_consistent=true"
        );
    }
}
