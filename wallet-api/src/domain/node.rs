use std::{collections::HashMap, time::Duration};

use crate::{
    context::Context,
    domain,
    infrastructure::{
        runtime::time::with_timeout,
        task_queue::{
            backend::{BackendApiTask, BackendApiTaskData},
            task::Tasks,
        },
    },
};
use wallet_database::{
    CoreDbPool,
    entities::node::{NodeCreateVo, NodeEntity},
    repositories::{chain::ChainRepo, node::NodeRepo},
};
use wallet_transport_backend::{request::ChainRpcListReq, response_vo::chain::ChainInfos};

const NODE_DYNAMIC_DATA_TIMEOUT: Duration = Duration::from_secs(2);
const NODE_DYNAMIC_DATA_UNAVAILABLE_DELAY: u64 = 0;
const NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT: i64 = -1;

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
        let env_network = Self::get_env_network_name();
        let params = vec![name, chain_code, &env_network];
        wallet_utils::snowflake::gen_hash_uid(params)
    }

    pub fn get_env_network_name() -> String {
        let mut env = "mainnet".to_owned();
        #[cfg(any(feature = "test", feature = "dev"))]
        {
            env = "testnet".to_owned();
        }
        env
    }

    pub(crate) async fn probe_dynamic_data_with_timeout(
        ctx: &'static Context,
        chain_code: &str,
        node: &NodeEntity,
    ) -> Result<
        crate::response_vo::standard_wallet::chain::NodeDynData,
        crate::error::service::ServiceError,
    > {
        let chain_instance =
            domain::chain::adapter::ChainAdapterFactory::get_node_transaction_adapter_with_ctx(
                &ctx,
                chain_code,
                &node.rpc_url,
                &node.network,
            )
            .await?;

        let start = std::time::Instant::now();
        let block_height = match with_timeout(NODE_DYNAMIC_DATA_TIMEOUT, chain_instance.block_num())
            .await
        {
            Ok(Ok(height)) => height as i64,
            Ok(Err(err)) => {
                tracing::warn!(node_id = %node.node_id, error = %err, "node dynamic probe failed");
                NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT
            }
            Err(_) => {
                tracing::warn!(node_id = %node.node_id, "node dynamic probe timed out");
                NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT
            }
        };
        let delay = if block_height == NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT {
            NODE_DYNAMIC_DATA_UNAVAILABLE_DELAY
        } else {
            (start.elapsed().as_millis() / 2) as u64
        };

        Ok(Self::build_dynamic_data(chain_code, node, delay, block_height))
    }

    pub(crate) async fn probe_dynamic_data(
        ctx: &'static Context,
        chain_code: &str,
        node: &NodeEntity,
    ) -> Result<
        crate::response_vo::standard_wallet::chain::NodeDynData,
        crate::error::service::ServiceError,
    > {
        let chain_instance =
            domain::chain::adapter::ChainAdapterFactory::get_node_transaction_adapter_with_ctx(
                &ctx,
                chain_code,
                &node.rpc_url,
                &node.network,
            )
            .await?;

        let start = std::time::Instant::now();
        let block_height = chain_instance.block_num().await.ok().map(|h| h as i64).unwrap_or(-1);
        let delay = (start.elapsed().as_millis() / 2) as u64;

        Ok(Self::build_dynamic_data(chain_code, node, delay, block_height))
    }

    pub(crate) fn build_dynamic_data(
        chain_code: &str,
        node: &NodeEntity,
        delay: u64,
        block_height: i64,
    ) -> crate::response_vo::standard_wallet::chain::NodeDynData {
        crate::response_vo::standard_wallet::chain::NodeDynData {
            chain_code: chain_code.to_string(),
            node_id: node.node_id.clone(),
            name: node.name.clone(),
            delay,
            block_height,
        }
    }

    pub(crate) async fn upsert_chain_rpc(
        pool: &CoreDbPool,
        chain_infos: ChainInfos,
    ) -> Result<(), crate::error::service::ServiceError> {
        for chain_info in chain_infos.list.iter() {
            let network = if chain_info.test { "testnet" } else { "mainnet" };
            let status = crate::config::Config::node_visibility_status(network);
            let node = NodeCreateVo::new(
                &chain_info.id,
                &chain_info.name,
                &chain_info.chain_code,
                &chain_info.rpc,
                chain_info.http_url.clone(),
            )
            .with_network(network);
            let node = node.with_status(status);
            tracing::debug!("创建节点: {:?}", node);
            match NodeRepo::upsert(pool, node).await {
                Ok(node) => tracing::debug!("创建节点成功: {:?}", node),
                Err(e) => {
                    tracing::error!("node_create error: {:?}", e);
                    continue;
                }
            };
        }

        let mut by_chain: HashMap<String, Vec<String>> = HashMap::new();
        for n in chain_infos.list.iter() {
            by_chain.entry(n.chain_code.clone()).or_default().push(n.id.clone());
        }

        for (chain, ids) in by_chain {
            let affected = NodeRepo::disable_backend_not_in(pool, &chain, &ids).await?;
            tracing::debug!("disabled {} backend nodes for chain {}", affected, chain);
        }
        Ok(())
    }

    fn default_node_lists_for_feature(
        profile: &'static str,
    ) -> Result<
        Vec<&'static crate::default_data::node::DefaultNodeList>,
        crate::error::service::ServiceError,
    > {
        match profile {
            "prod" => Ok(vec![crate::default_data::node::mainnet_default_nodes_list()?]),
            "dev" | "test" => Ok(vec![
                crate::default_data::node::mainnet_default_nodes_list()?,
                crate::default_data::node::testnet_default_nodes_list()?,
            ]),
            other => Err(crate::error::service::ServiceError::Parameter(format!(
                "unsupported default node config profile: {other}"
            ))),
        }
    }

    // async fn load_backend_node() -> Result<
    //     wallet_transport_backend::response_vo::chain::ChainList,
    //     crate::error::service::ServiceError,
    // > {
    //     let app_version = ConfigDomain::get_app_version().await?;
    //     let chain_list_req = BackendApiTaskData::new(
    //         wallet_transport_backend::consts::endpoint::CHAIN_LIST,
    //         &wallet_transport_backend::request::ChainListReq::new(app_version.app_version),
    //     )?;
    //
    //
    //     let backend_chains = backend_api
    //         .post_req_str::<wallet_transport_backend::response_vo::chain::ChainList>(
    //             wallet_transport_backend::consts::endpoint::CHAIN_LIST,
    //             &chain_list_req.body.clone(),
    //         )
    //         .await?;
    //     Ok(backend_chains)
    // }

    pub(crate) async fn init_sync_nodes_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        let local_chains = ChainRepo::get_chain_list(&pool).await?;
        let chain_codes: Vec<_> =
            local_chains.iter().map(|chain| chain.chain_code.clone()).collect();

        if !chain_codes.is_empty() {
            // 3. 派发 CHAIN_RPC_LIST 任务
            let req = BackendApiTaskData::new(
                wallet_transport_backend::consts::endpoint::CHAIN_RPC_LIST,
                &ChainRpcListReq::new(chain_codes),
            )?;
            Tasks::new().push(BackendApiTask::BackendApi(req)).send_with_ctx(ctx).await?;
        }

        Ok(())
    }

    pub async fn init_load_default_nodes_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let profile = crate::config::Config::active_feature_profile();
        let node_lists = Self::default_node_lists_for_feature(profile)?;

        let pool = ctx.core_pool()?;
        tracing::info!(
            "initializing default nodes profile={}, visible_networks={:?}",
            profile,
            crate::config::Config::visible_node_networks()
        );

        for node_list in node_lists {
            for (chain_code, nodes) in node_list.nodes.iter() {
                for default_node in nodes.nodes.iter() {
                    let dev_evm = crate::default_data::chain::is_dev_evm_chain(chain_code);
                    // Fresh dev installs use shared mainnet defaults for the rollout.
                    // Existing nodes/bindings remain owned by their original source.
                    if dev_evm
                        && default_node.network
                            != if profile == "dev" { "mainnet" } else { "testnet" }
                    {
                        continue;
                    }
                    let status = if default_node.active {
                        crate::config::Config::node_visibility_status(&default_node.network)
                    } else {
                        0
                    };

                    let id = NodeDomain::gen_node_id(&default_node.node_name, chain_code);
                    let node = NodeCreateVo::new(
                        &id,
                        &default_node.node_name,
                        chain_code,
                        &default_node.rpc_url,
                        Some(default_node.http_url.clone()),
                    )
                    .with_http_url(&default_node.http_url)
                    .with_network(&default_node.network)
                    .with_status(status)
                    .with_is_local(1);
                    if dev_evm {
                        NodeRepo::insert_missing(&pool, node).await?;
                    } else {
                        let r = NodeRepo::upsert(&pool, node).await;
                        tracing::debug!("Created node {}: {:?}", id, r);
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT, NODE_DYNAMIC_DATA_UNAVAILABLE_DELAY, NodeDomain,
    };
    use wallet_database::entities::node::NodeEntity;

    fn test_node() -> NodeEntity {
        NodeEntity {
            node_id: "node-a".to_string(),
            name: "Node A".to_string(),
            chain_code: "tron".to_string(),
            rpc_url: "https://rpc.test".to_string(),
            ws_url: "https://rpc.test".to_string(),
            http_url: String::new(),
            network: "mainnet".to_string(),
            status: 1,
            is_local: 0,
            created_at: sqlx::types::chrono::Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn node_dynamic_data_preserves_node_identity() {
        let data = NodeDomain::build_dynamic_data("tron", &test_node(), 15, 128);

        assert_eq!(data.chain_code, "tron");
        assert_eq!(data.node_id, "node-a");
        assert_eq!(data.name, "Node A");
        assert_eq!(data.delay, 15);
        assert_eq!(data.block_height, 128);
    }

    #[test]
    fn node_dynamic_data_uses_existing_unavailable_sentinel() {
        let data = NodeDomain::build_dynamic_data(
            "tron",
            &test_node(),
            NODE_DYNAMIC_DATA_UNAVAILABLE_DELAY,
            NODE_DYNAMIC_DATA_UNAVAILABLE_BLOCK_HEIGHT,
        );

        assert_eq!(data.delay, 0);
        assert_eq!(data.block_height, -1);
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use std::time::Instant;

    #[tokio::test]
    async fn main() {
        let url = "https://rpc.ankr.com/premium-http/tron/2554129db2045e61c3b8584ad6ee32b7b7808916d160e3e16b51dfee6d17d56c";

        let start = Instant::now();
        match reqwest::get(url).await {
            Ok(response) => {
                let duration = start.elapsed();
                println!("Ping successful! Status: {}, Time: {:?}", response.status(), duration);
            }
            Err(err) => {
                println!("Ping failed: {:?}", err);
            }
        }
    }
}
