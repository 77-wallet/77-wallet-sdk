// struct InitChainsInfo {
//     rpc_url: String,
//     ws_url: String,
// }

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultChain {
    pub(crate) name: String,
    pub(crate) chain_code: String,
    pub(crate) node_name: String,
    pub(crate) rpc_url: String,
    pub(crate) http_url: String,
    pub(crate) protocols: Vec<String>,
    pub(crate) main_symbol: String,
    pub(crate) network: String,
    pub(crate) active: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultChainList {
    pub(crate) chains: Vec<DefaultChain>,
}

static INIT_CHAINS_INFO: once_cell::sync::Lazy<once_cell::sync::OnceCell<DefaultChainList>> =
    once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);

pub(crate) fn init_default_chains_list() -> Result<&'static DefaultChainList, crate::ServiceError> {
    INIT_CHAINS_INFO.get_or_try_init(|| {
        // let mut res = std::collections::HashMap::new();
        let rpc_json = include_str!("../../data/default_chain_list/rpc.json");
        let rpc_json_data: DefaultChainList = wallet_utils::serde_func::serde_from_str(rpc_json)?;

        Ok(rpc_json_data)
    })
}
