use crate::{
    context::Context, domain::node::NodeDomain, error::service::ServiceError,
    infrastructure::chain_node::chain_node_ensurer::ChainNodeEnsurer,
};

// biz_type = RPC_ADDRESS_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RpcChange(Vec<RpcChangeBody>);

// biz_type = RPC_ADDRESS_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RpcChangeBody {
    // 链码
    pub chain_code: String,
    pub rpc_address_info_body_list: Vec<RpcAddressInfoBody>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RpcAddressInfoBody {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub chain_id: Option<i32>,
    pub name: String,
    pub url: String,
}

fn validate_rpc_url(url: &str) -> Result<(), ServiceError> {
    #[cfg(feature = "prod")]
    {
        let parsed = url::Url::parse(url)
            .map_err(|_| ServiceError::Parameter("invalid RPC URL".to_string()))?;
        if parsed.scheme() != "https" {
            return Err(ServiceError::Parameter("production RPC URL must use HTTPS".to_string()));
        }
    }

    #[cfg(not(feature = "prod"))]
    let _ = url;

    Ok(())
}

impl RpcChange {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.core_pool()?;
        // let list = crate::default_data::node::get_default_node_list()?;

        let RpcChange(body) = &self;
        let mut list = Vec::new();
        for rpc_change_body in body {
            let RpcChangeBody { chain_code, rpc_address_info_body_list } = rpc_change_body;

            for node in rpc_address_info_body_list.iter() {
                let Some(id) = &node.id else {
                    continue;
                };
                validate_rpc_url(&node.url)?;
                list.push(wallet_transport_backend::response_vo::chain::ChainInfo {
                    id: id.clone(),
                    name: node.name.clone(),
                    chain_code: chain_code.clone(),
                    rpc: node.url.clone(),
                    http_url: None,
                    test: false, // 或从 biz 判断
                });
            }
        }
        if list.is_empty() {
            return Ok(());
        }
        let chain_infos = wallet_transport_backend::response_vo::chain::ChainInfos { list };

        // 只做一件事：upsert backend nodes
        NodeDomain::upsert_chain_rpc(&pool, chain_infos).await?;

        // 可选：触发 ensurer，保证链可用
        let api_pool = ctx.api_wallet_pool()?;
        let ensurer = ChainNodeEnsurer::new(pool, api_pool);
        ensurer.ensure_all().await?;
        crate::domain::coin::CoinDomain::sync_default_coins_by_bound_nodes_with_ctx(ctx).await?;

        // let data = crate::notify::NotifyEvent::Init(self);
        // crate::notify::FrontendNotifyEvent::new(data).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::validate_rpc_url;

    #[cfg(feature = "prod")]
    #[test]
    fn production_accepts_https_rpc_url() {
        assert!(validate_rpc_url("https://rpc.example.com").is_ok());
    }

    #[cfg(feature = "prod")]
    #[test]
    fn production_rejects_insecure_or_invalid_rpc_url() {
        assert!(validate_rpc_url("http://rpc.example.com").is_err());
        assert!(validate_rpc_url("not a url").is_err());
    }

    #[cfg(not(feature = "prod"))]
    #[test]
    fn development_keeps_local_http_rpc_support() {
        assert!(validate_rpc_url("http://127.0.0.1:8545").is_ok());
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use std::str::FromStr;

    use crate::messaging::mqtt::Message;

    #[test]
    fn test_() {
        let raw = r#"
        {
            "appId": "13065ffa4e8f6958bd6",
            "bizType": "RPC_ADDRESS_CHANGE",
            "body": [{
                "chainCode": "sol",
                "rpcAddressInfoBodyList": [{
                    "chainId": 1,
                    "id": "66c597d2c4aa1c8385046116",
                    "name": "sol",
                    "url": "http://rpc.88ai.fun/sol"
                }]
            }, {
                "chainCode": "eth",
                "rpcAddressInfoBodyList": [{
                    "chainId": 1,
                    "id": "675c02a8f4d96273e8cd9653",
                    "name": "eth8",
                    "url": "http://rpc.88ai.fun/eth"
                }, {
                    "id": "675c02a8f4d96273e8cd9654",
                    "name": "eth022",
                    "url": "http://rpc.88ai.fun/eth"
                }]
            }, {
                "chainCode": "tron",
                "rpcAddressInfoBodyList": [{
                    "id": "676162e51350347bf4774d1b",
                    "name": "tron2",
                    "url": "http://www.222.com"
                }, {
                    "id": "676162e51350347bf4774d1a",
                    "name": "tron1",
                    "url": "http://www.1111.com"
                }, {
                    "id": "676162fe1350347bf4774d1c",
                    "name": "tron3",
                    "url": "http://www.333.com"
                }]
            }],
            "clientId": "b205d2716d87d73af508ff2375149487",
            "deviceType": "ANDROID",
            "sn": "ebe42b137abb313f0d0012f588080395c3742e7eac77e60f43fac0afb363e67c",
            "msgId": "6761634c9020540c37dc343f"
        }
        "#;
        let res = serde_json::from_str::<Message>(&raw);
        println!("res: {res:?}");
    }

    #[test]
    fn test_decimal() {
        let balance = wallet_types::Decimal::from_str("1996.733").unwrap();
        let balance = wallet_utils::unit::convert_to_u256(&balance.to_string(), 6).unwrap();
        println!("balance: {balance}");
        println!("balance: {}", wallet_utils::unit::format_to_string(balance, 6).unwrap());
        // let balance = wallet_utils::unit::u256_from_str(&balance.to_string()).unwrap();
    }
}
