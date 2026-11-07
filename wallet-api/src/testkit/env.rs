//! Test-only wallet manager and config helpers.

use crate::{dirs::Dirs, manager::WalletManager};
use anyhow::{Context, Result};
use std::{env, path::PathBuf};
use tracing::info;

use crate::request::{account::CreateAccountReq, devices::InitDeviceReq, wallet::CreateWalletReq};
use serde::Deserialize;
use std::fmt;

#[derive(Deserialize, Debug, Default)]
pub struct TestParams {
    pub device_req: InitDeviceReq,
    pub create_wallet_req: CreateWalletReq,
    pub create_account_req: CreateAccountReq,
    pub api_wallet_import: Option<ApiWalletImportConfig>,
}

#[derive(Deserialize, Debug, Default)]
pub struct ApiWalletImportConfig {
    pub sub_account: Option<ApiWalletImportParams>,
    pub withdrawal: Option<ApiWalletImportParams>,
}

#[derive(Deserialize, Clone)]
pub struct ApiWalletImportParams {
    pub language_code: u8,
    pub phrase: String,
    pub salt: String,
    pub wallet_name: String,
    pub wallet_password: String,
    pub invite_code: Option<String>,
    pub binding_address: Option<String>,
}

impl fmt::Debug for ApiWalletImportParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiWalletImportParams")
            .field("language_code", &self.language_code)
            .field("phrase", &"<redacted>")
            .field("salt", &"<redacted>")
            .field("wallet_name", &self.wallet_name)
            .field("wallet_password", &"<redacted>")
            .field("invite_code", &self.invite_code)
            .field("binding_address", &self.binding_address.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

pub async fn get_manager() -> Result<(WalletManager, TestParams)> {
    get_manager_with_config("config.toml").await
}

pub async fn get_manager_with_config(config_file: &str) -> Result<(WalletManager, TestParams)> {
    // Avoid macOS SystemConfiguration proxy resolver panics in sandboxed test environments.
    unsafe {
        std::env::set_var("WALLET_TRANSPORT_NO_PROXY", "1");
    }

    // 获取项目根目录
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);

    let config_dir = dir.join("examples/client_config/").join(config_file);
    println!("example_dir: {config_dir:?}");
    let test_params: TestParams = if let Ok(config_data) = std::fs::read_to_string(config_dir) {
        wallet_utils::serde_func::toml_from_str(&config_data)?
    } else {
        println!("use default TestParams");
        TestParams::default()
    };

    // std::env::set_var("RUST_BACKTRACE", "1");

    let client_id = format!("test_data_{}", config_file.replace(".toml", ""));
    // 获取项目根目录
    let storage_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("test_data").join(client_id);

    // 创建测试目录
    if !storage_dir.exists() {
        std::fs::create_dir_all(&storage_dir)?;
    }

    info!("[setup_test_environment] storage_dir: {:?}", storage_dir);

    let dirs =
        Dirs::new(&storage_dir.to_string_lossy()).with_context(|| "let dirs = Dirs::new(")?;
    // init_log(
    //     Some("info"),
    //     test_params.device_req.app_id.clone().unwrap().as_str(),
    //     &dirs,
    //     &test_params.device_req.sn,
    // )
    // .await?;
    let config = crate::config::Config::new(
        &crate::testkit::env::get_config().with_context(|| "crate::testkit::env::get_config()")?,
    )
    .with_context(|| "config = crate::config::Co")?;
    let wallet_manager = WalletManager::new(
        &test_params.device_req.sn,
        &test_params.device_req.device_type,
        None,
        config,
        dirs,
    )
    .await
    .with_context(|| "let wallet_manager = Wall")?;
    // let derivation_path = "m/44'/60'/0'/0/1".to_string();
    wallet_manager.init(test_params.device_req.clone()).await?;

    Ok((wallet_manager, test_params))
}

pub fn get_config() -> Result<String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| std::env::current_dir().unwrap().to_string_lossy().into_owned());
    let dir = PathBuf::from(manifest_dir);
    let config_dir = dir.join("examples").join("config.yaml");
    let config_data = std::fs::read_to_string(config_dir)?;
    Ok(config_data)
}
