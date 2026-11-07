use crate::{
    api::ReturnType, manager::WalletManager,
    response_vo::standard_wallet::address_book::AddressBookResp,
    service::address_book::AddressBookService,
};
use wallet_database::{entities::address_book::AddressBookEntity, pagination::Pagination};

impl WalletManager {
    pub async fn create_address_book(
        &self,
        name: String,
        address: String,
        chain_code: String,
    ) -> ReturnType<Option<AddressBookEntity>> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;
        service.create(&name, &address, &chain_code).await
    }

    pub async fn update_address_book(
        &self,
        id: u32,
        name: String,
        address: String,
        chain_code: String,
    ) -> ReturnType<Option<AddressBookEntity>> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;

        service.update(id, &name, &address, &chain_code).await
    }

    pub async fn delete_address_book(&self, id: i32) -> ReturnType<()> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;

        service.delete(id).await
    }

    pub async fn list_address_book(
        &self,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<AddressBookEntity>> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;

        service.lists(chain_code.as_deref(), page, page_size).await
    }

    pub async fn is_valid_address(&self, address: String, chain_code: String) -> ReturnType<()> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;

        service.check_address(address, chain_code).await
    }

    pub async fn find_by_address(
        &self,
        address: String,
        chain_code: String,
    ) -> ReturnType<AddressBookResp> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;
        service.find_by_address(address, chain_code).await
    }

    pub async fn address_status(&self, address: String, chain_code: String) -> ReturnType<i64> {
        let service = AddressBookService::new_with_ctx(self.ctx)?;

        service.address_status(address, chain_code, self.ctx).await
    }
}

mod test {

    #[tokio::test]
    #[ignore = "smoke/live: queries address status through a configured chain RPC"]
    async fn test_address_status() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (wallet_manager, _test_params) = crate::testkit::env::get_manager().await?;
        let res = wallet_manager
            .address_status("TUDrRQ6zvwXhW3ScTxwGv8nwicLShVVWoF".to_string(), "tron".to_string())
            .await?;
        let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
        tracing::info!("res: {res:?}");
        Ok(())
    }
}
