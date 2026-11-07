use wallet_database::pagination::Pagination;

use crate::{
    api::ReturnType,
    manager::WalletManager,
    messaging::mqtt::topics::api_wallet::cmd::address_allock::AddressAllockType,
    request::api_wallet::account::{CreateApiAccountReq, CreateWithdrawalAccountReq},
    response_vo::{
        api_wallet::account::{
            ApiAccountInfo, ApiWalletAddressSearchResp, QueryApiAccountDerivationPath,
        },
        standard_wallet::account::DerivedAddressesList,
    },
    service::api_wallet::account::ApiAccountService,
};

impl WalletManager {
    pub async fn list_api_wallet_account(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain: Option<String>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<ApiAccountInfo>> {
        tracing::info!(
            wallet_address = %wallet_address,
            account_id = ?account_id,
            chain = ?chain,
            page,
            page_size,
            "WalletManager::list_api_wallet_account"
        );
        ApiAccountService::new(self.ctx)
            .list_api_accounts_v2(wallet_address, account_id, chain, page, page_size)
            .await
    }

    pub async fn get_api_account_derivation_path(
        &self,
        wallet_address: &str,
        index: u32,
    ) -> ReturnType<Vec<QueryApiAccountDerivationPath>> {
        ApiAccountService::new(self.ctx).get_account_derivation_path(wallet_address, index).await
    }

    pub async fn create_api_account(&self, req: CreateApiAccountReq) -> ReturnType<()> {
        ApiAccountService::new(self.ctx)
            .create_account(
                &req.wallet_address,
                &req.wallet_password,
                req.indices,
                &req.name,
                req.is_default_name,
                req.api_wallet_type,
            )
            .await
    }

    pub async fn create_withdrawal_account(
        &self,
        req: CreateWithdrawalAccountReq,
    ) -> ReturnType<()> {
        ApiAccountService::new(self.ctx)
            .create_withdrawal_account(
                &req.wallet_address,
                &req.wallet_password,
                req.derivation_path,
                req.index,
                &req.name,
                req.is_default_name,
            )
            .await
    }

    #[allow(unused)]
    pub async fn expand_address(
        &self,
        address_allock_type: AddressAllockType,
        chain_code: &str,
        index: Option<i32>,
        uid: &str,
        number: u32,
        serial_no: &str,
        batch_id: &str,
    ) -> ReturnType<()> {
        ApiAccountService::new(self.ctx)
            .expand_address(
                address_allock_type,
                chain_code,
                index,
                uid,
                number,
                serial_no,
                batch_id,
            )
            .await
    }

    pub async fn get_api_account_private_key(
        &self,
        address: &str,
        chain_code: &str,
        password: &str,
    ) -> ReturnType<String> {
        let res = ApiAccountService::new(self.ctx)
            .get_account_private_key(address, chain_code, password)
            .await?;
        Ok(res.to_string())
    }

    pub async fn address_used(&self, chain_code: &str, index: i32, uid: &str) -> ReturnType<()> {
        ApiAccountService::new(self.ctx).address_used(chain_code, index, uid).await
    }

    pub async fn physical_delete_api_account(
        &self,
        wallet_address: &str,
        account_id: u32,
        password: &str,
    ) -> ReturnType<()> {
        ApiAccountService::new(self.ctx)
            .physical_delete_account(wallet_address, account_id, password)
            .await
    }

    // pub async fn get_api_account_list(
    //     &self,
    //     wallet_address: Option<&str>,
    //     account_id: Option<u32>,
    // ) -> ReturnType<Vec<ApiAccountEntity>> {
    //     ApiAccountService::new(self.ctx).get_account_list(wallet_address, account_id).await
    // }

    pub async fn edit_api_account_name(
        &self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> ReturnType<()> {
        ApiAccountService::new(self.ctx).edit_account_name(account_id, wallet_address, name).await
    }

    pub async fn list_api_wallet_derived_addresses(
        &self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> ReturnType<Vec<DerivedAddressesList>> {
        ApiAccountService::new(self.ctx)
            .list_derived_addresses(wallet_address, index, password, all)
            .await
    }

    /// 地址搜索：在指定 API 钱包 uid 范围内搜索账户地址
    pub async fn search_api_wallet_address(
        &self,
        uid: &str,
        keyword: &str,
    ) -> ReturnType<ApiWalletAddressSearchResp> {
        tracing::info!(
            uid = %uid,
            keyword = %keyword,
            "WalletManager::search_api_wallet_address"
        );
        ApiAccountService::new(self.ctx).search_address(uid, keyword).await
    }
}
