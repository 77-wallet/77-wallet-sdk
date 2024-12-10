use std::collections::HashSet;

use wallet_database::{
    entities::{
        chain::ChainEntity,
        node::{NodeCreateVo, NodeEntity},
    },
    repositories::{chain::ChainRepoTrait, node::NodeRepoTrait, ResourcesRepo},
};
use wallet_types::valueobject::NodeData;

pub struct NodeDomain;

impl NodeDomain {
    // pub(crate) async fn ping_node(node_id: &str)-> Result<(), crate::ServiceError> {
    //     let backend = crate::manager::Context::get_global_backend_api().unwrap();
    //     surge_ping::ping(host, payload)
    //     ping::ping(addr, timeout, ttl, ident, seq_cnt, payload)
    //     let res = backend.ping_node(node_id).await;
    //     match res {
    //         Ok(_) => Ok(()),
    //         Err(e) => {
    //             tracing::error!("ping_node: {:?}", e);
    //             Err(crate::ServiceError::Business(crate::BusinessError::Node(
    //                 crate::NodeError::PingFailed(node_id.to_string()),
    //             )))
    //         }
    //     }
    // }

    pub(crate) fn gen_node_id(name: &str, chain_code: &str) -> String {
        let params = vec![name, chain_code];
        wallet_utils::snowflake::gen_hash_uid(params)
    }

    pub(crate) async fn process_backend_nodes(
        default_nodes: Vec<NodeData>,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        let backend = crate::manager::Context::get_global_backend_api()?;

        let mut backend_nodes = Vec::new();

        for node in default_nodes.iter() {
            let nodes = match backend.chain_rpc_list(&node.chain_code).await {
                Ok(node) => node,
                Err(e) => {
                    tracing::error!("node_create: {:?}", e);
                    continue;
                }
            };
            for node in nodes.list.iter() {
                let network = if node.test { "testnet" } else { "mainnet" };
                let node = NodeCreateVo::new(&node.name, &node.chain_code, &node.rpc)
                    .with_network(network);
                match NodeRepoTrait::add(&mut repo, node).await {
                    Ok(node) => backend_nodes.push(node),
                    Err(e) => {
                        tracing::error!("node_create: {:?}", e);
                        continue;
                    }
                };
            }
        }
        // tokio::spawn(async move {
        //     if let Err(e) = Self::toggle_chains().await {
        //         tracing::error!("toggle_chains error: {:?}", e);
        //     }
        // });
        let chain_list_req = crate::domain::task_queue::BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::CHAIN_LIST,
            &(),
        )?;
        super::task_queue::Tasks::new()
            .push(super::task_queue::Task::BackendApi(
                super::task_queue::BackendApiTask::BackendApi(chain_list_req),
            ))
            .send()
            .await?;

        Self::process_filtered_nodes(&mut repo, &pool, &backend_nodes, default_nodes).await?;
        Ok(())
    }

    pub(crate) async fn process_filtered_nodes(
        repo: &mut ResourcesRepo,
        pool: &sqlx::SqlitePool,
        backend_nodes: &[NodeEntity],
        default_nodes: Vec<NodeData>,
    ) -> Result<(), crate::ServiceError> {
        let node_list = NodeRepoTrait::list(repo).await?;
        // node_list 排除chains_c中的rpc_url一致的节点
        let rpc_urls: Vec<String> = default_nodes
            .iter()
            .map(|node| node.rpc_url.clone())
            .collect();
        let filtered_nodes: Vec<_> = node_list
            .into_iter()
            .filter(|node| !rpc_urls.contains(&node.rpc_url))
            .collect();
        // 比较filtered_nodes 和 backend_nodes的节点，把backend_nodes中没有，filtered_nodes有的节点，删除

        let backend_node_rpcs: HashSet<String> =
            backend_nodes.iter().map(|n| n.rpc_url.clone()).collect();
        for node in filtered_nodes {
            if !backend_node_rpcs.contains(&node.rpc_url) {
                let chain = ChainRepoTrait::detail_by_id(repo, &node.node_id).await?;
                if let Some(chain) = chain {
                    if let Some(backend_nodes) = backend_nodes
                        .iter()
                        .find(|node| node.chain_code == chain.chain_code)
                    {
                        if let Err(e) = ChainEntity::set_chain_node(
                            pool,
                            &chain.chain_code,
                            &backend_nodes.node_id,
                        )
                        .await
                        {
                            tracing::error!("set_chain_node: {:?}", e);
                        }
                    } else if let Some(node) = default_nodes
                        .iter()
                        .find(|node| node.chain_code == chain.chain_code)
                    {
                        if let Err(e) =
                            ChainEntity::set_chain_node(pool, &chain.chain_code, &node.node_id)
                                .await
                        {
                            tracing::error!("set_chain_node: {:?}", e);
                        }
                    }
                }

                if let Err(e) = NodeRepoTrait::delete(repo, &node.rpc_url, &node.chain_code).await {
                    tracing::error!("Failed to remove filtered node {}: {:?}", node.node_id, e);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    #[tokio::test]
    async fn main() {
        let url = "https://rpc.ankr.com/premium-http/tron/2554129db2045e61c3b8584ad6ee32b7b7808916d160e3e16b51dfee6d17d56c";

        let start = Instant::now();
        match reqwest::get(url).await {
            Ok(response) => {
                let duration = start.elapsed();
                println!(
                    "Ping successful! Status: {}, Time: {:?}",
                    response.status(),
                    duration
                );
            }
            Err(err) => {
                println!("Ping failed: {:?}", err);
            }
        }
    }
}
