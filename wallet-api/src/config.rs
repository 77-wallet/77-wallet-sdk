use serde::Deserialize;

// 运行时稳定性相关的默认阈值（本轮止血参数）集中放在子模块中维护。
pub mod runtime_defaults;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChainNetwork {
    Mainnet,
    Testnet,
}

impl ChainNetwork {
    pub fn as_str(self) -> &'static str {
        match self {
            ChainNetwork::Mainnet => "mainnet",
            ChainNetwork::Testnet => "testnet",
        }
    }

    pub fn to_network_kind(self) -> wallet_types::chain::network::NetworkKind {
        match self {
            ChainNetwork::Mainnet => wallet_types::chain::network::NetworkKind::Mainnet,
            ChainNetwork::Testnet => wallet_types::chain::network::NetworkKind::Testnet,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default)]
    pub log_upload: Option<wallet_transport_backend::log_upload::LogUploadConfig>,
    pub backend_api: BackendApiConfig,
    pub aggregate_api: AggregateApi,
    pub crypto: CryptoConfig,
    pub app_code: String,
    #[serde(default)]
    pub unlock_session: UnlockSessionConfig,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(default)]
pub struct UnlockSessionConfig {
    /// 解锁会话的轮换周期，单位秒。
    pub rotation_interval_secs: u64,
    /// 解锁会话轮换检查的轮询周期，单位秒。
    pub rotation_check_interval_secs: u64,
}

