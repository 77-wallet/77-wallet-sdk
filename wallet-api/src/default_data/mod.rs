pub(crate) mod chains;
pub(crate) mod coins;

// static DEFAULT_LIST: once_cell::sync::Lazy<once_cell::sync::OnceCell<DefaultChainList>> =
//     once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);

// #[derive(Debug, serde::Deserialize)]
// pub(crate) struct DefaultChainList {
//     pub(crate) chains: Vec<crate::default_data::chains::DefaultChain>,
//     pub(crate) coins: Vec<crate::default_data::coins::DefaultCoin>,
// }

// pub(crate) fn init_default_list() -> Result<&'static DefaultChainList, crate::ServiceError> {
//     DEFAULT_LIST.get_or_try_init(|| {
//         let rpc_json_data = crate::default_data::chains::init_default_chains_list()?;
//         let coins_json_data = crate::default_data::coins::init_default_coins_list()?;

//         Ok(DefaultChainList {
//             chains: rpc_json_data.chains.to_vec(),
//             coins: coins_json_data.to_vec(),
//         })
//     })
// }
