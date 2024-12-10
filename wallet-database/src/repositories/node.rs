use crate::entities::node::{NodeCreateVo, NodeEntity};

#[async_trait::async_trait]
pub trait NodeRepoTrait: super::TransactionTrait {
    async fn add(&mut self, input: NodeCreateVo) -> Result<NodeEntity, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, NodeEntity::upsert, input)
    }

    async fn list(&mut self) -> Result<Vec<NodeEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, NodeEntity::list,)
    }

    async fn get_node_list_in_chain_codes(
        &mut self,
        chain_codes: Vec<&str>,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(
            executor,
            NodeEntity::get_node_list_in_chain_codes,
            chain_codes
        )
    }

    async fn detail(&mut self, node_id: &str) -> Result<Option<NodeEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        let req = crate::entities::node::QueryReq::new(node_id);
        crate::execute_with_executor!(executor, NodeEntity::detail, &req)
    }

    async fn delete(&mut self, rpc_url: &str, chain_code: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, NodeEntity::delete, rpc_url, chain_code)
    }
}
