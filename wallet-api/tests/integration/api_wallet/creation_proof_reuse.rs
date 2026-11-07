//! Run exact in a fresh process: this fixture owns the global Manager context.
use crate::harness::{self, ApiWalletBackendCall};
use std::time::Instant;
use wallet_database::{entities::api_wallet::ApiWalletType, repositories::device::DeviceRepo};
use wallet_transport_backend::response_vo::api_wallet::wallet::UidStatus;

const PHRASE: &str =
    "";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(import_bind)]
async fn creation_proof_reuse_retains_authentication_and_initialization() {
    let env = harness::ensure_env().await;
    let core = harness::open_core_pool(&env.db_dir).await;
    // Match the modern production policy explicitly; new_for_test does not run
    // App initialization and would otherwise silently default to Scrypt.
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
    // Observe real persistence without recording proof contents.
    sqlx::query("CREATE TABLE proof_write_audit (id INTEGER)")
        .execute(core.write_ref())
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER audit_proof_write AFTER UPDATE OF password_proof ON device BEGIN INSERT INTO proof_write_audit VALUES (1); END").execute(core.write_ref()).await.unwrap();
    let mut expected_proof = None;
    let mut observed = Vec::new();
    for (case, kind, expected_writes) in [
        ("first", ApiWalletType::SubAccount, 1),
        ("reuse_sub_1", ApiWalletType::SubAccount, 0),
        ("reuse_sub_2", ApiWalletType::SubAccount, 0),
        ("reuse_withdrawal", ApiWalletType::Withdrawal, 0),
        ("missing", ApiWalletType::SubAccount, 1),
        ("old_algorithm", ApiWalletType::SubAccount, 1),
        ("reuse_after_upgrade", ApiWalletType::SubAccount, 0),
    ] {
        if case == "missing" {
            DeviceRepo::update_password_proof(core.clone(), &env.sn, None).await.unwrap();
        }
        if case == "old_algorithm" {
            let old_proof = outdated_proof();
            DeviceRepo::update_password_proof(core.clone(), &env.sn, Some(&old_proof))
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM proof_write_audit").execute(core.write_ref()).await.unwrap();
        harness::reset_fake(env);
        env.fake_backend.enqueue_keys_uid_status(UidStatus::NotFound);
        env.fake_backend.enqueue_query_uid_bind_info("", "", false, &env.sn);
        let salt = harness::next_tag(case);
        let started = Instant::now();
        let uid = env
            .manager
            .create_api_wallet(
                1,
                PHRASE,
                &salt,
                "proof-reuse-fixture",
                harness::SMOKE_WALLET_PASSWORD,
                None,
                kind,
                None,
            )
            .await
            .unwrap();
        println!("PROOF_REUSE case={case} wall_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
        assert!(uid == harness::derive_uid(PHRASE, &salt));
        let pool = harness::open_api_wallet_pool(&env.db_dir).await;
        let wallet = wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::find_by_uid(
            &pool, &uid,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(wallet.is_init, 1);
        assert_eq!(wallet.api_wallet_type, kind);
        env.fake_backend.with_calls(|calls| {
            assert_eq!(calls.len(), if kind == ApiWalletType::SubAccount { 4 } else { 3 });
            assert!(matches!(calls[0], ApiWalletBackendCall::KeysUidCheck { .. }));
            assert!(matches!(calls[1], ApiWalletBackendCall::InitApiWallet(_)));
            assert!(matches!(calls[2], ApiWalletBackendCall::OldKeysInit(_)));
        });
        let proof = DeviceRepo::get_device_info(core.clone(), &env.sn)
            .await
            .unwrap()
            .unwrap()
            .password_proof;
        assert!(proof.is_some());
        let metadata: wallet_crypto::KeystoreJson =
            serde_json::from_str(proof.as_deref().unwrap()).unwrap();
        let expected = wallet_crypto::kdf::factory::KdfFactory::create(
            &wallet_crypto::KdfAlgorithm::Argon2id,
            match &metadata.crypto.kdfparams {
                wallet_crypto::kdf::KdfParams::Argon2id(p) => &p.salt,
                wallet_crypto::kdf::KdfParams::Scrypt(p) => &p.salt,
                wallet_crypto::kdf::KdfParams::Pbkdf2(p) => &p.salt,
            },
        )
        .unwrap()
        .params();
        // Record the mismatch without dumping ciphertext or secret inputs on failure.
        observed.push((
            "current_kdf_policy",
            i64::from(
                metadata.crypto.kdf != wallet_crypto::KdfAlgorithm::Argon2id
                    || metadata.crypto.kdfparams != expected,
            ),
            0,
        ));
        let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proof_write_audit")
            .fetch_one(core.read_ref())
            .await
            .unwrap();
        observed.push((case, writes, expected_writes));
        if case.starts_with("reuse") {
            observed.push(("proof_unchanged", i64::from(proof != expected_proof), 0));
        }
        expected_proof = proof;
    }
    // A failed fallback persistence must not authorize creation or reach HTTP.
    DeviceRepo::update_password_proof(core.clone(), &env.sn, None).await.unwrap();
    sqlx::query("DELETE FROM proof_write_audit").execute(core.write_ref()).await.unwrap();
    sqlx::query("CREATE TRIGGER reject_proof_write BEFORE UPDATE OF password_proof ON device WHEN NEW.password_proof IS NOT NULL BEGIN SELECT RAISE(ABORT, 'synthetic proof persistence failure'); END").execute(core.write_ref()).await.unwrap();
    harness::reset_fake(env);
    let result = env
        .manager
        .create_api_wallet(
            1,
            PHRASE,
            "proof-write-failure",
            "rejected",
            harness::SMOKE_WALLET_PASSWORD,
            None,
            ApiWalletType::SubAccount,
            None,
        )
        .await;
    assert!(result.is_err());
    env.fake_backend.with_calls(|calls| assert!(calls.is_empty()));
    let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proof_write_audit")
        .fetch_one(core.read_ref())
        .await
        .unwrap();
    assert_eq!(writes, 0);
    assert!(
        DeviceRepo::get_device_info(core.clone(), &env.sn)
            .await
            .unwrap()
            .unwrap()
            .password_proof
            .is_none()
    );
    sqlx::query("DROP TRIGGER reject_proof_write").execute(core.write_ref()).await.unwrap();
    DeviceRepo::update_password_proof(core.clone(), &env.sn, expected_proof.as_deref())
        .await
        .unwrap();
    // Wrong password and damaged proof must fail before any HTTP or proof write.
    for corrupt in [false, true] {
        if corrupt {
            DeviceRepo::update_password_proof(core.clone(), &env.sn, Some("invalid-proof"))
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM proof_write_audit").execute(core.write_ref()).await.unwrap();
        harness::reset_fake(env);
        let password =
            if corrupt { harness::SMOKE_WALLET_PASSWORD } else { "wrong-fixture-password" };
        let result = env
            .manager
            .create_api_wallet(
                1,
                PHRASE,
                "rejected",
                "rejected",
                password,
                None,
                ApiWalletType::SubAccount,
                None,
            )
            .await;
        assert!(result.is_err());
        env.fake_backend.with_calls(|calls| assert!(calls.is_empty()));
        let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proof_write_audit")
            .fetch_one(core.read_ref())
            .await
            .unwrap();
        assert_eq!(writes, 0);
    }
    DeviceRepo::update_password_proof(core.clone(), &env.sn, expected_proof.as_deref())
        .await
        .unwrap();
    for (case, actual, expected) in observed {
        assert_eq!(actual, expected, "{case}: proof writes/change");
    }
}

// A valid proof left by a pre-upgrade KDF, with the current device already upgraded.
fn outdated_proof() -> String {
    use wallet_crypto::EncryptedJsonGenerator;
    let proof = wallet_crypto::KeystoreJsonGenerator::new(
        rand::rngs::OsRng,
        wallet_crypto::KdfAlgorithm::Scrypt,
    )
    .generate(harness::SMOKE_WALLET_PASSWORD.as_bytes(), b"wallet-sdk-password-proof")
    .unwrap();
    serde_json::to_string(&proof).unwrap()
}
