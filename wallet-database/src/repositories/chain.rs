use crate::entities::chain::{ChainCreateVo, ChainEntity, ChainWithNode};

#[async_trait::async_trait]
pub trait ChainRepoTrait: super::TransactionTrait {
    async fn add(&mut self, input: ChainCreateVo) -> Result<ChainEntity, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::upsert, input)
    }

    async fn set_chain_node(
        &mut self,
        chain_code: &str,
        node_id: &str,
    ) -> Result<Vec<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::set_chain_node, chain_code, node_id)
    }

    async fn get_chain_list(&mut self) -> Result<Vec<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::list,)
    }

    async fn toggle_chains_status(
        &mut self,
        chain_codes: Vec<String>,
    ) -> Result<Vec<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::toggle_chains_status, chain_codes)
    }

    async fn get_chain_node_list(&mut self) -> Result<Vec<ChainWithNode>, crate::Error> {
        let executor = self.get_conn_or_tx()?;

        crate::execute_with_executor!(executor, ChainEntity::list_with_node_info,)
    }

    async fn detail(&mut self, chain_code: &str) -> Result<Option<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::detail, chain_code)
    }

    async fn detail_by_id(&mut self, node_id: &str) -> Result<Option<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::detail_by_id, node_id)
    }

    async fn detail_with_main_symbol(
        &mut self,
        main_symbol: &str,
    ) -> Result<Option<ChainEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::detail_with_main_symbol, main_symbol)
    }

    async fn detail_with_node(
        &mut self,
        chain_code: &str,
    ) -> Result<Option<ChainWithNode>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, ChainEntity::chain_node_info, chain_code)
    }
}
