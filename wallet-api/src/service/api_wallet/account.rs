use crate::{
    application::account::ApiAccountApplication,
    context::Context,
    error::service::ServiceError,
    messaging::mqtt::topics::api_wallet::cmd::address_allock::AddressAllockType,
    response_vo::{
        api_wallet::account::{
            ApiAccountInfo, ApiWalletAddressSearchResp, QueryApiAccountDerivationPath,
        },
        standard_wallet::account::DerivedAddressesList,
    },
};
use wallet_chain_interact::types::ChainPrivateKey;
use wallet_database::{entities::api_wallet::ApiWalletType, pagination::Pagination};

pub struct ApiAccountService {
    ctx: &'static Context,
}

impl ApiAccountService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    // pub async fn list_api_accounts(
    //     &self,
    //     wallet_address: &str,
    //     account_id: Option<u32>,
    //     chain_code: Option<String>,
    //     page: i64,
    //     page_size: i64,
    // ) -> Result<Pagination<ApiAccountInfo>, ServiceError> {
    //     ApiAccountDomain::list_api_accounts(wallet_address, account_id, chain_code, page, page_size)
    //         .await
    //     // ApiAccountDomain::list_api_accounts_v2(
    //     //     wallet_address,
    //     //     account_id,
    //     //     chain_code,
    //     //     page,
    //     //     page_size,
    //     // )
    //     // .await
    // }

    pub async fn list_api_accounts_v2(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<ApiAccountInfo>, ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .list_api_accounts_v2(wallet_address, account_id, chain_code, page, page_size)
            .await
    }

    pub async fn expand_address(
        &self,
        address_allock_type: AddressAllockType,
        chain_code: &str,
        index: Option<i32>,
        uid: &str,
        number: u32,
        serial_no: &str,
        batch_id: &str,
    ) -> Result<(), ServiceError> {
        ApiAccountApplication::new(self.ctx)
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

    pub async fn create_account(
        &self,
        wallet_address: &str,
        wallet_password: &str,
        // derivation_path: Option<String>,
        indices: Vec<i32>,
        name: &str,
        is_default_name: bool,
        api_wallet_type: ApiWalletType,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .create_account(
                wallet_address,
                wallet_password,
                indices,
                name,
                is_default_name,
                api_wallet_type,
            )
            .await
    }

    pub async fn create_withdrawal_account(
        &self,
        wallet_address: &str,
        wallet_password: &str,
        derivation_path: Option<String>,
        index: Option<i32>,
        name: &str,
        is_default_name: bool,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .create_withdrawal_account(
                wallet_address,
                wallet_password,
                derivation_path,
                index,
                name,
                is_default_name,
            )
            .await
    }

    pub async fn get_account_private_key(
        &self,
        address: &str,
        chain_code: &str,
        password: &str,
    ) -> Result<ChainPrivateKey, crate::error::service::ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .get_account_private_key(address, chain_code, password)
            .await
    }

    pub async fn address_used(
        &self,
        chain_code: &str,
        index: i32,
        uid: &str,
    ) -> Result<(), ServiceError> {
        ApiAccountApplication::new(self.ctx).address_used(chain_code, index, uid).await
    }

    pub async fn edit_account_name(
        &self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<(), ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .edit_account_name(account_id, wallet_address, name)
            .await
    }

    pub async fn physical_delete_account(
        self,
        wallet_address: &str,
        account_id: u32,
        password: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .physical_delete_account(wallet_address, account_id, password)
            .await
    }

    pub async fn list_derived_addresses(
        self,
        wallet_address: &str,
        index: i32,
        password: &str,
        all: bool,
    ) -> Result<Vec<DerivedAddressesList>, crate::error::service::ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .list_derived_addresses(wallet_address, index, password, all)
            .await
    }

    pub async fn get_account_derivation_path(
        self,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<QueryApiAccountDerivationPath>, ServiceError> {
        ApiAccountApplication::new(self.ctx)
            .get_account_derivation_path(wallet_address, account_id)
            .await
    }

    /// 地址搜索：在指定 API 钱包 uid 范围内搜索账户地址
    pub async fn search_address(
        self,
        uid: &str,
        keyword: &str,
    ) -> Result<ApiWalletAddressSearchResp, ServiceError> {
        ApiAccountApplication::new(self.ctx).search_api_wallet_address(uid, keyword).await
    }
}
