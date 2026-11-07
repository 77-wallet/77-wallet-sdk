use crate::error::service::ServiceError;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultChain {
    pub(crate) name: String,
    pub(crate) chain_code: String,
    pub(crate) protocols: Vec<String>,
    pub(crate) main_symbol: String,
    pub(crate) active: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DefaultChainList {
    pub(crate) chains: HashMap<String, DefaultChain>,
}

static INIT_MAINNET_CHAINS_INFO: once_cell::sync::Lazy<
    once_cell::sync::OnceCell<DefaultChainList>,
> = once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);
static INIT_TESTNET_CHAINS_INFO: once_cell::sync::Lazy<
    once_cell::sync::OnceCell<DefaultChainList>,
> = once_cell::sync::Lazy::new(once_cell::sync::OnceCell::new);

pub(crate) fn mainnet_default_chains_list() -> Result<&'static DefaultChainList, ServiceError> {
    INIT_MAINNET_CHAINS_INFO.get_or_try_init(|| {
        let toml_content = include_str!("../../data/config/chain.mainnet.toml");
        let toml_data: DefaultChainList = wallet_utils::serde_func::toml_from_str(toml_content)?;

        Ok(toml_data)
    })
}

pub(crate) fn testnet_default_chains_list() -> Result<&'static DefaultChainList, ServiceError> {
    INIT_TESTNET_CHAINS_INFO.get_or_try_init(|| {
        let toml_content = include_str!("../../data/config/chain.testnet.toml");
        let toml_data: DefaultChainList = wallet_utils::serde_func::toml_from_str(toml_content)?;

        Ok(toml_data)
    })
}

pub(crate) fn default_chain_lists_for_feature(
    profile: &'static str,
) -> Result<Vec<&'static DefaultChainList>, ServiceError> {
    match profile {
        "prod" => Ok(vec![mainnet_default_chains_list()?]),
        "dev" | "test" => Ok(vec![mainnet_default_chains_list()?, testnet_default_chains_list()?]),
        other => Err(ServiceError::Parameter(format!(
            "unsupported default chain config profile: {other}"
        ))),
    }
}

pub(crate) fn enabled_chain_codes_for_feature(
    profile: &'static str,
    backend_enabled_chain_codes: &[String],
    backend_returned_chain_codes: &[String],
) -> Result<Vec<String>, ServiceError> {
    match profile {
        "test" | "prod" => Ok(backend_enabled_chain_codes.to_vec()),
        "dev" => {
            let backend_returned: HashSet<&str> =
                backend_returned_chain_codes.iter().map(String::as_str).collect();
            let mut enabled = backend_enabled_chain_codes.to_vec();
            let mut seen: HashSet<String> = enabled.iter().cloned().collect();

            for list in default_chain_lists_for_feature(profile)? {
                for (chain_code, chain) in &list.chains {
                    if chain.active
                        && !backend_returned.contains(chain_code.as_str())
                        && seen.insert(chain_code.clone())
                    {
                        enabled.push(chain_code.clone());
                    }
                }
            }

            Ok(enabled)
        }
        other => Err(ServiceError::Parameter(format!(
            "unsupported default chain config profile: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_chain_display_names_match_product_catalog() {
        let mainnet = mainnet_default_chains_list().unwrap();
        let testnet = testnet_default_chains_list().unwrap();
        for (code, name, symbol) in [
            ("btc", "Bitcoin", "BTC"),
            ("eth", "Ethereum", "ETH"),
            ("tron", "TRON", "TRX"),
            ("bnb", "BNB Chain", "BNB"),
            ("sol", "Solana", "SOL"),
            ("ton", "TON", "GRAM"),
            ("sui", "Sui", "SUI"),
            ("doge", "Dogecoin", "DOGE"),
            ("ltc", "Litecoin", "LTC"),
            ("arbitrum", "Arbitrum One", "ETH"),
            ("polygon", "Polygon", "POL"),
            ("base", "Base", "ETH"),
            ("avalanche", "Avalanche-C", "AVAX"),
            ("scroll", "Scroll", "ETH"),
        ] {
            let chain = mainnet.chains.get(code).or_else(|| testnet.chains.get(code)).unwrap();
            assert_eq!((&*chain.name, &*chain.main_symbol), (name, symbol), "{code}");
        }
    }

    #[test]
    fn added_evm_chains_are_testnet_only() {
        let mainnet = &mainnet_default_chains_list().expect("load mainnet chains").chains;
        let testnet = &testnet_default_chains_list().expect("load testnet chains").chains;
        let expected = [
            ("polygon", "POL"),
            ("avalanche", "AVAX"),
            ("arbitrum", "ETH"),
            ("base", "ETH"),
            ("scroll", "ETH"),
        ];

        for (chain_code, main_symbol) in expected {
            assert!(!mainnet.contains_key(chain_code), "{chain_code} leaked into mainnet");
            let chain = testnet.get(chain_code).unwrap_or_else(|| panic!("missing {chain_code}"));
            assert_eq!(chain.chain_code, chain_code);
            assert_eq!(chain.main_symbol, main_symbol);
            assert_eq!(chain.protocols, ["ERC20"]);
            assert!(chain.active);
        }
        assert!(!testnet.contains_key("fantom"));
    }

    #[test]
    fn chain_profiles_select_expected_config_sets() {
        let prod = default_chain_lists_for_feature("prod").expect("prod chain configs");
        let dev = default_chain_lists_for_feature("dev").expect("dev chain configs");
        let test = default_chain_lists_for_feature("test").expect("test chain configs");

        assert_eq!(prod.len(), 1);
        assert_eq!(dev.len(), 2);
        assert_eq!(test.len(), 2);
        assert!(prod[0].chains.contains_key("eth"));
        assert!(dev.iter().any(|list| list.chains.contains_key("polygon")));
        assert!(test.iter().any(|list| list.chains.contains_key("scroll")));
    }

    #[test]
    fn dev_does_not_restore_backend_disabled_local_chain() {
        let enabled = vec!["tron".to_string()];
        let returned = vec!["tron".to_string(), "polygon".to_string()];

        let selected =
            enabled_chain_codes_for_feature("dev", &enabled, &returned).expect("dev chain codes");

        assert!(selected.iter().any(|chain| chain == "tron"));
        assert!(!selected.iter().any(|chain| chain == "polygon"));
    }

    #[test]
    fn dev_restores_local_chain_missing_from_backend() {
        let enabled = vec!["tron".to_string()];
        let returned = vec!["tron".to_string()];

        let selected =
            enabled_chain_codes_for_feature("dev", &enabled, &returned).expect("dev chain codes");

        assert!(selected.iter().any(|chain| chain == "tron"));
        assert!(selected.iter().any(|chain| chain == "polygon"));
        assert!(!selected.iter().any(|chain| chain == "fantom"));
    }

    #[test]
    fn test_and_prod_keep_backend_chain_selection() {
        let backend = vec!["tron".to_string()];

        assert_eq!(
            enabled_chain_codes_for_feature("test", &backend, &backend).expect("test chain codes"),
            backend
        );
        assert_eq!(
            enabled_chain_codes_for_feature("prod", &backend, &backend).expect("prod chain codes"),
            backend
        );
    }
}

/// Five-chain development rollout shared by ordinary and API wallets.
pub(crate) const DEV_EVM_CHAIN_CODES: [&str; 5] =
    ["polygon", "avalanche", "arbitrum", "base", "scroll"];

pub(crate) fn is_dev_evm_chain(code: &str) -> bool {
    DEV_EVM_CHAIN_CODES.contains(&code)
}
