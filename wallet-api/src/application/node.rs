use crate::{
    context::Context,
    domain::node::NodeDomain,
    error::service::ServiceError,
    response_vo::standard_wallet::chain::{NodeDynData, NodeListRes},
};
use wallet_database::{
    entities::node::NodeCreateVo,
    repositories::{chain::ChainRepo, node::NodeRepo},
};

pub(crate) struct NodeApplication {
    ctx: &'static Context,
}

impl NodeApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn add_node(
        &self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        http_url: Option<String>,
    ) -> Result<String, ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let id = NodeDomain::gen_node_id(name, chain_code);
        let req = NodeCreateVo::new(&id, name, chain_code, rpc_url, http_url);
        let res = NodeRepo::upsert(&core_pool, req).await.map_err(ServiceError::Database)?;
        Ok(res.node_id)
    }

    pub(crate) async fn get_node_list(
        &self,
        chain_code: &str,
    ) -> Result<Vec<NodeListRes>, ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let Some(chain) = ChainRepo::detail(&core_pool, chain_code).await? else {
            return Err(crate::error::business::BusinessError::Chain(
                crate::error::business::chain::ChainError::NotFound(chain_code.to_string()),
            )
            .into());
        };

        let node_list =
            NodeRepo::get_node_list_in_chain_codes(&core_pool, &[chain_code.to_string()], Some(1))
                .await?;

        let res = node_list
            .into_iter()
            .map(|node| {
                let status = if chain.node_id == Some(node.node_id.clone()) { 1 } else { 0 };
                NodeListRes {
                    node_id: node.node_id,
                    name: node.name,
                    chain_code: node.chain_code,
                    rpc_url: node.rpc_url,
                    ws_url: node.ws_url,
                    status,
                }
            })
            .collect();

        Ok(res)
    }

    pub(crate) async fn get_node_dynamic_data(
        &self,
        chain_code: &str,
    ) -> Result<Vec<NodeDynData>, ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let list_with_node =
            NodeRepo::get_node_list_in_chain_codes(&core_pool, &[chain_code.to_string()], Some(1))
                .await?;

        let mut res = Vec::new();
        for node in list_with_node {
            res.push(NodeDomain::probe_dynamic_data(self.ctx, chain_code, &node).await?);
        }

        Ok(res)
    }

    pub(crate) async fn get_node_dynamic_data_by_id(
        &self,
        chain_code: &str,
        node_id: &str,
    ) -> Result<NodeDynData, ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        let Some(node) = NodeRepo::detail(&core_pool, node_id).await? else {
            return Err(node_not_found());
        };

        if node.chain_code != chain_code || node.status != 1 {
            return Err(node_not_found());
        }

        NodeDomain::probe_dynamic_data_with_timeout(self.ctx, chain_code, &node).await
    }
}

fn node_not_found() -> ServiceError {
    crate::error::business::BusinessError::ChainNode(
        crate::error::business::chain_node::ChainNodeError::NodeNotFound,
    )
    .into()
}
