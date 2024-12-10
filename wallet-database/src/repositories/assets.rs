use crate::{
    dao::assets::CreateAssetsVo,
    entities::assets::{AssetsEntity, AssetsEntityWithAddressType, AssetsId},
};

// pub struct ChainRepo {
//     // pub repo: ResourcesRepo,
// }

// impl ChainRepo {
//     pub fn new(db_pool: crate::DbPool) -> Self {
//         Self {
//             // repo: ResourcesRepo::new(db_pool),
//         }
//     }
// }

// impl ChainRepoTrait for ChainRepo {}

#[async_trait::async_trait]
pub trait AssetsRepoTrait: super::TransactionTrait {
    async fn upsert_assets(&mut self, assets: CreateAssetsVo) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::upsert_assets, assets)
    }

    async fn update_is_multisig(&mut self, id: &AssetsId) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::update_is_multisig, id)
    }

    async fn update_balance(&mut self, id: &AssetsId, balance: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::update_balance, id, balance)
    }

    async fn update_status(
        &mut self,
        chain_code: &str,
        symbol: &str,
        token_address: Option<String>,
        status: u8,
    ) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::update_status,
            chain_code,
            symbol,
            token_address,
            status
        )
    }

    async fn unactived_list(&mut self) -> Result<Vec<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::unactived_list,)
    }

    async fn assets_by_id(&mut self, id: &AssetsId) -> Result<Option<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::assets_by_id, id)
    }

    async fn get_chain_assets_by_address_chain_code_symbol(
        &mut self,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::get_chain_assets_by_address_chain_code_symbol,
            address,
            chain_code,
            symbol,
            is_multisig
        )
    }

    async fn get_assets_by_address(
        &mut self,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntityWithAddressType>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::get_assets_by_address,
            address,
            chain_code,
            symbol,
            is_multisig
        )
    }

    async fn get_coin_assets_in_address_all_status(
        &mut self,
        addresses: Vec<String>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::get_coin_assets_in_address,
            addresses,
            None
        )
    }

    async fn get_coin_assets_in_address(
        &mut self,
        addresses: Vec<String>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::get_coin_assets_in_address,
            addresses,
            Some(1)
        )
    }

    async fn lists(
        &mut self,
        addr: Vec<String>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            AssetsEntity::all_assets,
            addr,
            chain_code,
            keyword,
            is_multisig
        )
    }

    async fn delete_multi_assets(&mut self, assets_ids: Vec<AssetsId>) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, AssetsEntity::delete_multi_assets, assets_ids)
    }
}
