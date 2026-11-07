pub mod config;
pub mod mqtt;
use config::ConfigDomain;
use wallet_crypto::KdfAlgorithm;
use wallet_database::{
    entities::{
        config::config_key::{KEYSTORE_KDF_ALGORITHM, WALLET_TREE_STRATEGY},
        device::DeviceEntity,
    },
    repositories::device::DeviceRepo,
};

use crate::{
    infrastructure::task_queue::backend::{BackendApiTask, BackendApiTaskData},
    service::device::APP_ID,
};

pub struct AppDomain<T> {
    phantom: std::marker::PhantomData<T>,
}
impl<T> Default for AppDomain<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> AppDomain<T> {
    pub fn new() -> Self {
        Self { phantom: std::marker::PhantomData }
    }
}

pub struct DeviceDomain;

impl DeviceDomain {
    pub fn device_content(
        device: &wallet_database::entities::device::DeviceEntity,
    ) -> Result<String, crate::error::service::ServiceError> {
        let identifier = DeviceDomain::device_identifier(&device.sn, &device.device_type);

        Ok(wallet_utils::ecb::Aes128EcbCryptor::new(APP_ID).unwrap().encrypt(&identifier).unwrap())
    }

    pub fn md5_sn(sn: &str) -> String {
        wallet_utils::md5(sn)
    }

    pub fn client_device_by_sn(sn: &str, device_type: &str) -> String {
        let identifier = DeviceDomain::device_identifier(sn, device_type);
        DeviceDomain::client_id_by_identifier(&identifier)
    }

    //  设备的唯一标识:(app_id,sn,device_type)
    pub fn device_identifier(sn: &str, device_type: &str) -> String {
        format!("{}_{}_{}", APP_ID, sn, device_type)
    }

    // 根据设备唯一标识计算：client_id
    pub fn client_id_by_identifier(identifier: &str) -> String {
        wallet_utils::md5(identifier)
    }

    pub fn client_id_by_device(
        device: &wallet_database::entities::device::DeviceEntity,
    ) -> Result<String, crate::error::service::ServiceError> {
        let identifier = DeviceDomain::device_identifier(&device.sn, &device.device_type);

        Ok(DeviceDomain::client_id_by_identifier(&identifier))
    }

    // pub(crate) async fn gen_device_bind_address_task_data(
    // ) -> Result<BackendApiTaskData, crate::ServiceError> {
    //     let device_bind_address_task_data = BackendApiTaskData::new(
    //         wallet_transport_backend::consts::endpoint::DEVICE_BIND_ADDRESS,
    //         &(),
    //     )?;
    //     Ok(device_bind_address_task_data)
    // }

    pub(crate) async fn gen_device_unbind_all_address_task_data(
        accounts: &[wallet_database::entities::account::AccountEntity],
        multisig_accounts: Vec<wallet_database::entities::multisig_account::MultisigAccountEntity>,
        sn: &str,
    ) -> Result<BackendApiTaskData, crate::error::service::ServiceError> {
        let mut device_unbind_address_req =
            wallet_transport_backend::request::DeviceBindAddressReq::new(sn);
        for account in accounts {
            device_unbind_address_req.push(&account.chain_code, &account.address);
        }
        for multisig_account in multisig_accounts {
            device_unbind_address_req.push(&multisig_account.chain_code, &multisig_account.address);
        }
        let device_unbind_address_task = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_UNBIND_ADDRESS,
            &device_unbind_address_req,
        )?;
        Ok(device_unbind_address_task)
    }

    pub(crate) async fn gen_device_unbind_all_api_address_task_data(
        accounts: &[wallet_database::entities::api_account::ApiAccountEntity],
        sn: &str,
    ) -> Result<BackendApiTaskData, crate::error::service::ServiceError> {
        let mut device_unbind_address_req =
            wallet_transport_backend::request::DeviceBindAddressReq::new(sn);
        for account in accounts {
            device_unbind_address_req.push(&account.chain_code, &account.address);
        }

        let device_unbind_address_task = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_UNBIND_ADDRESS,
            &device_unbind_address_req,
        )?;
        Ok(device_unbind_address_task)
    }

    pub(crate) async fn check_wallet_password_is_null(
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let sn = ctx.get_sn();
        let Some(device) = DeviceRepo::get_device_info(pool, sn).await? else {
            return Err(crate::error::business::BusinessError::Device(
                crate::error::business::device::DeviceError::Uninitialized,
            )
            .into());
        };

        let (keystore_kdf_algorithm, wallet_tree_strategy) = if device.password.is_some() {
            let keystore_kdf_algorithm = wallet_database::entities::config::KeystoreKdfAlgorithm {
                keystore_kdf_algorithm: KdfAlgorithm::Scrypt,
            };
            let wallet_tree_strategy = wallet_database::entities::config::WalletTreeStrategy {
                wallet_tree_strategy: wallet_tree::WalletTreeStrategy::V1,
            };
            (keystore_kdf_algorithm, wallet_tree_strategy)
        } else {
            let keystore_kdf_algorithm = wallet_database::entities::config::KeystoreKdfAlgorithm {
                keystore_kdf_algorithm: KdfAlgorithm::Argon2id,
            };
            let wallet_tree_strategy = wallet_database::entities::config::WalletTreeStrategy {
                wallet_tree_strategy: wallet_tree::WalletTreeStrategy::V2,
            };
            (keystore_kdf_algorithm, wallet_tree_strategy)
        };

        ConfigDomain::set_config(
            ctx,
            KEYSTORE_KDF_ALGORITHM,
            &keystore_kdf_algorithm.to_json_str()?,
        )
        .await?;
        ConfigDomain::set_config(ctx, WALLET_TREE_STRATEGY, &wallet_tree_strategy.to_json_str()?)
            .await?;
        Ok(())
    }

    pub(crate) async fn language_init(
        device_info: &DeviceEntity,
        language: &str,
    ) -> Result<BackendApiTask, crate::error::service::ServiceError> {
        let client_id = crate::domain::app::DeviceDomain::client_id_by_device(&device_info)?;
        let language_req = wallet_transport_backend::request::LanguageInitReq {
            client_id,
            lan: language.to_string(),
        };
        let language_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::LANGUAGE_INIT,
            &language_req,
        )?;

        Ok(BackendApiTask::BackendApi(language_init_task_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_identifier() {
        let identifier = DeviceDomain::device_identifier(
            // "14ae14461d0891116eb85ef447ecb28dc22781d987b5cb0f75f8d3bcca18ebed",
            "3cf5ee2bf4971c12306cf24a1a2fabfac2a97e895f994325c935babc022185d3",
            "ANDROID",
        );

        let client_id = DeviceDomain::client_id_by_identifier(&identifier);

        println!("identifier:{},client_id:{}", identifier, client_id);
    }
}
