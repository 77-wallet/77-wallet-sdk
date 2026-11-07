use wallet_database::entities::api_wallet::ApiWalletType;
use wallet_transport_backend::response_vo::api_wallet::wallet::{
    QueryUidBindInfoRes, QueryWalletActivationInfoResp,
};

use crate::{
    api::ReturnType, manager::WalletManager, response_vo::api_wallet::wallet::ApiWalletList,
    service::api_wallet::wallet::ApiWalletService,
};

impl WalletManager {
    pub async fn get_api_wallet_list(&self) -> ReturnType<ApiWalletList> {
        ApiWalletService::new(self.ctx).get_api_wallet_list().await
    }

    pub async fn create_api_wallet(
        &self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        // account_name: &str,
        // is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> ReturnType<String> {
        ApiWalletService::new(self.ctx)
            .create_wallet(
                language_code,
                phrase,
                salt,
                wallet_name,
                wallet_password,
                invite_code,
                api_wallet_type,
                binding_address,
            )
            .await
    }

    pub async fn import_api_wallet(
        &self,
        language_code: u8,
        phrase: &str,
        salt: &str,
        wallet_name: &str,
        // account_name: &str,
        // is_default_name: bool,
        wallet_password: &str,
        invite_code: Option<String>,
        api_wallet_type: ApiWalletType,
        binding_address: Option<&str>,
    ) -> ReturnType<String> {
        ApiWalletService::new(self.ctx)
            .import_wallet(
                language_code,
                phrase,
                salt,
                wallet_name,
                // account_name,
                // is_default_name,
                wallet_password,
                invite_code,
                api_wallet_type,
                binding_address,
            )
            .await
    }

    pub async fn change_withdrawal_wallet(
        &self,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> ReturnType<()> {
        ApiWalletService::new(self.ctx).change_withdrawal_wallet(recharge_uid, withdrawal_uid).await
    }

    /// 查询绑定信息
    pub async fn query_uid_bind_info(&self, uid: &str) -> ReturnType<QueryUidBindInfoRes> {
        ApiWalletService::new(self.ctx).query_uid_bind_info(uid).await
    }

    pub async fn scan_bind(
        &self,
        org_app_id: &str,
        merchain_id: &str,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> ReturnType<()> {
        ApiWalletService::new(self.ctx)
            .scan_bind(org_app_id, merchain_id, recharge_uid, withdrawal_uid)
            .await
    }

    pub async fn import_bind(
        &self,
        sn: &str,
        org_id: &str,
        app_id: &str,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> ReturnType<()> {
        ApiWalletService::new(self.ctx)
            .import_bind(sn, org_id, app_id, recharge_uid, withdrawal_uid)
            .await
    }

    pub async fn get_api_phrase(
        &self,
        wallet_address: &str,
        password: &str,
    ) -> ReturnType<crate::response_vo::standard_wallet::wallet::GetPhraseRes> {
        ApiWalletService::new(self.ctx).get_phrase(wallet_address, password).await
    }

    pub async fn unbind_merchant(
        &self,
        recharge_uid: &str,
        withdrawal_uid: &str,
    ) -> ReturnType<()> {
        ApiWalletService::new(self.ctx).unbind_merchant(recharge_uid, withdrawal_uid).await
    }

    pub async fn edit_api_wallet_name(
        &self,
        wallet_name: &str,
        wallet_address: &str,
    ) -> ReturnType<()> {
        ApiWalletService::new(self.ctx).edit_wallet_name(wallet_address, wallet_name).await
    }

    pub async fn set_passwd_cache(&self, wallet_password: &str) -> ReturnType<()> {
        ApiWalletService::new(self.ctx).set_passwd_cache(wallet_password).await
    }

    pub async fn query_wallet_activation_info(
        &self,
        wallet_address: &str,
    ) -> ReturnType<QueryWalletActivationInfoResp> {
        ApiWalletService::new(self.ctx).query_wallet_activation_info(wallet_address).await
    }

    pub async fn physical_delete_api_wallet(&self, address: &str) -> ReturnType<()> {
        ApiWalletService::new(self.ctx).physical_delete(address).await
    }

    // pub async fn appid_withdrawal_wallet_change(
    //     &self,
    //     withdrawal_uid: &str,
    //     org_app_id: &str,
    // ) -> ReturnType<()> {
    //     ApiWalletService::new(self.ctx)
    //         .appid_withdrawal_wallet_change(withdrawal_uid, org_app_id)
    //         .await
    // }

    // 钱包是否在本设备有效
    pub async fn is_wallet_authorized_on_device(&self, wallet_address: &str) -> ReturnType<bool> {
        ApiWalletService::new(self.ctx).is_wallet_authorized_on_device(wallet_address).await
    }
}
