use crate::domain::node::NodeDomain;

/*
{
    "clientId": "wenjing",
    "sn": "wenjing",
    "deviceType": "ANDROID",
    "bizType": "INIT",
    "body": [
        {
            "address": "TGyw6wH5UT5GVY5v6MTWedabScAwF4gffQ",
            "balance": 4000002,
            "chainCode": "tron",
            "code": "sadsadsad",
              "tokenAddress": "",
              "decimals": 6
        }
    ]
}
*/

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
    pub chain_id: i32,
    pub name: String,
    pub url: String,
}

impl RpcChange {
    pub(crate) async fn exec(self, _msg_id: &str) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        let list = crate::default_data::chains::init_default_chains_list()?;

        let mut default_nodes = Vec::new();
        for default_chain in list.chains.iter() {
            let node_id = NodeDomain::gen_node_id(&default_chain.name, &default_chain.chain_code);
            default_nodes.push(wallet_types::valueobject::NodeData {
                node_id,
                rpc_url: default_chain.rpc_url.to_owned(),
                chain_code: default_chain.chain_code.to_owned(),
            });
        }
        let RpcChange(body) = &self;
        let mut backend_nodes = Vec::new();
        for rpc_change_body in body {
            let RpcChangeBody {
                chain_code,
                rpc_address_info_body_list,
            } = rpc_change_body;

            for node in rpc_address_info_body_list.iter() {
                let network = "mainnet";
                let node = wallet_database::entities::node::NodeCreateVo::new(
                    &node.name, chain_code, &node.url,
                )
                .with_network(network);
                match wallet_database::repositories::node::NodeRepoTrait::add(&mut repo, node).await
                {
                    Ok(node) => backend_nodes.push(node),
                    Err(e) => {
                        tracing::error!("node_create: {:?}", e);
                        continue;
                    }
                };
            }
        }

        NodeDomain::process_filtered_nodes(&mut repo, &pool, &backend_nodes, default_nodes).await?;

        // let data = crate::notify::NotifyEvent::Init(self);
        // crate::notify::FrontendNotifyEvent::new(data).send().await?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use crate::mqtt::payload::incoming::init::Init;

    #[test]
    fn test_() {
        let raw = r#"
        {
            "bizType": "INIT",
            "body": [
                {
                    "address": "TCVt2AYPjUZdSvLgUy8x2xhT7uj1FrQRZs",
                    "balance": 2000000000,
                    "chainCode": "tron",
                    "code": "trx"
                }
            ],
            "clientId": "wenjing",
            "deviceType": "ANDROID",
            "sn": "wenjing"
        }
        "#;
        let res = serde_json::from_str::<Init>(&raw);
        println!("res: {res:?}");
    }

    #[test]
    fn test_decimal() {
        let balance = wallet_types::Decimal::from_str("1996.733").unwrap();
        let balance = wallet_utils::unit::convert_to_u256(&balance.to_string(), 6).unwrap();
        println!("balance: {balance}");
        println!(
            "balance: {}",
            wallet_utils::unit::format_to_string(balance, 6).unwrap()
        );
        // let balance = wallet_utils::unit::u256_from_str(&balance.to_string()).unwrap();
    }
}
