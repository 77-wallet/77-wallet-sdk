use crate::entities::account::{AccountEntity, CreateAccountVo};

// use super::TransactionTrait;

pub struct AccountRepo {
    repo: super::ResourcesRepo,
}

impl std::ops::Deref for AccountRepo {
    type Target = super::ResourcesRepo;

    fn deref(&self) -> &Self::Target {
        &self.repo
    }
}

impl std::ops::DerefMut for AccountRepo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.repo
    }
}

// crate::delegate_transaction_trait!(AccountRepo, self.repo);

#[async_trait::async_trait]
pub trait AccountRepoTrait: super::TransactionTrait {
    async fn upsert_multi_account(
        &mut self,
        input: Vec<CreateAccountVo>,
    ) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::upsert_multi_account, input)
    }

    async fn detail(&mut self, address: &str) -> Result<Option<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::account::QueryReq {
            wallet_address: None,
            address: Some(address.to_string()),
            chain_code: None,
            account_id: None,
            status: Some(1),
        };
        crate::execute_with_executor!(executor, AccountEntity::detail, &req)
    }

    async fn detail_by_address_and_chain_code(
        &mut self,
        address: &str,
        chain_code: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::account::QueryReq {
            wallet_address: None,
            address: Some(address.to_string()),
            chain_code: Some(chain_code.to_string()),
            account_id: None,
            status: Some(1),
        };
        crate::execute_with_executor!(executor, AccountEntity::detail, &req)
    }

    async fn detail_by_wallet_address_and_account_id_and_chain_code(
        &mut self,
        wallet_address: &str,
        account_id: u32,
        chain_code: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::account::QueryReq {
            wallet_address: Some(wallet_address.to_string()),
            address: None,
            chain_code: Some(chain_code.to_string()),
            account_id: Some(account_id),
            status: Some(1),
        };
        crate::execute_with_executor!(executor, AccountEntity::detail, &req)
    }

    async fn edit_account_name(
        &mut self,
        account_id: u32,
        wallet_address: &str,
        name: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::edit_account_name,
            account_id,
            wallet_address,
            name
        )
    }

    async fn account_detail_by_max_id_and_wallet_address(
        &mut self,
        wallet_address: &str,
    ) -> Result<Option<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_detail_by_max_id_and_wallet_address,
            wallet_address
        )
    }

    async fn has_account_id(
        &mut self,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<bool, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::has_account_id,
            wallet_address,
            account_id
        )
    }

    async fn account_init(
        &mut self,
        address: &str,
        chain_code: &str,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::init, address, chain_code)
    }

    async fn account(&mut self, address: &str) -> Result<Option<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::account::QueryReq {
            wallet_address: None,
            address: Some(address.to_string()),
            chain_code: None,
            account_id: None,
            status: Some(1),
        };
        crate::execute_with_executor!(executor, AccountEntity::detail, &req)
    }

    async fn list(&mut self) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_list,
            None,
            None,
            None,
            vec![],
            None
        )
    }

    async fn get_account_list_by_wallet_address(
        &mut self,
        wallet_address: Option<&str>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_list,
            wallet_address,
            None,
            None,
            vec![],
            None
        )
    }

    async fn get_account_list_by_wallet_address_and_account_id(
        &mut self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_list,
            wallet_address,
            None,
            None,
            vec![],
            account_id
        )
    }

    async fn account_list_by_wallet_address_and_account_id_and_chain_codes(
        &mut self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_codes: Vec<String>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_list,
            wallet_address,
            None,
            None,
            chain_codes,
            account_id
        )
    }

    async fn account_list_by_wallet_address_and_chain_code(
        &mut self,
        wallet_address: Option<&str>,
        chain_code: Option<String>,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let chain_code = if let Some(chain_code) = chain_code {
            vec![chain_code]
        } else {
            vec![]
        };
        crate::execute_with_executor!(
            executor,
            AccountEntity::account_list,
            wallet_address,
            None,
            None,
            chain_code,
            None
        )
    }

    async fn reset(&mut self, wallet_address: &str) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::reset_account, wallet_address)
    }

    async fn reset_all_account(&mut self) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::reset_all_account,)
    }

    async fn restart(&mut self, wallet_address: &str) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::restart_account, wallet_address)
    }

    async fn physical_delete_all(
        &mut self,
        wallet_address: &[&str],
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AccountEntity::physical_delete_all, wallet_address)
    }

    async fn physical_delete(
        &mut self,
        wallet_address: &str,
        account_id: u32,
    ) -> Result<Vec<AccountEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::physical_delete,
            wallet_address,
            account_id
        )
    }

    async fn count_unique_account_ids(
        &mut self,
        wallet_address: &str,
    ) -> Result<u32, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AccountEntity::count_unique_account_ids,
            wallet_address
        )
    }
}
