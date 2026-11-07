mod support;

use serial_test::serial;

use support::{
    PasswordRotationGiven, PasswordRotationScenario, PasswordRotationThen, PasswordRotationWhen,
    ScenarioRoles,
};

#[tokio::test]
#[serial(import_bind)]
async fn change_password_refreshes_api_wallet_unlock_session() {
    let scenario = PasswordRotationScenario::new().await;
    let given = scenario.given();
    let when = scenario.when();
    let then = scenario.then();

    given.backend_accepts_withdrawal_uid();

    let uid = when.withdrawal_wallet_is_imported().await;

    then.wallet_is_withdrawal(&uid).await;
    when.password_is_rotated().await;
    when.rotation_tick_passes().await;
    when.api_wallet_chain_data_syncs().await;
    when.password_is_restored().await;
}
