use crate::domain::{self, node::NodeDomain};
use wallet_database::{
    entities::node::NodeCreateVo,
    repositories::{chain::ChainRepoTrait, node::NodeRepoTrait, ResourcesRepo},
};
pub struct NodeService {
    pub repo: ResourcesRepo,
    // keystore: wallet_keystore::Keystore
}

impl NodeService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self { repo }
    }

    pub async fn add_node(
        &mut self,
        name: &str,
        chain_code: &str,
        rpc_url: &str,
        _ws_url: &str,
    ) -> Result<String, crate::ServiceError> {
        let tx = &mut self.repo;
        let req = NodeCreateVo::new(name, chain_code, rpc_url);
        let res = NodeRepoTrait::add(tx, req)
            .await
            .map_err(crate::SystemError::Database)?;
        Ok(res.node_id)
    }

    pub async fn init_node_info(&mut self) -> Result<(), crate::ServiceError> {
        let list = crate::default_data::chains::init_default_chains_list()?;
        let tx = &mut self.repo;

        let mut default_nodes = Vec::new();

        for default_chain in &list.chains {
            let node = NodeCreateVo::new(
                &default_chain.node_name,
                &default_chain.chain_code,
                &default_chain.rpc_url,
            )
            .with_http_url(&default_chain.http_url)
            .with_network(&default_chain.network);

            let node = match NodeRepoTrait::add(tx, node).await {
                Ok(node) => node,
                Err(e) => {
                    tracing::error!("Failed to create default node: {:?}", e);
                    continue;
                }
            };
            default_nodes.push(wallet_types::valueobject::NodeData::new(
                &node.node_id,
                &node.rpc_url,
                &node.chain_code,
            ));

            if !default_chain.active {
                continue;
            }
            let req = wallet_database::entities::chain::ChainCreateVo::new(
                &default_chain.name,
                &default_chain.chain_code,
                &node.node_id,
                &default_chain.protocols,
                &default_chain.main_symbol,
            );

            if let Err(e) = ChainRepoTrait::add(tx, req).await {
                tracing::error!("Failed to create default chain: {:?}", e);
                continue;
            }
        }

        tokio::spawn(async move {
            if let Err(e) = NodeDomain::process_backend_nodes(default_nodes).await {
                tracing::error!("Failed to process default nodes: {:?}", e);
            }
        });

        Ok(())
    }

    pub async fn get_node_list(
        &mut self,
        chain_code: &str,
    ) -> Result<Vec<crate::response_vo::chain::NodeListRes>, crate::ServiceError> {
        let tx = &mut self.repo;

        let Some(chain) = ChainRepoTrait::detail(tx, chain_code).await? else {
            return Err(crate::ServiceError::Business(crate::BusinessError::Chain(
                crate::ChainError::NotFound(chain_code.to_string()),
            )));
        };

        let node_list = NodeRepoTrait::get_node_list_in_chain_codes(tx, vec![chain_code]).await?;

        let res = node_list
            .into_iter()
            .map(|node| {
                let status = if chain.node_id == node.node_id { 1 } else { 0 };
                crate::response_vo::chain::NodeListRes {
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

    // 包括块高、延迟
    pub async fn get_node_dynamic_data(
        &mut self,
        chain_code: &str,
    ) -> Result<Vec<crate::response_vo::chain::NodeDynData>, crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        // let node_list = self.get_node_list(chain_code).await?;
        let list_with_node =
            wallet_database::entities::node::NodeEntity::get_node_list_in_chain_codes(
                &*pool,
                vec![chain_code],
            )
            .await?;
        let mut res = Vec::new();
        for node in list_with_node {
            let name = node.name.clone();
            let node_id = node.node_id.clone();
            let chain_instance =
                domain::chain::adapter::ChainAdapterFactory::get_node_transaction_adapter(
                    chain_code,
                    &node.rpc_url,
                    &node.http_url,
                )
                .await?;

            let start = std::time::Instant::now();
            let block_height = chain_instance
                .block_num()
                .await
                .ok()
                .map(|h| h as i64)
                .unwrap_or(-1);
            let delay = (start.elapsed().as_millis() / 2) as u64;
            res.push(crate::response_vo::chain::NodeDynData {
                chain_code: chain_code.to_string(),
                node_id,
                name,
                delay,
                block_height,
            })
        }

        Ok(res)
    }
}
