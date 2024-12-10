use crate::{
    domain::{self, coin::CoinDomain},
    response_vo::chain::ChainAssets,
};
use wallet_database::{
    entities::chain::{ChainCreateVo, ChainEntity, ChainWithNode},
    repositories::{
        account::AccountRepoTrait, assets::AssetsRepoTrait, chain::ChainRepoTrait,
        coin::CoinRepoTrait, ResourcesRepo, TransactionTrait as _,
    },
};

pub struct ChainService {
    repo: ResourcesRepo,
    coin_domain: CoinDomain,
}

impl ChainService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self {
            repo,
            coin_domain: CoinDomain::new(),
        }
    }

    pub async fn add(
        self,
        name: &str,
        chain_code: &str,
        node_id: &str,
        protocols: &[String],
        main_symbol: &str,
    ) -> Result<(), crate::error::ServiceError> {
        let input = ChainCreateVo::new(name, chain_code, node_id, protocols, main_symbol);
        let mut tx = self.repo;
        tx.begin_transaction().await?;

        let _res = tx.add(input).await?;

        tx.commit_transaction().await?;

        Ok(())
    }

    pub async fn set_chain_node(
        self,
        chain_code: &str,
        node_id: &str,
    ) -> Result<(), crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        tx.set_chain_node(chain_code, node_id).await?;

        tx.commit_transaction().await?;

        Ok(())
    }

    pub async fn get_chain_list(self) -> Result<Vec<ChainEntity>, crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let res = tx.get_chain_list().await?;
        tx.commit_transaction().await?;

        Ok(res)
    }

    pub async fn get_market_chain_list(self) -> Result<Vec<String>, crate::error::ServiceError> {
        let mut tx = self.repo;
        let res = tx.get_market_chain_list().await?;
        Ok(res)
    }

    pub async fn get_chain_list_with_node_info(
        self,
    ) -> Result<Vec<ChainWithNode>, crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let res = tx.get_chain_node_list().await?;

        tx.commit_transaction().await?;
        Ok(res)
    }

    pub async fn get_protocol_list(
        self,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::error::ServiceError> {
        let mut tx = self.repo;
        tx.begin_transaction().await?;
        let res = ChainRepoTrait::detail(&mut tx, chain_code).await?;

        tx.commit_transaction().await?;
        Ok(res)
    }

    pub async fn get_chain_list_by_address_account_id_symbol(
        mut self,
        address: &str,
        account_id: Option<u32>,
        symbol: &str,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ChainAssets>, crate::ServiceError> {
        let mut tx = self.repo;
        let token_currencies = self.coin_domain.get_token_currencies_v2(&mut tx).await?;

        let pool = crate::manager::Context::get_global_sqlite_pool()?;

        let mut account_addresses = Vec::<String>::new();

        if let Some(is_multisig) = is_multisig {
            if is_multisig {
                // 查询多签账户下的资产
                let account =
                    domain::multisig::MultisigDomain::account_by_address(address, true, &pool)
                        .await?;
                account_addresses.push(account.address);
            } else {
                // 获取钱包下的这个账户的所有地址
                let accounts = tx
                    .get_account_list_by_wallet_address_and_account_id(Some(address), account_id)
                    .await?;
                for account in accounts {
                    if !account_addresses
                        .iter()
                        .any(|address| address == &account.address)
                    {
                        account_addresses.push(account.address);
                    }
                }
            }
        } else {
            // 获取钱包下的这个账户的所有地址
            let accounts = tx
                .get_account_list_by_wallet_address_and_account_id(Some(address), account_id)
                .await?;
            for account in accounts {
                if !account_addresses
                    .iter()
                    .any(|address| address == &account.address)
                {
                    account_addresses.push(account.address);
                }
            }
        }

        let datas = tx
            .get_assets_by_address(account_addresses, None, Some(symbol), is_multisig)
            .await?;

        let chains = tx.get_chain_list().await?;
        let res = token_currencies
            .calculate_chain_assets_list(datas, chains)
            .await?;

        Ok(res)
    }
}