impl Default for UnlockSessionConfig {
    fn default() -> Self {
        let defaults = runtime_defaults::unlock_session();
        Self {
            rotation_interval_secs: defaults.rotation_interval_secs,
            rotation_check_interval_secs: defaults.rotation_check_interval_secs,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct CryptoConfig {
    pub aes_key: String,
    pub aes_iv: String,
}

#[derive(Deserialize, Debug)]
pub struct AggregateApi {
    pub dev_url: String,
    pub test_url: String,
    pub prod_url: String,
}

#[derive(Deserialize, Debug)]
pub struct BackendApiConfig {
    pub dev_url: String,
    pub test_url: String,
    pub prod_url: String,
}

impl Config {
    pub fn new(config_content: &str) -> Result<Self, crate::error::service::ServiceError> {
        let config: Config = wallet_utils::serde_func::serde_yaml_from_str(config_content)?;
        Ok(config)
    }

    pub fn unlock_session_rotation_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.unlock_session.rotation_interval_secs)
    }

    pub fn unlock_session_rotation_check_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.unlock_session.rotation_check_interval_secs)
    }

    pub fn resolved_chain_network(&self) -> ChainNetwork {
        Self::feature_chain_network()
    }

    /// NOTE:
    /// feature network is a compile-profile compatibility value only.
    /// Runtime chain network must be resolved from the bound node.network per chain.
    pub fn feature_chain_network() -> ChainNetwork {
        #[cfg(feature = "prod")]
        {
            return ChainNetwork::Mainnet;
        }
        #[cfg(any(feature = "test", feature = "dev"))]
        {
            return ChainNetwork::Testnet;
        }
        ChainNetwork::Mainnet
    }

    pub fn active_feature_profile() -> &'static str {
        #[cfg(feature = "prod")]
        {
            return "prod";
        }
        #[cfg(feature = "dev")]
        {
            return "dev";
        }
        #[cfg(feature = "test")]
        {
            return "test";
        }
        "unknown"
    }

    pub fn visible_node_networks() -> &'static [&'static str] {
        #[cfg(feature = "prod")]
        {
            return &["mainnet"];
        }
        #[cfg(any(feature = "dev", feature = "test"))]
        {
            return &["mainnet", "testnet"];
        }
        &["mainnet"]
    }

    pub fn node_network_is_visible(network: &str) -> bool {
        let network = if network.is_empty() { "mainnet" } else { network };
        Self::visible_node_networks().iter().any(|allowed| allowed.eq_ignore_ascii_case(network))
    }

    pub fn node_visibility_status(network: &str) -> u8 {
        if Self::node_network_is_visible(network) { 1 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_legacy_scope_cannot_select_chain_address_or_nonce() {
        let base = "app_code: test\ncrypto: { aes_key: synthetic-key, aes_iv: synthetic-iv }\nbackend_api: { dev_url: x, test_url: x, prod_url: x }\naggregate_api: { dev_url: x, test_url: x, prod_url: x }\n";
        // Removed configuration is inert, just like other unknown historical fields.
        // Recovery is selected from durable order facts, in every environment.
        for chain in ["eth", "scroll", "base"] {
            let legacy = format!(
                "{base}collect_replacement_recovery:\n  chain_code: {chain}\n  from_addr: ignored\n  nonce: 42\n"
            );
            assert!(Config::new(&legacy).is_ok());
        }
    }

    #[test]
    fn log_upload_config_is_optional_and_does_not_require_oss() {
        let yaml = r#"
app_code: test
crypto: { aes_key: synthetic-key, aes_iv: synthetic-iv }
backend_api: { dev_url: x, test_url: x, prod_url: x }
aggregate_api: { dev_url: x, test_url: x, prod_url: x }
"#;
        assert!(Config::new(yaml).unwrap().log_upload.is_none());
        let updated = format!(
            "{yaml}\nlog_upload: {{ url: 'https://logs.example.invalid/log/uploadFile' }}\n"
        );
        let cfg = Config::new(&updated).unwrap();
        let upload = cfg.log_upload.unwrap();
        assert_eq!(cfg.crypto.aes_key, "synthetic-key");
        assert!(wallet_transport_backend::log_upload::LogUploadClient::new(upload).is_ok());
    }

    #[test]
    fn log_upload_config_requires_only_the_backend_url() {
        let yaml = r#"
app_code: test
crypto: { aes_key: synthetic-key, aes_iv: synthetic-iv }
backend_api: { dev_url: x, test_url: x, prod_url: x }
aggregate_api: { dev_url: x, test_url: x, prod_url: x }
log_upload: { url: 'https://logs.example.invalid/upload' }
"#;
        assert!(Config::new(yaml).is_ok());
    }

    #[test]
    fn unlock_session_config_uses_defaults_when_missing() {
        let config = Config::new(
            r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
"#,
        )
        .expect("parse config");

        assert_eq!(
            config.unlock_session_rotation_interval(),
            std::time::Duration::from_secs(
                runtime_defaults::unlock_session().rotation_interval_secs
            )
        );
        assert_eq!(
            config.unlock_session_rotation_check_interval(),
            std::time::Duration::from_secs(
                runtime_defaults::unlock_session().rotation_check_interval_secs
            )
        );
    }

    #[test]
    fn unlock_session_config_uses_yaml_overrides() {
        let config = Config::new(
            r#"
app_code: "test"
crypto:
  aes_key: "1234567890abcdef"
  aes_iv: "abcdef1234567890"
backend_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
aggregate_api:
  dev_url: "http://127.0.0.1:9"
  test_url: "http://127.0.0.1:9"
  prod_url: "http://127.0.0.1:9"
oss:
  access_key_id: "id"
  access_key_secret: "secret"
  bucket_name: "bucket"
  endpoint: "oss-endpoint"
unlock_session:
  rotation_interval_secs: 7
  rotation_check_interval_secs: 2
"#,
        )
        .expect("parse config");

        assert_eq!(config.unlock_session_rotation_interval(), std::time::Duration::from_secs(7));
        assert_eq!(
            config.unlock_session_rotation_check_interval(),
            std::time::Duration::from_secs(2)
        );
    }

    #[test]
    fn visible_node_networks_match_feature_profile() {
        let networks = Config::visible_node_networks();
        if cfg!(feature = "prod") {
            assert_eq!(networks, &["mainnet"]);
        } else if cfg!(any(feature = "dev", feature = "test")) {
            assert_eq!(networks, &["mainnet", "testnet"]);
        } else {
            assert_eq!(networks, &["mainnet"]);
        }
    }

    #[test]
    fn node_visibility_status_matches_allowed_networks() {
        assert_eq!(
            Config::node_visibility_status("mainnet"),
            if Config::node_network_is_visible("mainnet") { 1 } else { 0 }
        );
        assert_eq!(
            Config::node_visibility_status("testnet"),
            if Config::node_network_is_visible("testnet") { 1 } else { 0 }
        );
    }
}
