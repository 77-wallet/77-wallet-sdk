use crate::{application::node::NodeApplication, context::Context};

pub struct NodeService {
    ctx: &'static Context,
}

impl NodeService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }
}

impl NodeService {
    pub async fn add_node(
        &self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        _ws_url: &str,
        http_url: Option<String>,
    ) -> Result<String, crate::error::service::ServiceError> {
        NodeApplication::new(self.ctx).add_node(name, chain_code, rpc_url, http_url).await
    }

    // // 首先在没有请求后端接口的情况下，只需要初始化默认的链信息和节点信息
    // // 然后请求后端接口，获取后端默认的链信息和节点信息，然后更新到数据库中
    // pub async fn init_node_info(&mut self) -> Result<(), crate::error::service::ServiceError> {
    //     let tx = &mut self.repo;

    //     let mut chains_set = std::collections::HashSet::new();
    //     Self::init_default_nodes(tx, &mut chains_set).await?;
    //     tracing::debug!("init_default_nodes done chains_set: {:?}", chains_set);
    //     NodeDomain::prune_nodes(tx, &mut chains_set, Some(1)).await?;
    //     if let Err(e) = NodeDomain::process_backend_nodes().await {
    //         tracing::error!("Failed to process default nodes: {:?}", e);
    //     }

    //     Ok(())
    // }

    pub async fn get_node_list(
        &self,
        chain_code: &str,
    ) -> Result<
        Vec<crate::response_vo::standard_wallet::chain::NodeListRes>,
        crate::error::service::ServiceError,
    > {
        NodeApplication::new(self.ctx).get_node_list(chain_code).await
    }

    // 包括块高、延迟
    pub async fn get_node_dynamic_data(
        &self,
        chain_code: &str,
    ) -> Result<
        Vec<crate::response_vo::standard_wallet::chain::NodeDynData>,
        crate::error::service::ServiceError,
    > {
        NodeApplication::new(self.ctx).get_node_dynamic_data(chain_code).await
    }

    pub async fn get_node_dynamic_data_by_id(
        &self,
        chain_code: &str,
        node_id: &str,
    ) -> Result<
        crate::response_vo::standard_wallet::chain::NodeDynData,
        crate::error::service::ServiceError,
    > {
        NodeApplication::new(self.ctx).get_node_dynamic_data_by_id(chain_code, node_id).await
    }
}
