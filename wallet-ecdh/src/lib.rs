use crate::{
    data::EncryptedData,
    encryption::{decrypt_with_shared_secret, encrypt_with_shared_secret},
    error::EncryptionError,
    sign::{sign_with_derived_ecdsa, verify_derived_ecdsa_signature},
};
use k256::{PublicKey, SecretKey, ecdh, ecdh::SharedSecret, ecdsa::Signature};
use once_cell::sync::Lazy;
use rand_core::OsRng;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

pub mod data;
pub mod encryption;
pub mod error;
pub mod sign;

pub static GLOBAL_KEY: Lazy<Arc<ExKey>> = Lazy::new(|| {
    Arc::new(ExKey::try_new().unwrap_or_else(|err| {
        panic!("failed to initialize GLOBAL_KEY: {err}");
    }))
});

/// `ExKey` stores process-level ECDH session state used by existing wallet flows.
/// It is stateful and not a pure crypto helper, so tests must reset shared state explicitly.
pub struct ExKey {
    sn: RwLock<String>,
    secret: SecretKey,
    shared_secret: RwLock<Option<SharedSecret>>,
}

impl ExKey {
    fn try_new() -> Result<Self, EncryptionError> {
        // Generate once per instance; GLOBAL_KEY reuses it for the process lifetime.
        // Reconnecting or repeating the handshake must not rotate this key.
        let secret = SecretKey::random(&mut OsRng);
        Ok(Self { sn: RwLock::new(String::new()), secret, shared_secret: RwLock::new(None) })
    }

    pub fn new() -> Self {
        Self::try_new().unwrap_or_else(|err| panic!("failed to initialize ExKey: {err}"))
    }

    pub fn set_sn(&self, sn: &str) {
        match self.sn.write() {
            Ok(mut w) => *w = sn.to_string(),
            Err(poisoned) => {
                let mut w = poisoned.into_inner();
                *w = sn.to_string();
                tracing::warn!("sn lock poisoned; value updated with recovered guard");
            }
        }
    }

    pub fn sn(&self) -> String {
        match self.sn.read() {
            Ok(v) => v.to_string(),
            Err(poisoned) => {
                tracing::warn!("sn lock poisoned; reading value from recovered guard");
                poisoned.into_inner().to_string()
            }
        }
    }

    pub fn secret_pub_key(&self) -> String {
        let pub_key = self.secret.public_key();
        pub_key.to_string()
    }

    #[cfg(test)]
    pub fn reset_shared_secret_for_test(&self) -> Result<(), EncryptionError> {
        let mut sn = self.sn.write().map_err(|_| EncryptionError::LockPoisoned)?;
        *sn = String::new();
        drop(sn);

        let mut shared_secret =
            self.shared_secret.write().map_err(|_| EncryptionError::LockPoisoned)?;
        *shared_secret = None;
        Ok(())
    }

