use anyhow::Result;
use std::{env, path::PathBuf};
use tracing::info;

use crate::WalletManager;

pub struct TestData {
    pub wallet_manager: WalletManager,
    pub wallet_env: TestWalletEnv,
}

pub struct TestWalletEnv {
    // pub(crate) storage_dir: PathBuf,
    pub language_code: u8,
    pub phrase: String,
    pub salt: String,
    pub wallet_name: String,
    pub password: String,
}

impl TestWalletEnv {
    fn new(
        language_code: u8,
        phrase: String,
        salt: String,
        wallet_name: String,
        password: String,
    ) -> TestWalletEnv {
        Self {
            language_code,
            phrase,
            salt,
            wallet_name,
            password,
        }
    }
}

pub async fn setup_test_environment(
    mut wallet_name: Option<String>,
    client_id: Option<String>,
    temp: bool,
    phrase: Option<String>,
) -> Result<TestData> {
    let phrase = phrase.unwrap_or_else(|| "".to_string());

    let client_id = client_id.unwrap_or_else(|| "test_data".to_string());
    // 获取项目根目录
    let storage_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join(&client_id);

    // 创建测试目录
    if !storage_dir.exists() {
        std::fs::create_dir_all(&storage_dir)?;
    }

    // 测试参数
    let language_code = 1;
    // let salt = "12345678".to_string();
    let salt = "".to_string();

    if temp {
        info!("storage_dir: {:?}", storage_dir);
        // 创建临时目录结构
        let temm_dir = tempfile::tempdir_in(&storage_dir)?;
        wallet_name = temm_dir
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
    }
    let wallet_name = wallet_name.unwrap_or_else(|| "example_wallet".to_string());

    let password = "".to_string();
    info!("[setup_test_environment] storage_dir: {:?}", storage_dir);
    let wallet_manager = WalletManager::new(
        "bdb6412a9cb4b12c48ebe1ef4e9f052b07af519b7485cd38a95f38d89df97cb8",
        "ANDROID",
        &storage_dir.to_string_lossy(),
        None,
    )
    .await?;

    let wallet_env = TestWalletEnv::new(language_code, phrase, salt, wallet_name, password);

    Ok(TestData {
        wallet_manager,
        wallet_env,
    })
}
