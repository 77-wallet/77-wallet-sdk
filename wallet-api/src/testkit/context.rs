use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{config::Config, context::Context};

static API_TRANS_TEST_CTX: tokio::sync::OnceCell<&'static Context> =
    tokio::sync::OnceCell::const_new();

pub async fn api_trans_test_ctx() -> &'static Context {
    *API_TRANS_TEST_CTX
        .get_or_init(|| async {
            let config = Config::new(
                r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
            )
            .expect("parse api trans test config");

            let run_id = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before unix epoch")
                .as_nanos();
            let mut dir = PathBuf::from(std::env::temp_dir());
            dir.push(format!("wallet-api-api-trans-tests-{run_id}"));
            std::fs::create_dir_all(&dir).expect("ensure api trans test dir");

            let dirs = crate::dirs::Dirs::new(&dir.to_string_lossy()).expect("create test dirs");
            crate::context::init_context(
                "wallet_api_api_trans_test_sn",
                "unittest",
                dirs,
                None,
                config,
            )
            .await
            .expect("init api trans test context")
        })
        .await
}

pub async fn api_trans_test_pool() -> wallet_database::ApiTransactionDbPool {
    api_trans_test_ctx().await.api_transaction_pool().expect("api transaction test pool")
}

pub async fn api_resource_test_pool() -> wallet_database::ApiResourceDbPool {
    api_trans_test_ctx().await.api_resource_pool().expect("api resource test pool")
}

/// Initialize the usual Context for explicit worker steps without starting actors.
pub async fn init_without_workers(
    sn: &str,
    device_type: &str,
    dirs: crate::dirs::Dirs,
    config: Config,
) -> Result<&'static Context, crate::error::service::ServiceError> {
    crate::context::init_context(sn, device_type, dirs, None, config).await
}

/// Observe ownership in controlled cancellation tests without exposing a guard.
pub fn api_creation_credential_gate_is_locked(ctx: &Context) -> bool {
    ctx.api_wallet_credential_gate.try_lock().is_err()
}
