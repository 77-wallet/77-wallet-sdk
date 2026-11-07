use wallet_database::entities::api_wallet::ApiWalletType;
use wallet_transport_backend::response_vo::api_wallet::wallet::ActiveStatus;

use crate::harness::{
    ApiWalletBackendCall, SMOKE_WALLET_PASSWORD, ensure_env, next_tag, reset_fake, upsert_wallet,
};

#[tokio::test]
async fn query_wallet_activation_info_should_use_fake_backend_uid_boundary() {
    let env = ensure_env().await;
    reset_fake(env);
    let uid = next_tag("activation-uid");
    let address = upsert_wallet(&env.db_dir, &env.sn, &uid, ApiWalletType::Withdrawal, None).await;
    env.fake_backend.enqueue_wallet_activation_info(vec![("tron", ActiveStatus::Active)]);

    env.manager.set_passwd_cache(SMOKE_WALLET_PASSWORD).await.expect("set password cache");
    let resp =
        env.manager.query_wallet_activation_info(&address).await.expect("query activation info");

    assert_eq!(resp.0.len(), 1);
    assert_eq!(resp.0[0].chain, "tron");
    assert!(matches!(resp.0[0].active, ActiveStatus::Active));
    env.fake_backend.with_calls(|calls| {
        let activation_calls: Vec<&String> = calls
            .iter()
            .filter_map(|call| match call {
                ApiWalletBackendCall::QueryWalletActivationInfo { uid } => Some(uid),
                _ => None,
            })
            .collect();
        assert_eq!(activation_calls, vec![&uid]);
    });
}