    pub fn set_shared_secret(&self, s: &str) -> Result<(), crate::error::EncryptionError> {
        // let pem_string = wallet_utils::base64_to_bytes(s)?;
        let bob_public = PublicKey::from_str(s).map_err(|_| EncryptionError::InvalidPubKey)?;
        let shared_key =
            ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), bob_public.as_affine());
        let mut w = self.shared_secret.write().map_err(|_| EncryptionError::LockPoisoned)?;
        *w = Some(shared_key);
        Ok(())
    }

    pub fn is_exchange_shared_secret(&self) -> Result<(), EncryptionError> {
        let r = self.shared_secret.read().map_err(|_| EncryptionError::LockPoisoned)?;
        if r.is_some() { Ok(()) } else { Err(EncryptionError::InvalidSharedKey) }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedData, EncryptionError> {
        let key = Uuid::new_v4().to_string();
        let r = self.shared_secret.read().map_err(|_| EncryptionError::LockPoisoned)?;
        if let Some(shared_secret) = &*r {
            encrypt_with_shared_secret(plaintext, &shared_secret, key.as_bytes())
        } else {
            Err(EncryptionError::InvalidSharedKey)
        }
    }

    pub fn decrypt(&self, plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let r = self.shared_secret.read().map_err(|_| EncryptionError::LockPoisoned)?;
        if let Some(shared_secret) = &*r {
            decrypt_with_shared_secret(plaintext, shared_secret, key)
        } else {
            Err(EncryptionError::InvalidSharedKey)
        }
    }

    pub fn sign(&self, tag: &str, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let key = plaintext.get(..32).ok_or(EncryptionError::InvalidSigningInput)?;
        // tracing::info!(tag = tag, "Got signing key: {:?}", hex::encode(key));
        let r = self.shared_secret.read().map_err(|_| EncryptionError::LockPoisoned)?;
        if let Some(shared_secret) = &*r {
            let res = sign_with_derived_ecdsa(tag, plaintext, shared_secret, key)?;
            Ok(res.to_vec())
        } else {
            Err(EncryptionError::InvalidSharedKey)
        }
    }

    pub fn verify(&self, tag: &str, plaintext: &[u8], sig: &[u8]) -> Result<(), EncryptionError> {
        let key = plaintext.get(..32).ok_or(EncryptionError::InvalidSigningInput)?;
        // tracing::info!(tag = tag, "verify, Got seed key: {:?}", hex::encode(key));
        let signature = if sig.len() == 64 {
            // tracing::info!(tag = tag, "signature length 64");
            // tracing::info!(
            //     tag = tag,
            //     "msg hash = {}",
            //     hex::encode(sha2::Sha256::digest(plaintext))
            // );
            // tracing::info!(tag = tag, "r = {}", hex::encode(&sig[..32]));
            // tracing::info!(tag = tag, "s = {}", hex::encode(&sig[32..]));
            Signature::from_slice(sig).map_err(|_| EncryptionError::InvalidSignature)?
        } else {
            // tracing::info!(tag = tag, "signature length 32");
            Signature::from_der(sig).map_err(|_| EncryptionError::InvalidSignature)?
        };

        // tracing::info!(tag = tag, "r = {}", signature.r());
        // tracing::info!(tag = tag, "s = {}", signature.s());

        let r = self.shared_secret.read().map_err(|_| EncryptionError::LockPoisoned)?;
        if let Some(shared_secret) = &*r {
            verify_derived_ecdsa_signature(tag, plaintext, &signature, shared_secret, key)
        } else {
            Err(EncryptionError::InvalidSharedKey)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExKey, GLOBAL_KEY};

    const TEST_PEER_PUBLIC_KEY: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEWDZNP0ClbeWJey9hBr2rsjSayQEBywnv
ZXi0RberQCAp+06fOjvr+jZI5qwYGglmMkGJw49tbni6qgm4QNV6WQ==
-----END PUBLIC KEY-----"#;

    #[test]
    fn exkey_new_does_not_panic() {
        let key = ExKey::new();
        assert!(!key.secret_pub_key().is_empty());
    }

    #[test]
    fn set_shared_secret_then_encrypt_decrypt_roundtrip()
    -> Result<(), crate::error::EncryptionError> {
        let key = ExKey::new();
        key.set_shared_secret(TEST_PEER_PUBLIC_KEY)?;

        let plaintext = b"wallet-ecdh-roundtrip-payload";
        let encrypted = key.encrypt(plaintext)?;
        let decrypted = key.decrypt(&encrypted.ciphertext, &encrypted.key)?;

        assert_eq!(decrypted, plaintext);
        Ok(())
    }

    #[test]
    fn global_key_test_helper_resets_state() -> Result<(), crate::error::EncryptionError> {
        GLOBAL_KEY.set_sn("sn-before-reset");
        GLOBAL_KEY.set_shared_secret(TEST_PEER_PUBLIC_KEY)?;
        GLOBAL_KEY.reset_shared_secret_for_test()?;

        assert_eq!(GLOBAL_KEY.sn(), "");
        assert!(matches!(
            GLOBAL_KEY.is_exchange_shared_secret(),
            Err(crate::error::EncryptionError::InvalidSharedKey)
        ));
        Ok(())
    }

    #[test]
    fn operations_fail_when_shared_secret_not_set() {
        let key = ExKey::new();

        let encrypt_err = key.encrypt(b"payload").expect_err("encrypt must fail");
        assert!(matches!(encrypt_err, crate::error::EncryptionError::InvalidSharedKey));

        let decrypt_err = key.decrypt(b"cipher", b"key").expect_err("decrypt must fail");
        assert!(matches!(decrypt_err, crate::error::EncryptionError::InvalidSharedKey));

        let sign_err =
            key.sign("tag", b"01234567890123456789012345678901").expect_err("sign must fail");
        assert!(matches!(sign_err, crate::error::EncryptionError::InvalidSharedKey));
    }
}

#[cfg(test)]
mod key_lifecycle_tests;
