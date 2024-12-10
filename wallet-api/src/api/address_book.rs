use crate::{
    api::ReturnType, response_vo::address_book::AddressBookResp,
    service::address_book::AddressBookService,
};
use wallet_database::{entities::address_book::AddressBookEntity, pagination::Pagination};

impl crate::WalletManager {
    pub async fn create_address_book(
        &self,
        name: String,
        address: String,
        chain_code: String,
    ) -> ReturnType<Option<AddressBookEntity>> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };
        service.create(&name, &address, &chain_code).await?.into()
    }

    pub async fn update_address_book(
        &self,
        id: u32,
        name: String,
        address: String,
        chain_code: String,
    ) -> ReturnType<Option<AddressBookEntity>> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };

        service
            .update(id, &name, &address, &chain_code)
            .await?
            .into()
    }

    pub async fn delete_address_book(&self, id: i32) -> ReturnType<()> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };

        service.delete(id).await?.into()
    }

    pub async fn list_address_book(
        &self,
        chain_code: Option<String>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<AddressBookEntity>> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };

        service
            .lists(chain_code.as_deref(), page, page_size)
            .await?
            .into()
    }

    pub async fn is_valid_address(&self, address: String, chain_code: String) -> ReturnType<()> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };

        service.check_address(address, chain_code).await?.into()
    }

    pub async fn find_by_address(
        &self,
        address: String,
        chain_code: String,
    ) -> ReturnType<AddressBookResp> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };
        service.find_by_address(address, chain_code).await?.into()
    }

    pub async fn address_status(&self, address: String, chain_code: String) -> ReturnType<i64> {
        let service = AddressBookService {
            repo: self.repo_factory.address_book_repo(),
        };

        service.address_status(address, chain_code).await?.into()
    }
}
