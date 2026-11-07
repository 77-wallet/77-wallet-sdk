use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use crate::{
    context::Context,
    domain::api_wallet::unlock::{
        WalletUnlockMaterial, WalletUnlockSession, WalletUnlockSessionCodec,
    },
    error::service::ServiceError,
};
use once_cell::sync::OnceCell;

struct UnlockSessionRuntime {
    context: &'static Context,
    rotation_started: AtomicBool,
}

static RUNTIME: OnceCell<UnlockSessionRuntime> = OnceCell::new();

fn ensure_runtime(
    context: &'static Context,
) -> Result<&'static UnlockSessionRuntime, ServiceError> {
    if let Some(runtime) = RUNTIME.get() {
        if std::ptr::eq(runtime.context, context) {
            return Ok(runtime);
        }

        return Err(crate::error::service::ServiceError::System(
            crate::error::system::SystemError::Internal(
                "unlock session runtime already initialized with another context".to_string(),
            ),
        ));
    }

    RUNTIME
        .set(UnlockSessionRuntime { context, rotation_started: AtomicBool::new(false) })
        .map_err(|_| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "unlock session runtime initialization failed".to_string(),
                ),
            )
        })?;

    runtime()
}

fn runtime() -> Result<&'static UnlockSessionRuntime, ServiceError> {
    RUNTIME.get().ok_or_else(|| crate::error::system::SystemError::ContextNotInit.into())
}

pub(crate) async fn wallet_unlock_session_snapshot() -> Option<WalletUnlockSession> {
    let Ok(runtime) = runtime() else {
        return None;
    };

    runtime.context.wallet_unlock_session_snapshot().await
}

pub(crate) async fn rotate_wallet_unlock_session_if_due() -> Result<bool, ServiceError> {
    let context = runtime()?.context;
    rotate_wallet_unlock_session_if_due_in_context(context).await
}

async fn rotate_wallet_unlock_session_if_due_in_context(
    context: &'static Context,
) -> Result<bool, ServiceError> {
    let Some(session) = context.wallet_unlock_session_snapshot().await else {
        return Ok(false);
    };

    if !session.is_expired() {
        return Ok(false);
    }

    tracing::debug!("wallet unlock session rotation due, rotating session");
    crate::domain::api_wallet::wallet::ApiWalletDomain::new(context)
        .rotate_wallet_session_key()
        .await?;
    Ok(true)
}

