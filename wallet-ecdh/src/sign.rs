use crate::error::EncryptionError;
use hkdf::Hkdf;
use k256::{
    PublicKey, SecretKey,
    ecdh::SharedSecret,
    ecdsa::{
        Signature, SigningKey,
        signature::{Signer, Verifier},
    },
    sha2::Sha256,
};

// 从 ECDH 共享密钥派生 ECDSA 密钥对
fn derive_ecdsa_from_shared_secret(
    _tag: &str,
    shared_secret: &SharedSecret,
    key: &[u8],
) -> Result<(SecretKey, PublicKey), EncryptionError> {
    // 1. 使用 HKDF 从共享密钥派生私钥
    let shared_bytes = shared_secret.raw_secret_bytes();
    let hkdf = Hkdf::<Sha256>::new(None, shared_bytes);

    // 2. 派生 ECDSA 私钥
    let mut private_key_bytes = [0u8; 32];
    hkdf.expand(key, &mut private_key_bytes)
        .map_err(|e| EncryptionError::KeyDerivationFailed(e.to_string()))?;

    // 3. 创建 ECDSA 密钥对
    let secret_key = SecretKey::from_bytes(&private_key_bytes.into())?;
    // tracing::info!(tag = tag, "Got sign secret key: {:?}", hex::encode(secret_key.to_bytes()));
    let public_key = secret_key.public_key();
    Ok((secret_key, public_key))
}

// 使用派生的 ECDSA 密钥进行签名
pub(crate) fn sign_with_derived_ecdsa(
    tag: &str,
    message: &[u8],
    shared_secret: &SharedSecret,
    key: &[u8],
) -> Result<Signature, EncryptionError> {
    let (secret_key, _) = derive_ecdsa_from_shared_secret(tag, shared_secret, key)?;
    // 创建 SigningKey
    let signing_key = SigningKey::from_bytes(&secret_key.to_bytes())?;
    // tracing::info!("Got sign key: {:?}", hex::encode(secret_key.to_bytes()));

    // 生成签名
    let (signature, _) = signing_key.sign(message);

    // tracing::info!(
    //     tag = tag,
    //     "Got sign signature: {:?}, {}",
    //     signature.to_bytes(),
    //     signature.to_bytes().len()
    // );
    // tracing::info!("{}", hex::encode(signature.to_bytes()));

    Ok(signature)
}

// 验证使用派生 ECDSA 密钥的签名
pub(crate) fn verify_derived_ecdsa_signature(
    tag: &str,
    message: &[u8],
    signature: &Signature,
    shared_secret: &SharedSecret,
    key: &[u8],
) -> Result<(), EncryptionError> {
    let (_, public_key) = derive_ecdsa_from_shared_secret(tag, shared_secret, key)?;
    let verifying_key = k256::ecdsa::VerifyingKey::from(public_key);
    // tracing::info!(tag = tag, "verifying_key: {:?}", verifying_key.to_sec1_bytes());
    let res = verifying_key.verify(message, signature);
    match res {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::error!(tag = tag, "failed to verify signature, error: {:?}", err);
            Err(EncryptionError::SignatureError(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{sign_with_derived_ecdsa, verify_derived_ecdsa_signature};
    use crate::error::EncryptionError;
    use k256::ecdh::EphemeralSecret;
    use rand_core::OsRng;

    #[test]
    fn shared_secret_matches_for_both_peers() {
        let alice_secret = EphemeralSecret::random(&mut OsRng);
        let alice_public = alice_secret.public_key();
        let bob_secret = EphemeralSecret::random(&mut OsRng);
        let bob_public = bob_secret.public_key();

        let alice_shared = alice_secret.diffie_hellman(&bob_public);
        let bob_shared = bob_secret.diffie_hellman(&alice_public);

        assert_eq!(alice_shared.raw_secret_bytes(), bob_shared.raw_secret_bytes());
    }

    #[test]
    fn sign_and_verify_roundtrip() -> Result<(), EncryptionError> {
        let alice_secret = EphemeralSecret::random(&mut OsRng);
        let bob_secret = EphemeralSecret::random(&mut OsRng);
        let shared_secret1 = alice_secret.diffie_hellman(&bob_secret.public_key());
        let shared_secret2 = bob_secret.diffie_hellman(&alice_secret.public_key());

        let message = b"Hello, ECDSA derived from ECDH!";
        let key = b"ecdsa_private_key";
        let signature = sign_with_derived_ecdsa("1111", message, &shared_secret1, key)?;
        verify_derived_ecdsa_signature("1111", message, &signature, &shared_secret2, key)?;
        Ok(())
    }

    #[test]
    fn verify_fails_for_modified_message() -> Result<(), EncryptionError> {
        let alice_secret = EphemeralSecret::random(&mut OsRng);
        let bob_secret = EphemeralSecret::random(&mut OsRng);
        let shared_secret1 = alice_secret.diffie_hellman(&bob_secret.public_key());
        let shared_secret2 = bob_secret.diffie_hellman(&alice_secret.public_key());

        let message = b"Hello, ECDSA derived from ECDH!";
        let modified_message = b"hello, ECDSA derived from ECDH!";
        let key = b"ecdsa_private_key";
        let signature = sign_with_derived_ecdsa("1111", message, &shared_secret1, key)?;

        let err = verify_derived_ecdsa_signature(
            "1111",
            modified_message,
            &signature,
            &shared_secret2,
            key,
        )
        .expect_err("modified message must fail verification");
        assert!(matches!(err, EncryptionError::SignatureError(_)));
        Ok(())
    }
}
