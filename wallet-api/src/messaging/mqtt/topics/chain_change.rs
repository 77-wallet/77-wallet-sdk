use crate::{
    context::Context,
    domain::app::config::ConfigDomain,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent, other::ChainChangeFrontend},
};
use wallet_transport_backend::response_vo::chain::ChainUrlInfo;

// biz_type = CHAIN_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChainChange(Vec<ChainUrlInfo>);

// biz_type = CHAIN_CHANGE
impl ChainChange {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let ChainChange(body) = &self;
        ConfigDomain::set_block_browser_url(ctx, body).await?;

        let has_new_chain =
            crate::domain::chain::ChainDomain::upsert_multi_chain_than_toggle_with_ctx(
                ctx,
                body.into(),
            )
            .await?;
        let data = ChainChangeFrontend { has_new_chain, chains: self.0.to_vec() };
        let data = NotifyEvent::ChainChange(data);
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        Ok(())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::messaging::mqtt::Message;
    use std::str::FromStr;

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
