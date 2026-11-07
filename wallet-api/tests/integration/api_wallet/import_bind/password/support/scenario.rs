use std::time::Duration;

use wallet_database::entities::api_wallet::ApiWalletType;
use wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus;

use crate::harness::{
    self, GivenRole, ThenRole, WhenRole, ensure_env, load_wallet_by_uid, next_tag, reset_fake,
};

use super::super::super::support::{WALLET_PASSWORD, WITHDRAWAL_PHRASE};

const ROTATED_PASSWORD: &str = "new_passwd";

pub(crate) struct PasswordRotationScenario {
    env: &'static harness::TestEnv,
}

impl PasswordRotationScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_env().await;
        reset_fake(env);
        Self { env }
    }
}

pub(crate) trait PasswordRotationGiven {
    fn backend_accepts_withdrawal_uid(&self);
}

impl PasswordRotationGiven for GivenRole<'_, PasswordRotationScenario> {
    fn backend_accepts_withdrawal_uid(&self) {
        self.scenario().env.fake_backend.enqueue_keys_uid_status(UidStatus::ApiWaw);
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait PasswordRotationWhen {
    async fn withdrawal_wallet_is_imported(&self) -> String;

    async fn password_is_rotated(&self);

    async fn rotation_tick_passes(&self);

    async fn api_wallet_chain_data_syncs(&self);

    async fn password_is_restored(&self);
}

#[async_trait::async_trait(?Send)]
impl PasswordRotationWhen for WhenRole<'_, PasswordRotationScenario> {
    async fn withdrawal_wallet_is_imported(&self) -> String {
        self.scenario()
            .env
            .manager
            .import_api_wallet(
                1,
                WITHDRAWAL_PHRASE,
                &next_tag("salt-password-change"),
                &next_tag("withdraw-wallet-password-change"),
                WALLET_PASSWORD,
                None,
                ApiWalletType::Withdrawal,
                None,
            )
            .await
            .expect("import withdrawal wallet")
    }

    async fn password_is_rotated(&self) {
        self.scenario()
            .env
            .manager
            .set_all_password(WALLET_PASSWORD, ROTATED_PASSWORD)
            .await
            .expect("change password");
    }

    async fn rotation_tick_passes(&self) {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    async fn api_wallet_chain_data_syncs(&self) {
        self.scenario()
            .env
            .manager
            .sync_api_wallet_chain_data()
            .await
            .expect("sync api wallet chain data after password change");
    }

    async fn password_is_restored(&self) {
        self.scenario()
            .env
            .manager
            .set_all_password(ROTATED_PASSWORD, WALLET_PASSWORD)
            .await
            .expect("restore wallet password for later integration tests");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait PasswordRotationThen {
    async fn wallet_is_withdrawal(&self, uid: &str);
}

#[async_trait::async_trait(?Send)]
impl PasswordRotationThen for ThenRole<'_, PasswordRotationScenario> {
    async fn wallet_is_withdrawal(&self, uid: &str) {
        let wallet = load_wallet_by_uid(self.scenario().env, uid).await;
        assert_eq!(wallet.api_wallet_type as u8, ApiWalletType::Withdrawal as u8);
    }
}