pub(crate) async fn start_wallet_unlock_session_rotation_task(
    context: &'static Context,
) -> Result<(), ServiceError> {
    let runtime = ensure_runtime(context)?;
    if runtime
        .rotation_started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    tokio::spawn(async move {
        let mut interval = crate::infrastructure::runtime::time::new_production_interval(
            WalletUnlockSessionCodec::unlock_session_rotation_check_interval(),
        );
        loop {
            interval.tick().await;
            match rotate_wallet_unlock_session_if_due().await {
                Ok(true) => {
                    tracing::debug!("wallet unlock session rotation loop refreshed session");
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::error!("wallet unlock session rotation loop failed: {:?}", err);
                    // Backoff to break the lock contention loop: rotation fails
                    // because DB write lock is contended by asset Workers;
                    // retrying immediately re-acquires the lock and fails again.
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            }
        }
    });

    Ok(())
}

pub(crate) async fn wallet_unlock_token() -> Result<String, ServiceError> {
    let context = runtime()?.context;
    wallet_unlock_token_from_context(context).await
}

async fn wallet_unlock_token_from_context(context: &Context) -> Result<String, ServiceError> {
    let Some(session) = context.wallet_unlock_session_snapshot().await else {
        return Err(crate::error::system::SystemError::SystemNotReady.into());
    };

    Ok(session.session_token().to_string())
}

pub(crate) async fn wallet_unlock_token_is_active(token: &str) -> Result<bool, ServiceError> {
    let context = runtime()?.context;
    wallet_unlock_token_is_active_in_context(context, token).await
}

async fn wallet_unlock_token_is_active_in_context(
    context: &Context,
    token: &str,
) -> Result<bool, ServiceError> {
    let Some(session) = context.wallet_unlock_session_snapshot().await else {
        return Ok(false);
    };

    Ok(session.session_token() == token)
}

pub(crate) async fn wallet_unlock_material(
    wallet_address: &str,
) -> Result<WalletUnlockMaterial, ServiceError> {
    let context = runtime()?.context;
    wallet_unlock_material_from_context(context, wallet_address).await
}

async fn wallet_unlock_material_from_context(
    context: &Context,
    wallet_address: &str,
) -> Result<WalletUnlockMaterial, ServiceError> {
    let Some(session) = context.wallet_unlock_session_snapshot().await else {
        return Err(crate::error::system::SystemError::SystemNotReady.into());
    };

    session
        .wallet_material(wallet_address)
        .cloned()
        .ok_or_else(|| crate::error::system::SystemError::SystemNotReady.into())
}

#[cfg(debug_assertions)]
pub(crate) fn diagnostic_context_matches(context: &'static Context) -> Option<bool> {
    RUNTIME.get().map(|runtime| std::ptr::eq(runtime.context, context))
}

pub(crate) async fn upsert_wallet_unlock_material(
    wallet_address: &str,
    wallet_password: &str,
) -> Result<(), ServiceError> {
    let context = runtime()?.context;
    upsert_wallet_unlock_material_in_context(context, wallet_address, wallet_password).await
}

async fn upsert_wallet_unlock_material_in_context(
    context: &'static Context,
    wallet_address: &str,
    wallet_password: &str,
) -> Result<(), ServiceError> {
    let pool = context.api_wallet_pool()?;
    let Some(wallet) =
        wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::find_by_address(
            &pool,
            wallet_address,
        )
        .await?
    else {
        return Err(crate::error::business::BusinessError::ApiWallet(
            crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
        )
        .into());
    };

    let envelope = crate::domain::api_wallet::unlock::SeedEnvelopeCodec::decrypt_seed_envelope(
        wallet_password,
        &wallet.seed,
    )
    .await?;
    let smk = WalletUnlockSessionCodec::derive_smk(wallet_password, &envelope.salt).await?;
    let wallet_material = WalletUnlockMaterial::new(smk.to_vec());

    let Some(mut session) = context.wallet_unlock_session_snapshot().await else {
        let mut wallet_materials = std::collections::HashMap::new();
        wallet_materials.insert(wallet_address.to_string(), wallet_material);
        let unlock_session = crate::domain::api_wallet::unlock::WalletUnlockSession::new(
            crate::domain::api_wallet::unlock::WalletUnlockSessionCodec::generate_unlock_token(),
            Instant::now()
                + crate::domain::api_wallet::unlock::WalletUnlockSessionCodec::unlock_session_rotation_interval(),
            wallet_materials,
        );
        context.set_wallet_unlock_session(unlock_session).await?;
        return Ok(());
    };

    session.upsert_wallet_material(wallet_address.to_string(), wallet_material);
    let next_rotation_at = session.next_rotation_at();
    let session_token = session.session_token().to_string();
    let wallet_materials = session.wallet_materials_snapshot();
    context
        .set_wallet_unlock_session(WalletUnlockSession::new(
            session_token,
            next_rotation_at,
            wallet_materials,
        ))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiWalletBackend, dirs::Dirs, domain::api_wallet::wallet::ApiWalletDomain};
    use async_trait::async_trait;
    use serial_test::serial;
    use std::{
        collections::HashMap,
        sync::{Arc, Once},
        time::{Duration, Instant},
    };
    use tempfile::TempDir;
    use tokio::time::sleep;
    use wallet_database::{
        entities::api_wallet::ApiWalletType, repositories::api_wallet::wallet::ApiWalletRepo,
    };
    use wallet_transport_backend::{
        request::{
            DeviceDeleteReq, KeysInitReq,
            api_wallet::{
                address::ExpandAddressCompleteReq,
                swap::{ApiInitSwapReq, ApiInitSwapResponse},
                wallet::{
                    AppIdImportRechargeWalletReq, AppIdImportReq, AppIdUidUsageReq, BindAppIdReq,
                },
            },
        },
        response_vo::api_wallet::wallet::{
            AppIdUidUsageRes, KeysUidCheckRes, QueryUidBindInfoRes, QueryWalletActivationInfoResp,
            UidStatus,
        },
    };

    static TEST_TRACING: Once = Once::new();

    fn init_test_tracing() {
        TEST_TRACING.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_max_level(tracing::Level::INFO)
                .try_init();
        });
    }

    struct UnlockSessionTestEnv {
        _tempdir: TempDir,
        context: &'static Context,
    }

    #[derive(Default)]
    struct NoopApiWalletBackend;

    #[async_trait]
    impl ApiWalletBackend for NoopApiWalletBackend {
        async fn wallet_bind_appid(&self, _: BindAppIdReq) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn init_api_wallet(&self, _: AppIdImportReq) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn old_keys_init(&self, _: KeysInitReq) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn appid_import(&self, _: AppIdImportReq) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn appid_import_recharge_wallet(
            &self,
            _: AppIdImportRechargeWalletReq,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn keys_uid_check(&self, uid: &str) -> Result<KeysUidCheckRes, ServiceError> {
            Ok(KeysUidCheckRes { uid: uid.to_string(), status: UidStatus::ApiRaw })
        }

        async fn query_uid_bind_info(
            &self,
            uid: &str,
        ) -> Result<QueryUidBindInfoRes, ServiceError> {
            Ok(QueryUidBindInfoRes {
                app_id: String::new(),
                org_id: String::new(),
                bind_status: false,
                sn: uid.to_string(),
            })
        }

        async fn query_wallet_activation_info(
            &self,
            _: &str,
        ) -> Result<QueryWalletActivationInfoResp, ServiceError> {
            Ok(QueryWalletActivationInfoResp(Vec::new()))
        }

        async fn appid_uid_usage(
            &self,
            _: AppIdUidUsageReq,
        ) -> Result<AppIdUidUsageRes, ServiceError> {
            Ok(AppIdUidUsageRes { used: false })
        }

        async fn expand_address_complete(
            &self,
            _: ExpandAddressCompleteReq,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn appid_withdrawal_wallet_change(
            &self,
            _: &str,
            _: &str,
        ) -> Result<(), ServiceError> {
            Ok(())
        }

        async fn init_swap(&self, _: &ApiInitSwapReq) -> Result<ApiInitSwapResponse, ServiceError> {
            Ok(ApiInitSwapResponse { success: true, code: None, msg: None, data: None })
        }

        async fn device_delete(&self, _: &DeviceDeleteReq) -> Result<Option<()>, ServiceError> {
            Ok(Some(()))
        }
    }

    async fn unlock_session_env() -> UnlockSessionTestEnv {
        init_test_tracing();
        let config = crate::config::Config::new(
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
        .expect("parse test config");
        let tempdir = TempDir::new().expect("create tempdir");
        let dirs =
            Dirs::new(tempdir.path().to_str().expect("utf8 temp path")).expect("create dirs");
        let context = Box::leak(Box::new(
            Context::new_without_handles_for_test(dirs, config)
                .await
                .expect("create isolated test context"),
        ));
        UnlockSessionTestEnv { _tempdir: tempdir, context }
    }

    #[tokio::test]
    #[serial]
    async fn wallet_unlock_session_rotation_logs() {
        init_test_tracing();
        let _env = unlock_session_env().await;
        let context = _env.context;

        let wallet_address = "0xcontext-unlock-session";
        let unlock_material = WalletUnlockMaterial::new(vec![0x11; 32]);
        let mut wallet_materials = HashMap::new();
        wallet_materials.insert(wallet_address.to_string(), unlock_material.clone());

        eprintln!("[context-unlock] 1) prepare unlock material and session");
        let session_token = WalletUnlockSessionCodec::generate_unlock_token();
        let unlock_session = WalletUnlockSession::new(
            session_token.clone(),
            Instant::now() + WalletUnlockSessionCodec::unlock_session_rotation_interval(),
            wallet_materials,
        );
        eprintln!("[context-unlock] 1.1) session ready");

        eprintln!("[context-unlock] 2) store unlock session in context");
        context.set_wallet_unlock_session(unlock_session).await.expect("store unlock session");

        eprintln!("[context-unlock] 3) read token from context");
        let stored_token = wallet_unlock_token_from_context(context).await.expect("read token");
        eprintln!("[context-unlock] 3.1) token roundtrip ok");
        assert!(
            wallet_unlock_token_is_active_in_context(context, &stored_token)
                .await
                .expect("check token active")
        );
        assert_eq!(stored_token, session_token);

        eprintln!("[context-unlock] 4) read wallet material from context");
        let stored_material = wallet_unlock_material_from_context(context, wallet_address)
            .await
            .expect("read wallet material");
        eprintln!("[context-unlock] 4.1) wallet material ready");
        assert_eq!(stored_material.smk(), unlock_material.smk());

        eprintln!("[context-unlock] 5) wait for rotation interval and trigger refresh");
        sleep(
            WalletUnlockSessionCodec::unlock_session_rotation_interval()
                + Duration::from_millis(100),
        )
        .await;
        let rotated = rotate_wallet_unlock_session_if_due_in_context(context)
            .await
            .expect("rotate due session");
        eprintln!("[context-unlock] 5.1) rotate helper returned {rotated}");

        let token_after_rotate =
            wallet_unlock_token_from_context(context).await.expect("read token after rotate");
        eprintln!("[context-unlock] 5.2) token after rotate");
        assert!(!token_after_rotate.is_empty());

        let material_after_rotate = wallet_unlock_material_from_context(context, wallet_address)
            .await
            .expect("read material after rotate");
        eprintln!("[context-unlock] 5.3) material after rotate");
        assert_eq!(material_after_rotate.smk(), unlock_material.smk());
        if rotated {
            assert_ne!(token_after_rotate, session_token);
        }
    }

    #[tokio::test]
    #[serial]
    async fn wallet_unlock_session_rotation_rebuild_logs() {
        init_test_tracing();
        let _env = unlock_session_env().await;
        let context = _env.context;

        let wallet_address = "0xcontext-unlock-session-expired";
        let unlock_material = WalletUnlockMaterial::new(vec![0x22; 32]);
        let mut wallet_materials = HashMap::new();
        wallet_materials.insert(wallet_address.to_string(), unlock_material.clone());

        let unlock_session = WalletUnlockSession::new(
            "rotation-due-token".to_string(),
            Instant::now() - Duration::from_millis(1),
            wallet_materials,
        );
        eprintln!("[context-unlock] rotation 1) store rotation-due unlock session");
        context
            .set_wallet_unlock_session(unlock_session)
            .await
            .expect("store rotation-due unlock session");

        eprintln!("[context-unlock] rotation 2) run rotation helper");
        let rotated = rotate_wallet_unlock_session_if_due_in_context(context)
            .await
            .expect("rotate due session");
        eprintln!("[context-unlock] rotation 2.1) rotate returned {rotated}");
        assert!(rotated);

        eprintln!("[context-unlock] rotation 3) verify token/material are still available");
        let token =
            wallet_unlock_token_from_context(context).await.expect("token should remain available");
        eprintln!("[context-unlock] rotation 3.1) token read ok");
        assert!(!token.is_empty());

        let material = wallet_unlock_material_from_context(context, wallet_address)
            .await
            .expect("material should remain available");
        eprintln!("[context-unlock] rotation 3.2) material read ok");
        assert_eq!(material.smk(), unlock_material.smk());
    }

    #[tokio::test]
    #[serial]
    async fn upsert_wallet_unlock_material_keeps_existing_session() {
        init_test_tracing();
        let _env = unlock_session_env().await;
        let context = _env.context;

        let wallet1_address = "0xcontext-unlock-wallet-1";
        let wallet2_address = "0xcontext-unlock-wallet-2";
        let wallet3_address = "0xcontext-unlock-wallet-3";
        let password1 = "unlock-password-one";
        let password2 = "unlock-password-two";
        let phrase = "phrase-package-roundtrip";
        let seed = b"unlock-flow-seed";

        ApiWalletDomain::new(context)
            .upsert_api_wallet(
                "uid-wallet-1",
                "wallet-1",
                wallet1_address,
                password1,
                phrase,
                seed,
                ApiWalletType::Withdrawal,
                None,
            )
            .await
            .expect("upsert wallet 1");
        ApiWalletDomain::new(context)
            .upsert_api_wallet(
                "uid-wallet-2",
                "wallet-2",
                wallet2_address,
                password2,
                phrase,
                seed,
                ApiWalletType::Withdrawal,
                None,
            )
            .await
            .expect("upsert wallet 2");
        ApiWalletDomain::new(context)
            .upsert_api_wallet(
                "uid-wallet-3",
                "wallet-3",
                wallet3_address,
                password1,
                phrase,
                seed,
                ApiWalletType::Withdrawal,
                None,
            )
            .await
            .expect("upsert wallet 3");

        let wallet1 =
            ApiWalletRepo::find_by_address(&context.api_wallet_pool().unwrap(), wallet1_address)
                .await
                .expect("find wallet 1")
                .expect("wallet 1 exists");
        let envelope1 =
            crate::domain::api_wallet::unlock::SeedEnvelopeCodec::decrypt_seed_envelope(
                password1,
                &wallet1.seed,
            )
            .await
            .expect("decrypt wallet 1 envelope");
        let smk1 = WalletUnlockSessionCodec::derive_smk(password1, &envelope1.salt)
            .await
            .expect("derive wallet 1 smk");
        let mut wallet_materials = HashMap::new();
        wallet_materials
            .insert(wallet1_address.to_string(), WalletUnlockMaterial::new(smk1.to_vec()));
        let session_token = WalletUnlockSessionCodec::generate_unlock_token();
        let unlock_session = WalletUnlockSession::new(
            session_token.clone(),
            Instant::now() + WalletUnlockSessionCodec::unlock_session_rotation_interval(),
            wallet_materials,
        );
        context.set_wallet_unlock_session(unlock_session).await.expect("store initial session");

        upsert_wallet_unlock_material_in_context(context, wallet3_address, password1)
            .await
            .expect("upsert wallet 3 unlock material");

        let stored_token =
            wallet_unlock_token_from_context(context).await.expect("token remains available");
        assert_eq!(stored_token, session_token);
        assert!(
            wallet_unlock_token_is_active_in_context(context, &stored_token)
                .await
                .expect("token active")
        );

        let wallet3_material = wallet_unlock_material_from_context(context, wallet3_address)
            .await
            .expect("wallet 3 material");
        assert!(!wallet3_material.smk().is_empty());

        let wallet2_material = wallet_unlock_material_from_context(context, wallet2_address)
            .await
            .expect_err("wallet 2 should not be touched by wallet 3 upsert");
        assert!(matches!(
            wallet2_material,
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::SystemNotReady
            )
        ));
    }

    #[tokio::test]
    #[serial]
    async fn upsert_wallet_unlock_material_creates_session_when_absent() {
        init_test_tracing();
        let _env = unlock_session_env().await;
        let context = _env.context;

        let wallet_address = "0xcontext-unlock-wallet-new";
        let password = "unlock-password-new";
        let phrase = "phrase-package-roundtrip";
        let seed = b"unlock-flow-seed";

        ApiWalletDomain::new(context)
            .upsert_api_wallet(
                "uid-wallet-new",
                "wallet-new",
                wallet_address,
                password,
                phrase,
                seed,
                ApiWalletType::Withdrawal,
                None,
            )
            .await
            .expect("upsert wallet");

        upsert_wallet_unlock_material_in_context(context, wallet_address, password)
            .await
            .expect("upsert wallet unlock material");

        let stored_token =
            wallet_unlock_token_from_context(context).await.expect("token available");
        assert!(
            wallet_unlock_token_is_active_in_context(context, &stored_token)
                .await
                .expect("token active")
        );

        let material = wallet_unlock_material_from_context(context, wallet_address)
            .await
            .expect("wallet material available");
        assert!(!material.smk().is_empty());
    }
}
