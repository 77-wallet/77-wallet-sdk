pub mod config;
use wallet_database::entities::account::AccountEntity;

use crate::service::device::APP_ID;

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
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

pub struct DeviceDomain;

impl DeviceDomain {
    pub fn device_content(
        device: &wallet_database::entities::device::DeviceEntity,
    ) -> Result<String, crate::ServiceError> {
        let identifier = DeviceDomain::device_identifier(&device.sn, &device.device_type);

        Ok(wallet_utils::ecb::Aes128EcbCryptor::new(APP_ID)
            .unwrap()
            .encrypt(&identifier)
            .unwrap())
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
    ) -> Result<String, crate::ServiceError> {
        let identifier = DeviceDomain::device_identifier(&device.sn, &device.device_type);

        Ok(DeviceDomain::client_id_by_identifier(&identifier))
    }

    pub(crate) async fn gen_device_bind_address_task_data(
        sn: &str,
    ) -> Result<super::task_queue::BackendApiTaskData, crate::ServiceError> {
        let pool = crate::Context::get_global_sqlite_pool()?;

        let mut device_bind_address_req =
            wallet_transport_backend::request::DeviceBindAddressReq::new(sn);

        let accounts = AccountEntity::account_list(&*pool, None, None, None, vec![], None).await?;
        let multisig_accounts =
            wallet_database::dao::multisig_account::MultisigAccountDaoV1::find_owner_on_chain_account(&*pool)
                .await
                .map_err(|e| crate::ServiceError::Database(wallet_database::Error::Database(e)))?;

        for account in accounts {
            device_bind_address_req.push(&account.chain_code, &account.address);
        }
        for multisig_account in multisig_accounts {
            device_bind_address_req.push(&multisig_account.chain_code, &multisig_account.address);
        }
        let device_bind_address_task_data = crate::domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_BIND_ADDRESS,
            &device_bind_address_req,
        )?;
        Ok(device_bind_address_task_data)
    }

    pub(crate) async fn gen_device_unbind_all_address_task_data(
        accounts: &Vec<wallet_database::entities::account::AccountEntity>,
        multisig_accounts: Vec<wallet_database::entities::multisig_account::MultisigAccountEntity>,
        sn: &str,
    ) -> Result<super::task_queue::BackendApiTaskData, crate::ServiceError> {
        let mut device_unbind_address_req =
            wallet_transport_backend::request::DeviceBindAddressReq::new(sn);
        for account in accounts {
            device_unbind_address_req.push(&account.chain_code, &account.address);
        }
        for multisig_account in multisig_accounts {
            device_unbind_address_req.push(&multisig_account.chain_code, &multisig_account.address);
        }
        let device_unbind_address_task = crate::domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::DEVICE_UNBIND_ADDRESS,
            &device_unbind_address_req,
        )?;
        Ok(device_unbind_address_task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_identifier() {
        let identifier = DeviceDomain::device_identifier(
            // "14ae14461d0891116eb85ef447ecb28dc22781d987b5cb0f75f8d3bcca18ebed",
            "bdb6412a9cb4b12c48ebe1ef4e9f052b07af519b7485cd38a95f38d89df97cb8",
            "ANDROID",
        );

        let client_id = DeviceDomain::client_id_by_identifier(&identifier);

        println!("identifier:{},client_id:{}", identifier, client_id);
    }
}
