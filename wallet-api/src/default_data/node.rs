use crate::error::service::ServiceError;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultNode {
    // pub(crate) chain_code: String,
    pub(crate) node_name: String,
    pub(crate) rpc_url: String,
    pub(crate) http_url: String,
    pub(crate) network: String,
    pub(crate) active: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Nodes {
    pub(crate) nodes: Vec<DefaultNode>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultNodeList {
    pub(crate) nodes: HashMap<String, Nodes>,
}

static INIT_MAINNET_NODES_INFO: once_cell::sync::Lazy<once_cell::sync::OnceCell<DefaultNodeList>> =
    once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);
static INIT_TESTNET_NODES_INFO: once_cell::sync::Lazy<once_cell::sync::OnceCell<DefaultNodeList>> =
    once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);

fn validate_default_node_list(
    profile: &'static str,
    node_list: &DefaultNodeList,
) -> Result<(), ServiceError> {
    for nodes in node_list.nodes.values() {
        for node in nodes.nodes.iter() {
            if !node.network.eq_ignore_ascii_case(profile) {
                return Err(crate::error::business::BusinessError::Config(
                    crate::error::business::config::ConfigError::DefaultNodeNetworkMismatch {
                        profile: profile.to_string(),
                        node_name: node.node_name.clone(),
                        network: node.network.clone(),
                    },
                )
                .into());
            }
        }
    }

    Ok(())
}

fn init_default_node_list_by_profile(
    profile: &'static str,
) -> Result<&'static DefaultNodeList, ServiceError> {
    let (cell, toml_content) = match profile {
        "mainnet" => {
            (&*INIT_MAINNET_NODES_INFO, include_str!("../../data/config/node.mainnet.toml"))
        }
        "testnet" => {
            (&*INIT_TESTNET_NODES_INFO, include_str!("../../data/config/node.testnet.toml"))
        }
        _ => {
            return Err(ServiceError::Parameter(format!(
                "unsupported default node config profile: {profile}"
            )));
        }
    };

    tracing::info!("loading default node config profile={}", profile);
    cell.get_or_try_init(|| {
        let node_data: DefaultNodeList = wallet_utils::serde_func::toml_from_str(toml_content)?;
        validate_default_node_list(profile, &node_data)?;
        Ok(node_data)
    })
}

pub(crate) fn mainnet_default_nodes_list() -> Result<&'static DefaultNodeList, ServiceError> {
    init_default_node_list_by_profile("mainnet")
}

pub(crate) fn testnet_default_nodes_list() -> Result<&'static DefaultNodeList, ServiceError> {
    init_default_node_list_by_profile("testnet")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_node(node_name: &str, network: &str) -> DefaultNode {
        DefaultNode {
            node_name: node_name.to_string(),
            rpc_url: "https://rpc.test".to_string(),
            http_url: "".to_string(),
            network: network.to_string(),
            active: true,
        }
    }

    fn build_node_list(chain_code: &str, nodes: Vec<DefaultNode>) -> DefaultNodeList {
        DefaultNodeList { nodes: HashMap::from([(chain_code.to_string(), Nodes { nodes })]) }
    }

    #[test]
    fn mainnet_loader_returns_only_mainnet_nodes() {
        let node_list = mainnet_default_nodes_list().expect("load mainnet nodes");
        assert!(!node_list.nodes.is_empty());
        assert!(
            node_list
                .nodes
                .values()
                .all(|nodes| nodes.nodes.iter().all(|node| node.network == "mainnet"))
        );
    }

    #[test]
    fn testnet_loader_returns_only_testnet_nodes() {
        let node_list = testnet_default_nodes_list().expect("load testnet nodes");
        assert!(!node_list.nodes.is_empty());
        assert!(
            node_list
                .nodes
                .values()
                .all(|nodes| nodes.nodes.iter().all(|node| node.network == "testnet"))
        );
        assert!(
            node_list
                .nodes
                .values()
                .any(|nodes| nodes.nodes.iter().any(|node| node.node_name == "Nileex"))
        );
    }

    #[test]
    fn added_evm_chains_have_active_testnet_nodes() {
        let nodes = &testnet_default_nodes_list().expect("load testnet nodes").nodes;

        for chain_code in ["polygon", "avalanche", "arbitrum", "base", "scroll"] {
            let chain_nodes =
                nodes.get(chain_code).unwrap_or_else(|| panic!("missing {chain_code} nodes"));
            assert!(chain_nodes.nodes.iter().any(|node| {
                node.active && node.network == "testnet" && node.rpc_url.starts_with("https://")
            }));
        }
        assert!(!nodes.contains_key("fantom"));
    }

    #[test]
    fn validate_default_node_list_rejects_mixed_networks() {
        let node_list = build_node_list(
            "tron",
            vec![build_node("Nileex", "testnet"), build_node("Bad", "mainnet")],
        );

        let err = validate_default_node_list("testnet", &node_list).unwrap_err();
        match err {
            ServiceError::Business(crate::error::business::BusinessError::Config(
                crate::error::business::config::ConfigError::DefaultNodeNetworkMismatch {
                    profile,
                    node_name,
                    network,
                },
            )) => {
                assert_eq!(profile, "testnet");
                assert_eq!(node_name, "Bad");
                assert_eq!(network, "mainnet");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
