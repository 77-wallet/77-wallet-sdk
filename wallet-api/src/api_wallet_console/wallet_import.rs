use anyhow::{Context, Result};
use wallet_database::entities::api_wallet::ApiWalletType;

use crate::{
    manager::WalletManager,
    testkit::env::{ApiWalletImportParams, TestParams},
};

pub async fn import_configured_api_wallets(
    wallet_manager: &WalletManager,
    test_params: &TestParams,
    config_file: &str,
) -> Result<Vec<String>> {
    let import_config = test_params
        .api_wallet_import
        .as_ref()
        .with_context(|| format!("missing api_wallet_import in {config_file}"))?;

    let sub_account = import_config
        .sub_account
        .as_ref()
        .with_context(|| format!("missing api_wallet_import.sub_account in {config_file}"))?;
    import_configured_api_wallet(
        wallet_manager,
        sub_account,
        ApiWalletType::SubAccount,
        "sub_account",
    )
    .await?;

    let withdrawal = import_config
        .withdrawal
        .as_ref()
        .with_context(|| format!("missing api_wallet_import.withdrawal in {config_file}"))?;
    import_configured_api_wallet(
        wallet_manager,
        withdrawal,
        ApiWalletType::Withdrawal,
        "withdrawal",
    )
    .await?;

    Ok(vec![
        "import sub_account api wallet succeeded".to_string(),
        "import withdrawal api wallet succeeded".to_string(),
    ])
}

async fn import_configured_api_wallet(
    wallet_manager: &WalletManager,
    params: &ApiWalletImportParams,
    api_wallet_type: ApiWalletType,
    _label: &str,
) -> Result<()> {
    wallet_manager
        .import_api_wallet(
            params.language_code,
            &params.phrase,
            &params.salt,
            &params.wallet_name,
            &params.wallet_password,
            params.invite_code.clone(),
            api_wallet_type,
            params.binding_address.as_deref(),
        )
        .await?;

    Ok(())
}
