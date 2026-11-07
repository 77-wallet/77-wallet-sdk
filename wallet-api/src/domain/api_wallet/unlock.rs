use aes_gcm::{
    Aes256Gcm, Key, KeyInit, Nonce,
    aead::{Aead, AeadCore},
};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

use crate::{context::Context, error::service::ServiceError};
use sha2::Digest;

pub(crate) use aes_gcm::aead::OsRng;

/// 版本化的 seed 封装格式。
///
/// 这层只负责“把 seed 从可直接解读的单层密文，升级成可轮换的封装结构”。
/// 真正的明文密码不应该进入这层的持久化格式里。
pub(crate) const SEED_ENVELOPE_VERSION_V1: u8 = 1;
pub(crate) const SEED_ENVELOPE_SALT_BYTES: usize = 16;
pub(crate) const SEED_ENVELOPE_KEY_BYTES: usize = 32;
pub(crate) const SEED_ENVELOPE_NONCE_BYTES: usize = 12;
pub(crate) const SEED_ENVELOPE_ROTATION_COUNTER: u64 = 0;
const SEED_ENVELOPE_HKDF_INFO: &[u8] = b"wallet-api-seed-envelope-v1";
// Seed package 的二进制格式魔数，只用于区分包格式，不参与安全性。
const SEED_PACKAGE_MAGIC: &[u8; 4] = b"wb1\0";
const SEED_PACKAGE_VERSION_V1: u8 = 1;
const SEED_PACKAGE_HKDF_INFO: &[u8] = b"wallet-api-seed-package-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeedEnvelopeV1 {
    pub(crate) version: u8,
    pub(crate) salt: Vec<u8>,
    pub(crate) rotation_counter: u64,
    pub(crate) session_nonce: Vec<u8>,
    pub(crate) seed_nonce: Vec<u8>,
    pub(crate) wrapped_dek: Vec<u8>,
    pub(crate) seed_cipher: Vec<u8>,
}

#[derive(Debug, Clone)]
struct SeedEnvelopePackageV1 {
    version: u8,
    salt: Vec<u8>,
    rotation_counter: u64,
    nonce: Vec<u8>,
    payload: Vec<u8>,
}

/// 解锁材料只保存“可以重新解开 seed 的中间材料”，而不是明文密码。
/// 这里的 material 只是当前会话里可复用的中间态，不是长期缓存的最终秘密。
#[derive(Debug, Clone)]
pub(crate) struct WalletUnlockMaterial {
    smk: Vec<u8>,
}

impl WalletUnlockMaterial {
    pub(crate) fn new(smk: Vec<u8>) -> Self {
        Self { smk }
    }

    pub(crate) fn smk(&self) -> &[u8] {
        &self.smk
    }
}

impl Drop for WalletUnlockMaterial {
    fn drop(&mut self) {
        self.smk.zeroize();
    }
}

/// 钱包解锁会话是一个短期的“能力句柄”：
/// - session_token 用来表示当前会话已解锁
/// - wallet_materials 为每个钱包保存对应的解锁材料
/// - 到点后会进入轮换，但不会因为时间到就直接失能
#[derive(Debug, Clone)]
pub(crate) struct WalletUnlockSession {
    session_token: String,
    next_rotation_at: Instant,
    wallet_materials: HashMap<String, WalletUnlockMaterial>,
}

impl WalletUnlockSession {
    pub(crate) fn new(
        session_token: String,
        next_rotation_at: Instant,
        wallet_materials: HashMap<String, WalletUnlockMaterial>,
    ) -> Self {
        Self { session_token, next_rotation_at, wallet_materials }
    }

    pub(crate) fn session_token(&self) -> &str {
        &self.session_token
    }

    pub(crate) fn is_expired(&self) -> bool {
        Instant::now() >= self.next_rotation_at
    }

    pub(crate) fn wallet_material(&self, wallet_address: &str) -> Option<&WalletUnlockMaterial> {
        self.wallet_materials.get(wallet_address)
    }

    pub(crate) fn upsert_wallet_material(
        &mut self,
        wallet_address: String,
        wallet_material: WalletUnlockMaterial,
    ) {
        self.wallet_materials.insert(wallet_address, wallet_material);
    }

