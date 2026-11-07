use wallet_types::chain::{
    address::r#type::{
        AddressType, BTC_ADDRESS_TYPES, DOG_ADDRESS_TYPES, LTC_ADDRESS_TYPES, TON_ADDRESS_TYPES,
    },
    chain::ChainCode,
};

use crate::{
    application::wallet::WalletApplication, context::Context, error::service::ServiceError,
};

const DEFAULT_SALT: &str = "salt";

pub struct WalletDomain {}

impl Default for WalletDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl WalletDomain {
    pub fn new() -> Self {
        Self {}
    }

    pub(crate) fn encrypt_password(password: &str, salt: &str) -> Result<String, ServiceError> {
        let encrypted_password = wallet_utils::pbkdf2_string(
            password,
            &format!("{}{}", salt, DEFAULT_SALT),
            100000,
            32,
        )?;
        Ok(encrypted_password)
    }

    pub fn address_type_by_chain(chian: ChainCode) -> Vec<AddressType> {
        match chian {
            ChainCode::Bitcoin => BTC_ADDRESS_TYPES.to_vec(),
            ChainCode::Dogcoin => DOG_ADDRESS_TYPES.to_vec(),
            ChainCode::Litecoin => LTC_ADDRESS_TYPES.to_vec(),
            ChainCode::Ton => TON_ADDRESS_TYPES.to_vec(),
            _ => vec![AddressType::Other],
        }
    }

    pub(crate) async fn validate_password_with_context(
        ctx: &'static Context,
        password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::validate_password(ctx, password).await
    }

    pub(crate) async fn validate_password_with_proof_status(
        ctx: &'static Context,
        password: &str,
    ) -> Result<Option<String>, ServiceError> {
        WalletApplication::validate_password_with_proof_status(ctx, password).await
    }

    /// Re-authenticate a changed stored proof under the caller's credential gate.
    /// This also checks the fixed proof plaintext, not just the keystore MAC.
    pub(crate) async fn revalidate_password_proof(
        proof: &str,
        password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::decrypt_password_proof(proof, password).await.map_err(|_| {
            crate::error::business::BusinessError::Wallet(
                crate::error::business::wallet::WalletError::PasswordIncorrect,
            )
            .into()
        })
    }

    /// Eligibility only: caller must supply the snapshot authenticated by validation.
    /// Constructing expected KDF parameters does not derive a password key.
    pub(crate) async fn password_proof_matches_current_policy(
        ctx: &'static Context,
        proof: &str,
    ) -> Result<bool, ServiceError> {
        use wallet_crypto::{
            EncryptedJsonGenerator, KeystoreJsonGenerator,
            kdf::{KdfParams, factory::KdfFactory},
        };
        let algorithm =
            crate::domain::app::config::ConfigDomain::get_keystore_kdf_algorithm(ctx).await?;
        let proof: wallet_crypto::KeystoreJson = wallet_utils::serde_func::serde_from_str(proof)?;
        let salt: &[u8] = match &proof.crypto.kdfparams {
            KdfParams::Pbkdf2(params) => &params.salt,
            KdfParams::Scrypt(params) => &params.salt,
            KdfParams::Argon2id(params) => &params.salt,
        };
        Ok(proof.crypto.kdf == algorithm
            && salt.len() == <KeystoreJsonGenerator<rand::rngs::OsRng> as EncryptedJsonGenerator>::DEFAULT_KEY_SIZE
            && proof.crypto.kdfparams == KdfFactory::create(&algorithm, salt)?.params())
    }

    pub(crate) async fn upgrade_algorithm_with_context(
        ctx: &'static Context,
        password: &str,
    ) -> Result<(), ServiceError> {
        WalletApplication::upgrade_algorithm(ctx, password).await
    }

    pub(crate) async fn get_seed(
        ctx: &'static Context,
        wallet_address: &str,
        wallet_password: &str,
    ) -> Result<Vec<u8>, ServiceError> {
        WalletApplication::get_seed(ctx, wallet_address, wallet_password).await
    }

    pub(crate) async fn restart_existing_wallet(
        &self,
        core_pool: wallet_database::CoreDbPool,
        address: &str,
    ) -> Result<std::collections::HashSet<u32>, ServiceError> {
        WalletApplication::restart_existing_wallet(core_pool, address).await
    }

    pub(crate) async fn check_api_wallet_exist_with_context(
        ctx: &'static Context,
        address: &str,
    ) -> Result<bool, ServiceError> {
        WalletApplication::check_api_wallet_exist(ctx, address).await
    }

    pub(crate) async fn generate_password_proof(
        ctx: &'static Context,
        password: &str,
    ) -> Result<String, ServiceError> {
        WalletApplication::generate_password_proof(ctx, password).await
    }
}
