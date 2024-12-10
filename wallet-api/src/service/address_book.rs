use crate::{domain, response_vo::address_book::AddressBookResp};
use wallet_database::{
    dao::bill::BillDao, entities::address_book::AddressBookEntity, pagination::Pagination,
    repositories::address_book::AddressBookRepo,
};

pub struct AddressBookService {
    pub repo: AddressBookRepo,
}

impl AddressBookService {
    pub async fn create(
        self,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::ServiceError> {
        self.check_address(address.to_string(), chain_code.to_string())
            .await?;

        let condition = vec![("address", address), ("chain_code", chain_code)];
        let res = self.repo.find_by_conditions(condition).await?;
        if res.is_some() {
            return Err(crate::BusinessError::Account(
                crate::AccountError::AddressRepeat,
            ))?;
        }

        Ok(self.repo.insert(name, address, chain_code).await?)
    }

    pub async fn update(
        self,
        id: u32,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::ServiceError> {
        self.check_address(address.to_string(), chain_code.to_string())
            .await?;

        let res = self.repo.check_not_self(id, address, chain_code).await?;
        if res.is_some() {
            return Err(crate::BusinessError::Account(
                crate::AccountError::AddressRepeat,
            ))?;
        }

        Ok(self.repo.update(id, name, address, chain_code).await?)
    }

    pub async fn delete(self, id: i32) -> Result<(), crate::ServiceError> {
        Ok(self.repo.delete(id).await?)
    }

    pub async fn lists(
        self,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AddressBookEntity>, crate::ServiceError> {
        Ok(self.repo.list(chain_code, page, page_size).await?)
    }

    pub async fn check_address(
        &self,
        address: String,
        chain_code: String,
    ) -> Result<(), crate::ServiceError> {
        let net = wallet_types::chain::network::NetworkKind::Mainnet;

        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code.as_ref())?;

        // check address format is right
        crate::domain::chain::check_address(&address, chain, net)?;

        Ok(())
    }

    pub async fn find_by_address(
        &self,
        address: String,
        chain_code: String,
    ) -> Result<AddressBookResp, crate::ServiceError> {
        // find address book
        let address_book = self.repo.find_by_address(&address, &chain_code).await?;

        // check is first transfer
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let bill = BillDao::first_transfer(&address, &chain_code, pool.as_ref())
            .await
            .map_err(|e| crate::ServiceError::Database(e.into()))?;

        Ok(AddressBookResp {
            address_book,
            first_transfer: bill.is_none(),
        })
    }

    // 查询地址的动态状态 0 正常的状态 1冻结
    pub async fn address_status(
        &self,
        address: String,
        chain_code: String,
    ) -> Result<i64, crate::ServiceError> {
        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code.as_ref())?;

        // query address is black
        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter(&chain_code)
                .await?;

        let token_address = match chain {
            wallet_types::chain::chain::ChainCode::Bitcoin => {
                wallet_types::constant::check_black::BTC
            }
            wallet_types::chain::chain::ChainCode::Solana => {
                wallet_types::constant::check_black::SOLANA
            }
            wallet_types::chain::chain::ChainCode::Ethereum => {
                wallet_types::constant::check_black::ETH
            }
            wallet_types::chain::chain::ChainCode::BnbSmartChain => {
                wallet_types::constant::check_black::BNB
            }
            wallet_types::chain::chain::ChainCode::Tron => {
                wallet_types::constant::check_black::TRON
            }
        };

        for token in token_address {
            if adapter.black_address(chain, token, &address).await? {
                return Ok(1);
            }
        }
        Ok(0)
    }
}