    pub(crate) fn next_rotation_at(&self) -> Instant {
        self.next_rotation_at
    }

    pub(crate) fn wallet_materials_snapshot(&self) -> HashMap<String, WalletUnlockMaterial> {
        self.wallet_materials.clone()
    }

    pub(crate) fn wallet_material_count(&self) -> usize {
        self.wallet_materials.len()
    }
}

pub(crate) struct WalletUnlockSessionCodec;

impl WalletUnlockSessionCodec {
    pub(crate) fn fingerprint_bytes(bytes: &[u8]) -> String {
        let digest = sha2::Sha256::digest(bytes);
        hex::encode(&digest[..8])
    }

    pub(crate) fn token_fingerprint(token: &str) -> String {
        Self::fingerprint_bytes(token.as_bytes())
    }

    pub(crate) fn unlock_session_rotation_interval_with_ctx(context: &Context) -> Duration {
        context.config().unlock_session_rotation_interval()
    }

    pub(crate) fn unlock_session_rotation_check_interval_with_ctx(context: &Context) -> Duration {
        context.config().unlock_session_rotation_check_interval()
    }

    pub(crate) fn unlock_session_rotation_interval() -> Duration {
        Duration::from_secs(
            crate::config::runtime_defaults::unlock_session().rotation_interval_secs,
        )
    }

    pub(crate) fn unlock_session_rotation_check_interval() -> Duration {
        Duration::from_secs(
            crate::config::runtime_defaults::unlock_session().rotation_check_interval_secs,
        )
    }

    pub(crate) fn generate_unlock_token() -> String {
        let mut token_bytes = vec![0u8; 32];
        let mut rng = rand::rngs::OsRng;
        rng.fill_bytes(&mut token_bytes);
        hex::encode(token_bytes)
    }

    pub(crate) async fn derive_smk(
        password: &str,
        salt: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, ServiceError> {
        let mut smk = vec![0u8; SEED_ENVELOPE_KEY_BYTES];
        let params = Params::default();
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        argon2.hash_password_into(password.as_bytes(), salt, &mut smk).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        Ok(Zeroizing::new(smk))
    }

    async fn derive_session_key(
        smk: &[u8],
        rotation_counter: u64,
    ) -> Result<Zeroizing<Vec<u8>>, ServiceError> {
        let hkdf = Hkdf::<sha2::Sha256>::new(None, smk);
        let mut session_key = vec![0u8; SEED_ENVELOPE_KEY_BYTES];
        let info = [SEED_ENVELOPE_HKDF_INFO, &rotation_counter.to_le_bytes()].concat();
        hkdf.expand(&info, &mut session_key).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        Ok(Zeroizing::new(session_key))
    }

    async fn derive_package_key(
        smk: &[u8],
        rotation_counter: u64,
    ) -> Result<Zeroizing<Vec<u8>>, ServiceError> {
        let hkdf = Hkdf::<sha2::Sha256>::new(None, smk);
        let mut package_key = vec![0u8; SEED_ENVELOPE_KEY_BYTES];
        let info = [SEED_PACKAGE_HKDF_INFO, &rotation_counter.to_le_bytes()].concat();
        hkdf.expand(&info, &mut package_key).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        Ok(Zeroizing::new(package_key))
    }

    async fn unwrap_dek(
        session_key: &[u8],
        wrapped_dek: &[u8],
        session_nonce: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, ServiceError> {
        if session_nonce.len() != SEED_ENVELOPE_NONCE_BYTES {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "invalid session nonce length: {}",
                    session_nonce.len()
                )),
            ));
        }

        let key = Key::<Aes256Gcm>::from_slice(session_key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(session_nonce);
        let dek = cipher.decrypt(nonce, wrapped_dek).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        Ok(Zeroizing::new(dek))
    }
}

pub(crate) struct SeedEnvelopeCodec;

impl SeedEnvelopeCodec {
    pub(crate) fn seed_fingerprint(seed: &[u8]) -> String {
        WalletUnlockSessionCodec::fingerprint_bytes(seed)
    }

