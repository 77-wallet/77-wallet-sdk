use crate::{domain::api_wallet::unlock::SeedEnvelopeCodec, error::service::ServiceError};

pub struct PhrasePackageCodec;

impl PhrasePackageCodec {
    pub async fn encrypt_phrase(password: &str, phrase: &str) -> Result<Vec<u8>, ServiceError> {
        SeedEnvelopeCodec::encrypt_seed_bundle(password, phrase.as_bytes()).await
    }

    pub async fn decrypt_phrase(password: &str, phrase: &[u8]) -> Result<String, ServiceError> {
        let data = SeedEnvelopeCodec::decrypt_seed_bundle(password, phrase).await?;
        Ok(wallet_utils::conversion::vec_to_string(&data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::PhrasePackageCodec;
    use std::sync::Once;

    static TEST_TRACING: Once = Once::new();

    const TEST_PASSWORD: &str = "test-password";

    fn init_test_tracing() {
        TEST_TRACING.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_max_level(tracing::Level::INFO)
                .try_init();
        });
    }

    #[tokio::test]
    async fn phrase_package_roundtrip_logs() {
        init_test_tracing();
        let phrase = "phrase-package-roundtrip";

        let encoded = PhrasePackageCodec::encrypt_phrase(TEST_PASSWORD, phrase)
            .await
            .expect("encrypt phrase package");
        eprintln!("[phrase-package] 1) stored phrase blob");
        assert!(encoded.starts_with(b"wb1\0"));
        assert_ne!(encoded, phrase.as_bytes());

        let decoded = PhrasePackageCodec::decrypt_phrase(TEST_PASSWORD, &encoded)
            .await
            .expect("decrypt phrase package");
        eprintln!("[phrase-package] 2) decrypted phrase roundtrip");
        assert_eq!(decoded, phrase);
    }

    #[tokio::test]
    async fn phrase_package_rejects_wrong_password() {
        init_test_tracing();
        let phrase = "phrase-package-roundtrip";

        let encoded = PhrasePackageCodec::encrypt_phrase(TEST_PASSWORD, phrase)
            .await
            .expect("encrypt phrase package");

        let err = PhrasePackageCodec::decrypt_phrase("wrong-password", &encoded)
            .await
            .expect_err("wrong password must fail");
        let debug = format!("{err:?}");
        assert!(!debug.contains(phrase));
    }
}
