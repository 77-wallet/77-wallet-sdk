use crate::{
    context::Context,
    domain::{self, address_book::AddressBookDomain},
    response_vo::standard_wallet::address_book::AddressBookResp,
};
use wallet_database::{
    CoreDbPool,
    entities::address_book::AddressBookEntity,
    pagination::Pagination,
    repositories::{address_book::AddressBookRepo, bill::BillRepo},
};

pub struct AddressBookService {
    pub pool: CoreDbPool,
}

impl AddressBookService {
    pub fn new(pool: CoreDbPool) -> Self {
        Self { pool }
    }

    pub fn new_with_ctx(
        ctx: &'static Context,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        Ok(Self { pool })
    }
}

impl AddressBookService {
    pub async fn create(
        self,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::error::service::ServiceError> {
        AddressBookDomain::check_address(address.to_string(), chain_code.to_string()).await?;

        let condition = vec![("address", address), ("chain_code", chain_code)];
        let res = AddressBookRepo::find_by_conditions(&self.pool, condition).await?;
        if res.is_some() {
            return Err(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::AddressRepeat,
            ))?;
        }

        Ok(AddressBookRepo::insert(&self.pool, name, address, chain_code).await?)
    }

    pub async fn update(
        self,
        id: u32,
        name: &str,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AddressBookEntity>, crate::error::service::ServiceError> {
        AddressBookDomain::check_address(address.to_string(), chain_code.to_string()).await?;

        let res = AddressBookRepo::check_not_self(&self.pool, id, address, chain_code).await?;
        if res.is_some() {
            return Err(crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::AddressRepeat,
            ))?;
        }

        Ok(AddressBookRepo::update(&self.pool, id, name, address, chain_code).await?)
    }

    pub async fn delete(self, id: i32) -> Result<(), crate::error::service::ServiceError> {
        Ok(AddressBookRepo::delete(&self.pool, id).await?)
    }

    pub async fn lists(
        self,
        chain_code: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<AddressBookEntity>, crate::error::service::ServiceError> {
        Ok(AddressBookRepo::list(&self.pool, chain_code, page, page_size).await?)
    }

    pub async fn check_address(
        self,
        address: String,
        chain_code: String,
    ) -> Result<(), crate::error::service::ServiceError> {
        let net = wallet_types::chain::network::NetworkKind::Mainnet;

        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code.as_ref())?;

        // check address format is right
        crate::domain::chain::check_address(&address, chain, net)?;

        Ok(())
    }

    pub async fn find_by_address(
        self,
        address: String,
        chain_code: String,
    ) -> Result<AddressBookResp, crate::error::service::ServiceError> {
        // find address book
        let address_book =
            AddressBookRepo::find_by_address(&self.pool, &address, &chain_code).await?;

        // check is first transfer
        let bill = BillRepo::first_transfer(&address, &chain_code, &self.pool).await?;

        Ok(AddressBookResp { address_book, first_transfer: bill.is_none() })
    }

    // 查询地址的动态状态 0 正常的状态 1冻结
    pub async fn address_status(
        &self,
        address: String,
        chain_code: String,
        ctx: &'static Context,
    ) -> Result<i64, crate::error::service::ServiceError> {
        let chain = wallet_types::chain::chain::ChainCode::try_from(chain_code.as_ref())?;

        // query address is black
        let adapter =
            domain::chain::adapter::ChainAdapterFactory::get_transaction_adapter_with_ctx(
                &ctx,
                &chain_code,
            )
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
                #[cfg(feature = "prod")]
                {
                    &wallet_types::constant::check_black::TRON
                }
                #[cfg(not(feature = "prod"))]
                {
                    &wallet_types::constant::check_black::TRON_TESTNET
                }
            }
            _ => &[],
        };

        for token in token_address {
            tracing::info!("token: {:?}", token);
            if adapter.black_address(chain, token, &address).await? {
                return Ok(1);
            }
        }
        Ok(0)
    }
}