    pub(crate) fn envelope_fingerprint(envelope: &SeedEnvelopeV1) -> String {
        let serialized = serde_json::to_vec(envelope).unwrap_or_default();
        WalletUnlockSessionCodec::fingerprint_bytes(&serialized)
    }

    pub(crate) fn package_fingerprint(package: &[u8]) -> String {
        WalletUnlockSessionCodec::fingerprint_bytes(package)
    }

    fn encode_seed_package(blob: &SeedEnvelopePackageV1) -> Result<Vec<u8>, ServiceError> {
        if blob.salt.len() > u8::MAX as usize {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "invalid seed blob salt length: {}",
                    blob.salt.len()
                )),
            ));
        }
        if blob.nonce.len() > u8::MAX as usize {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "invalid seed blob nonce length: {}",
                    blob.nonce.len()
                )),
            ));
        }

        let mut encoded = Vec::with_capacity(
            SEED_PACKAGE_MAGIC.len()
                + 1
                + 1
                + blob.salt.len()
                + 8
                + 1
                + blob.nonce.len()
                + blob.payload.len(),
        );
        encoded.extend_from_slice(SEED_PACKAGE_MAGIC);
        encoded.push(blob.version);
        encoded.push(blob.salt.len() as u8);
        encoded.extend_from_slice(&blob.salt);
        encoded.extend_from_slice(&blob.rotation_counter.to_le_bytes());
        encoded.push(blob.nonce.len() as u8);
        encoded.extend_from_slice(&blob.nonce);
        encoded.extend_from_slice(&blob.payload);
        Ok(encoded)
    }

    fn decode_seed_package(seed: &[u8]) -> Result<Option<SeedEnvelopePackageV1>, ServiceError> {
        if seed.len() < SEED_PACKAGE_MAGIC.len() + 1 + 1 + 8 + 1 {
            return Ok(None);
        }
        if seed.get(..SEED_PACKAGE_MAGIC.len()) != Some(SEED_PACKAGE_MAGIC.as_slice()) {
            return Ok(None);
        }

        let mut offset = SEED_PACKAGE_MAGIC.len();
        let version = *seed.get(offset).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal("invalid seed blob header".to_string()),
            )
        })?;
        offset += 1;

        let salt_len = *seed.get(offset).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal("invalid seed blob header".to_string()),
            )
        })? as usize;
        offset += 1;

        let salt = seed.get(offset..offset + salt_len).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "invalid seed blob salt length".to_string(),
                ),
            )
        })?;
        offset += salt_len;

        let rotation_counter_bytes = seed.get(offset..offset + 8).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "invalid seed blob rotation counter".to_string(),
                ),
            )
        })?;
        let rotation_counter = u64::from_le_bytes(rotation_counter_bytes.try_into().unwrap());
        offset += 8;

        let nonce_len = *seed.get(offset).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal("invalid seed blob header".to_string()),
            )
        })? as usize;
        offset += 1;

        let nonce = seed.get(offset..offset + nonce_len).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "invalid seed blob nonce length".to_string(),
                ),
            )
        })?;
        offset += nonce_len;

        let payload = seed.get(offset..).ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "invalid seed blob payload".to_string(),
                ),
            )
        })?;

        Ok(Some(SeedEnvelopePackageV1 {
            version,
            salt: salt.to_vec(),
            rotation_counter,
            nonce: nonce.to_vec(),
            payload: payload.to_vec(),
        }))
    }

    pub(crate) async fn encrypt_seed_bundle(
        password: &str,
        seed: &[u8],
    ) -> Result<Vec<u8>, ServiceError> {
        let mut salt = vec![0u8; SEED_ENVELOPE_SALT_BYTES];
        OsRng.fill_bytes(&mut salt);

        let smk = WalletUnlockSessionCodec::derive_smk(password, &salt).await?;
        Self::encrypt_seed_bundle_with_smk(&smk, &salt, seed, SEED_ENVELOPE_ROTATION_COUNTER).await
    }

    pub(crate) async fn encrypt_seed_bundle_with_smk(
        smk: &[u8],
        salt: &[u8],
        seed: &[u8],
        rotation_counter: u64,
    ) -> Result<Vec<u8>, ServiceError> {
        let session_key =
            WalletUnlockSessionCodec::derive_session_key(smk, rotation_counter).await?;

        let mut dek = vec![0u8; SEED_ENVELOPE_KEY_BYTES];
        let mut rng = rand::rngs::OsRng;
        rng.fill_bytes(&mut dek);

        let session_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&session_key));
        let session_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let wrapped_dek =
            session_cipher.encrypt(&session_nonce, dek.as_slice()).map_err(|err| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Internal(err.to_string()),
                )
            })?;

        let seed_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
        let seed_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let encrypted_seed = seed_cipher.encrypt(&seed_nonce, seed).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        let envelope = SeedEnvelopeV1 {
            version: SEED_ENVELOPE_VERSION_V1,
            salt: salt.to_vec(),
            rotation_counter,
            session_nonce: session_nonce.to_vec(),
            seed_nonce: seed_nonce.to_vec(),
            wrapped_dek,
            seed_cipher: encrypted_seed,
        };
        let envelope_json = serde_json::to_vec(&envelope).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;
        let package_key =
            WalletUnlockSessionCodec::derive_package_key(smk, rotation_counter).await?;
        let package_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&package_key));
        let package_nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let payload =
            package_cipher.encrypt(&package_nonce, envelope_json.as_slice()).map_err(|err| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Internal(err.to_string()),
                )
            })?;

        Self::encode_seed_package(&SeedEnvelopePackageV1 {
            version: SEED_PACKAGE_VERSION_V1,
            salt: salt.to_vec(),
            rotation_counter,
            nonce: package_nonce.to_vec(),
            payload,
        })
    }

    pub(crate) async fn decrypt_seed_bundle_with_smk(
        smk: &[u8],
        envelope: &SeedEnvelopeV1,
    ) -> Result<Vec<u8>, ServiceError> {
        let session_key =
            WalletUnlockSessionCodec::derive_session_key(smk, envelope.rotation_counter).await?;
        let dek = WalletUnlockSessionCodec::unwrap_dek(
            &session_key,
            &envelope.wrapped_dek,
            &envelope.session_nonce,
        )
        .await?;

        if envelope.seed_nonce.len() != SEED_ENVELOPE_NONCE_BYTES {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "invalid seed nonce length: {}",
                    envelope.seed_nonce.len()
                )),
            ));
        }

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
        let nonce = Nonce::from_slice(&envelope.seed_nonce);
        let seed = cipher.decrypt(nonce, envelope.seed_cipher.as_ref()).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        Ok(seed)
    }

    pub(crate) async fn decrypt_seed_envelope(
        password: &str,
        seed: &[u8],
    ) -> Result<SeedEnvelopeV1, ServiceError> {
        let blob = Self::decode_seed_package(seed)?.ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "unsupported seed package format".to_string(),
                ),
            )
        })?;
        if blob.version != SEED_PACKAGE_VERSION_V1 {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "unsupported seed package version: {}",
                    blob.version
                )),
            ));
        }

        let smk = WalletUnlockSessionCodec::derive_smk(password, &blob.salt).await?;
        let package_key =
            WalletUnlockSessionCodec::derive_package_key(smk.as_ref(), blob.rotation_counter)
                .await?;
        let package_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&package_key));
        let inner = package_cipher
            .decrypt(Nonce::from_slice(&blob.nonce), blob.payload.as_ref())
            .map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })?;

        serde_json::from_slice::<SeedEnvelopeV1>(&inner).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })
    }

    pub(crate) async fn decrypt_seed_bundle(
        password: &str,
        seed: &[u8],
    ) -> Result<Vec<u8>, ServiceError> {
        let envelope = Self::decrypt_seed_envelope(password, seed).await?;
        let smk = WalletUnlockSessionCodec::derive_smk(password, &envelope.salt).await?;
        Self::decrypt_seed_bundle_with_smk(&smk, &envelope).await
    }

    pub(crate) async fn decrypt_seed_envelope_with_smk(
        smk: &[u8],
        seed: &[u8],
    ) -> Result<SeedEnvelopeV1, ServiceError> {
        let blob = Self::decode_seed_package(seed)?.ok_or_else(|| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(
                    "unsupported seed package format".to_string(),
                ),
            )
        })?;
        if blob.version != SEED_PACKAGE_VERSION_V1 {
            return Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(format!(
                    "unsupported seed package version: {}",
                    blob.version
                )),
            ));
        }

        let blob_key =
            WalletUnlockSessionCodec::derive_package_key(smk, blob.rotation_counter).await?;
        let blob_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&blob_key));
        let inner = blob_cipher
            .decrypt(Nonce::from_slice(&blob.nonce), blob.payload.as_ref())
            .map_err(|err| {
                crate::error::service::ServiceError::System(
                    crate::error::system::SystemError::Internal(err.to_string()),
                )
            })?;

        serde_json::from_slice::<SeedEnvelopeV1>(&inner).map_err(|err| {
            crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Internal(err.to_string()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, time::Instant};
    use tokio::time::sleep;

    const TEST_PASSWORD: &str = "unlock-flow-password";
    const TEST_SEED: &[u8] = b"unlock-flow-seed";
    const TEST_SALT: [u8; SEED_ENVELOPE_SALT_BYTES] = [0x42; SEED_ENVELOPE_SALT_BYTES];
    const TEST_ROTATION_COUNTER: u64 = 7;

    #[tokio::test]
    async fn unlock_flow_package_roundtrip_logs() {
        eprintln!("[unlock-flow] 1) derive SMK from password + salt");
        let smk = WalletUnlockSessionCodec::derive_smk(TEST_PASSWORD, &TEST_SALT)
            .await
            .expect("derive smk");
        eprintln!("[unlock-flow] 1.1) SMK ready");

        let unlock_material = WalletUnlockMaterial::new(smk.to_vec());
        let mut wallet_materials = HashMap::new();
        wallet_materials.insert("0xunlock-flow".to_string(), unlock_material.clone());
        let unlock_session = WalletUnlockSession::new(
            "demo-unlock-token".to_string(),
            Instant::now() + Duration::from_secs(60),
            wallet_materials,
        );
        eprintln!("[unlock-flow] 2) unlock session ready");

        eprintln!("[unlock-flow] 3) encrypt seed into versioned envelope");
        let encrypted = SeedEnvelopeCodec::encrypt_seed_bundle_with_smk(
            unlock_material.smk(),
            &TEST_SALT,
            TEST_SEED,
            TEST_ROTATION_COUNTER,
        )
        .await
        .expect("encrypt seed bundle");
        eprintln!("[unlock-flow] 3.1) envelope serialized");
        assert!(!encrypted.starts_with(b"{"));

        eprintln!("[unlock-flow] 4) parse seed envelope");
        let blob = SeedEnvelopeCodec::decode_seed_package(&encrypted)
            .expect("parse blob envelope")
            .expect("blob envelope");
        eprintln!("[unlock-flow] 4.1) parsed blob header");

        let envelope = SeedEnvelopeCodec::decrypt_seed_envelope(TEST_PASSWORD, &encrypted)
            .await
            .expect("new envelope");
        eprintln!("[unlock-flow] 4.2) parsed envelope");

        eprintln!("[unlock-flow] 5) decrypt seed from unlock material");
        let decrypted =
            SeedEnvelopeCodec::decrypt_seed_bundle_with_smk(unlock_material.smk(), &envelope)
                .await
                .expect("decrypt seed bundle");
        eprintln!("[unlock-flow] 5.1) seed restored");

        assert_eq!(decrypted, TEST_SEED);
    }

    #[tokio::test]
    async fn unlock_flow_roundtrip_logs() {
        eprintln!("[unlock-flow] 1) encrypt seed into opaque envelope");
        let encrypted = SeedEnvelopeCodec::encrypt_seed_bundle(TEST_PASSWORD, TEST_SEED)
            .await
            .expect("encrypt seed bundle");
        eprintln!("[unlock-package] 1.1) package stored");
        assert!(!encrypted.starts_with(b"{"));

        eprintln!("[unlock-flow] 2) parse seed package header");
        let blob = SeedEnvelopeCodec::decode_seed_package(&encrypted)
            .expect("parse package envelope")
            .expect("package envelope");
        eprintln!("[unlock-package] 2.1) parsed package header");

        eprintln!("[unlock-flow] 3) decrypt seed envelope from opaque package");
        let envelope = SeedEnvelopeCodec::decrypt_seed_envelope(TEST_PASSWORD, &encrypted)
            .await
            .expect("decrypt opaque package");
        eprintln!("[unlock-package] 3.1) decrypted envelope");

        eprintln!("[unlock-flow] 4) decrypt seed from opaque envelope");
        let decrypted = SeedEnvelopeCodec::decrypt_seed_bundle_with_smk(
            &WalletUnlockSessionCodec::derive_smk(TEST_PASSWORD, &envelope.salt)
                .await
                .expect("derive smk"),
            &envelope,
        )
        .await
        .expect("decrypt opaque envelope");
        eprintln!("[unlock-package] 4.1) seed restored");

        assert_eq!(decrypted, TEST_SEED);
    }

    #[tokio::test]
    async fn unlock_flow_with_state_roundtrip_logs() {
        let smk = WalletUnlockSessionCodec::derive_smk(TEST_PASSWORD, &TEST_SALT)
            .await
            .expect("derive smk");
        let unlock_material = WalletUnlockMaterial::new(smk.to_vec());
        let encrypted = SeedEnvelopeCodec::encrypt_seed_bundle_with_smk(
            unlock_material.smk(),
            &TEST_SALT,
            TEST_SEED,
            TEST_ROTATION_COUNTER,
        )
        .await
        .expect("encrypt seed bundle with state");
        eprintln!("[unlock-flow] state roundtrip");
        assert!(!encrypted.starts_with(b"{"));

        let envelope =
            SeedEnvelopeCodec::decrypt_seed_envelope_with_smk(unlock_material.smk(), &encrypted)
                .await
                .expect("decrypt opaque envelope with state");
        let decrypted =
            SeedEnvelopeCodec::decrypt_seed_bundle_with_smk(unlock_material.smk(), &envelope)
                .await
                .expect("decrypt seed with state");

        eprintln!("[unlock-flow] state roundtrip recovered seed");
        assert_eq!(decrypted, TEST_SEED);
    }

    #[tokio::test]
    async fn unlock_flow_wrong_password_logs() {
        eprintln!("[unlock-flow] failure path: build envelope with known password");
        let encrypted = SeedEnvelopeCodec::encrypt_seed_bundle(TEST_PASSWORD, TEST_SEED)
            .await
            .expect("encrypt seed bundle");

        eprintln!("[unlock-flow] failure path: try decrypting with wrong password");
        let err = SeedEnvelopeCodec::decrypt_seed_envelope("wrong-password", &encrypted)
            .await
            .expect_err("wrong password must fail");
        eprintln!("[unlock-flow] failure path: got expected error: {err:?}");

        let debug = format!("{err:?}");
        assert!(!debug.contains("unlock-flow-seed"));
    }

    #[tokio::test]
    async fn unlock_session_rotation_interval_triggers_logs() {
        let ttl = WalletUnlockSessionCodec::unlock_session_rotation_interval();
        eprintln!("[unlock-ttl] 1) configured unlock session rotation interval = {:?}", ttl);
        assert!(ttl > Duration::from_millis(0));

        let unlock_session = WalletUnlockSession::new(
            "ttl-debug-token".to_string(),
            Instant::now() + ttl,
            HashMap::new(),
        );
        eprintln!("[unlock-ttl] 2) session created");
        assert!(!unlock_session.is_expired());

        let sleep_for = ttl + Duration::from_millis(100);
        eprintln!("[unlock-ttl] 3) sleeping for {:?} to cross rotation point", sleep_for);
        sleep(sleep_for).await;

        eprintln!("[unlock-ttl] 4) re-check rotation due state");
        assert!(unlock_session.is_expired());
    }
}
