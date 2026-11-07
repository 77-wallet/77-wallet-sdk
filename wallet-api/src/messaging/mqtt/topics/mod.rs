mod bulletin_info;
pub use bulletin_info::*;

mod chain_change;
pub use chain_change::*;

mod order;
pub use order::*;

mod switch;
pub(crate) use switch::*;
pub(crate) mod api_wallet;
// #[cfg(feature = "token")]
pub(crate) mod token_price;
// #[cfg(feature = "token")]
pub(crate) use token_price::*;
mod rpc;
pub use rpc::*;

#[derive(Debug)]
struct TopicPattern {
    prefix: &'static [&'static str],
    topic: Topic,
    extract_client_id: bool,
}

macro_rules! define_topic_patterns {
    (
        $( $topic:ident => [$($segment:expr),*] $(($with_client_id:ident))? ),* $(,)?
    ) => {
        const TOPIC_PATTERNS: &[TopicPattern] = &[
            $(
                TopicPattern {
                    prefix: &[$($segment),*],
                    topic: Topic::$topic,
                    extract_client_id: define_topic_patterns!(@flag $($with_client_id)?),
                },
            )*
        ];

        impl From<Topic> for String {
            fn from(value: Topic) -> Self {
                match value {
                    $( Topic::$topic => [$($segment),*].join("/"), )*
                }
            }
        }
    };

    (@flag) => { false };
    (@flag with_client_id) => { true };
}

define_topic_patterns! {
    Common => ["wallet", "common"] (with_client_id),
    Order => ["wallet", "order"] (with_client_id),
    BulletinInfo => ["wallet", "bulletin"] (with_client_id),
    Switch => ["wallet", "switch"],
    Token => ["wallet", "token"],
    RpcChange => ["wallet", "rpc", "change"],
    ChainChange => ["wallet", "chain", "change"],


    MerchantTrans => ["aw",  "merchant", "trans"] (with_client_id),
    // WalletMerchantTrans => ["aw", "merchant", "trans"] (with_client_id),
    WalletMerchantCmd => ["aw", "merchant", "cmd"] (with_client_id),

    // MerchantAddressBind => ["wallet", "merchant", "address", "bind"] (with_client_id),
    // MerchantAddressAllock => ["wallet", "merchant", "address", "allock"] (with_client_id),
}

#[derive(Debug, serde::Deserialize, Clone)]
pub(crate) enum Topic {
    #[serde(rename = "wallet/common")]
    Common,
    #[serde(rename = "wallet/order")]
    Order,
    #[serde(rename = "wallet/switch")]
    Switch,
    #[serde(rename = "wallet/token")]
    Token,
    #[serde(rename = "wallet/bulletin")]
    BulletinInfo,
    #[serde(rename = "wallet/rpc/change")]
    RpcChange,
    #[serde(rename = "wallet/chain/change")]
    ChainChange,

    /// 推送的交易消息
    #[serde(rename = "aw/merchant/trans")]
    MerchantTrans,
    // /// 交易结果通知
    // #[serde(rename = "aw/merchant/trans")]
    // WalletMerchantTrans,
    /// 命令通知    
    #[serde(rename = "aw/merchant/cmd")]
    WalletMerchantCmd,
    // #[serde(rename = "wallet/merchant/address/bind")]
    // MerchantAddressBind,
    // #[serde(rename = "wallet/merchant/address/allock")]
    // MerchantAddressAllock,
}

impl Topic {
    // pub(crate) fn from_bytes(raw: Vec<u8>) -> Result<TopicClientId, anyhow::Error> {
    //     let topic = String::from_utf8(raw)?;
    //     let v: Vec<&str> = topic.split('/').collect();

    //     if v.is_empty() {
    //         return Err(anyhow::anyhow!("Invalid topic format"));
    //     }

    //     let (topic_str, client_id) = if v.len() == 4 && v[0] == "wallet" && v[1] == "token" {
    //         // 处理新的格式: wallet/token/{chainCode}/{tokenCode}
    //         // let chain_code = v[2].to_string();
    //         // let token_code = v[3].to_string();

    //         // 构造 topic 为 "wallet/token"
    //         let topic = format!("{}/{}", v[0], v[1]);

    //         // 可选地将 chainCode 和 tokenCode 作为 client_id 返回
    //         (topic, None) // 如果不需要可以设置为 None
    //     } else if v.len() == 3 && v[0] == "wallet" && v[1] == "rpc" && v[2] == "change" {
    //         let topic = format!("{}/{}/{}", v[0], v[1], v[2]);

    //         // 可选地将 chainCode 和 tokenCode 作为 client_id 返回
    //         (topic, None) // 如果不需要可以设置为 None
    //     } else if v.len() == 3 && v[0] == "wallet" && v[1] == "chain" && v[2] == "change" {
    //         let topic = format!("{}/{}/{}", v[0], v[1], v[2]);

    //         // 可选地将 chainCode 和 tokenCode 作为 client_id 返回
    //         (topic, None) // 如果不需要可以设置为 None
    //     } else if v.len() > 2 {
    //         // 动态构造 topic，最后一个部分作为 client_id
    //         let topic_parts = &v[..v.len() - 1]; // 去掉最后一个部分
    //         let topic = topic_parts.join("/"); // 拼接成 topic
    //         let client_id = v.last().map(|s| s.to_string());
    //         (topic, client_id)
    //     } else if v.len() == 2 {
    //         // 只有两个部分的情况，没有 client_id
    //         let topic = format!("{}/{}", v[0], v[1]);
    //         (topic, None)
    //     } else {
    //         return Err(anyhow::anyhow!("Invalid topic format"));
    //     };

    //     // 反序列化 topic 并构造返回结果
    //     let res = TopicClientId {
    //         topic: serde_json::from_value(serde_json::json!(topic_str))?,
    //         client_id,
    //     };

    //     Ok(res)
    // }

    // pub(crate) fn from_bytes_v2(raw: Vec<u8>) -> Result<TopicClientId, anyhow::Error> {
    //     let topic_str = String::from_utf8(raw)?;
    //     let parts: Vec<&str> = topic_str.split('/').collect();

    //     if parts.len() < 2 || parts[0] != "wallet" {
    //         return Err(anyhow::anyhow!("Invalid topic format"));
    //     }

    //     let (topic_enum, client_id) = match parts.as_slice() {
    //         ["wallet", "common", client_id] => (Topic::Common, Some(client_id.to_string())),
    //         ["wallet", "common"] => (Topic::Common, None),
    //         ["wallet", "order", client_id] => (Topic::Order, Some(client_id.to_string())),
    //         ["wallet", "order"] => (Topic::Order, None),
    //         ["wallet", "bulletin", client_id] => (Topic::BulletinInfo, Some(client_id.to_string())),
    //         ["wallet", "bulletin"] => (Topic::BulletinInfo, None),

    //         ["wallet", "switch"] => (Topic::Switch, None),
    //         ["wallet", "token", ..] => (Topic::Token, None), // chainCode/tokenCode 不是 client_id
    //         ["wallet", "rpc", "change"] => (Topic::RpcChange, None),
    //         ["wallet", "chain", "change"] => (Topic::ChainChange, None),
    //         _ => return Err(anyhow::anyhow!("Unknown topic format: {}", topic_str)),
    //     };

    //     Ok(TopicClientId {
    //         topic: topic_enum,
    //         client_id,
    //     })
    // }

    pub(crate) fn from_bytes_v3(raw: Vec<u8>) -> Result<TopicClientId, anyhow::Error> {
        let topic_str = String::from_utf8(raw)?;
        let topic_str = topic_str.trim_start_matches('/');

        let parts: Vec<&str> = topic_str.split('/').collect();
        for pattern in TOPIC_PATTERNS {
            if parts.starts_with(pattern.prefix) {
                let client_id = if pattern.extract_client_id {
                    if parts.len() == pattern.prefix.len() + 1 {
                        Some(parts.last().unwrap().to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };

                return Ok(TopicClientId { topic: pattern.topic.clone(), client_id });
            }
        }

        Err(anyhow::anyhow!("Unknown topic format: {}", topic_str))
    }
}

#[derive(Debug)]
pub(crate) struct TopicClientId {
    pub(crate) topic: Topic,
    #[allow(dead_code)]
    pub(crate) client_id: Option<String>,
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    fn check_topic(input: &str, expected_topic: Topic, expected_client_id: Option<&str>) {
        let raw = input.as_bytes().to_vec();
        let parsed = Topic::from_bytes_v3(raw).expect("parse failed");

        assert_eq!(
            std::mem::discriminant(&parsed.topic),
            std::mem::discriminant(&expected_topic),
            "Topic enum not matched for input: {}",
            input
        );
        assert_eq!(
            parsed.client_id.as_deref(),
            expected_client_id,
            "Client id mismatch for input: {}",
            input
        );
    }

    #[test]
    fn test_wallet_topics() {
        check_topic("wallet/common", Topic::Common, None);
        check_topic("wallet/common/abc123", Topic::Common, Some("abc123"));
        check_topic("wallet/order", Topic::Order, None);
        check_topic("wallet/order/xyz456", Topic::Order, Some("xyz456"));
        check_topic("wallet/switch", Topic::Switch, None);
        check_topic("wallet/bulletin", Topic::BulletinInfo, None);
        check_topic("wallet/bulletin/notice1", Topic::BulletinInfo, Some("notice1"));
        check_topic("wallet/token/eth/usdt", Topic::Token, None);
        check_topic("wallet/rpc/change", Topic::RpcChange, None);
        check_topic("wallet/chain/change", Topic::ChainChange, None);
    }

    #[test]
    fn test_aw_merchant_topics() {
        // 注意：这里测试带前导斜杠的情况
        check_topic(
            "/aw/merchant/cmd/df1b2982f3240f55fa8769e38e747010",
            Topic::WalletMerchantCmd,
            Some("df1b2982f3240f55fa8769e38e747010"),
        );

        check_topic("aw/merchant/trans/abcdef", Topic::MerchantTrans, Some("abcdef"));
    }

    #[test]
    fn test_invalid_topic() {
        let raw = "wallet/unknown/topic".as_bytes().to_vec();
        let res = Topic::from_bytes_v3(raw);
        assert!(res.is_err(), "unexpected success for invalid topic");
    }
}

// #[cfg(test)]
// mod tests {
// use super::*;

// fn check_equal(input: &str) {
//     let raw = input.as_bytes().to_vec();
//     let v1 = Topic::from_bytes(raw.clone());
//     let v2 = Topic::from_bytes_v2(raw.clone());
//     let v3 = Topic::from_bytes_v3(raw.clone());

//     match (v1, v2, v3) {
//         (Ok(a), Ok(b), Ok(c)) => {
//             println!("Comparing topic: a = {:?}, b = {:?}, c = {:?}", a, b, c);
//             let ad = std::mem::discriminant(&a.topic);
//             assert_eq!(ad, std::mem::discriminant(&b.topic));
//             assert_eq!(ad, std::mem::discriminant(&c.topic));
//             assert_eq!(a.client_id, b.client_id);
//             assert_eq!(a.client_id, c.client_id);
//         }
//         (Err(e1), Err(e2), Err(e3)) => {
//             // Allow different error messages as long as both fail
//             println!("Both failed as expected: \"{}\" vs \"{}\" vs \"{}\"", e1, e2, e3);
//         }
//         (res1, res2, res3) => {
//             eprintln!(
//                 "Mismatch for input '{}': from_bytes = {:?}, from_bytes_v2 = {:?}, from_bytes_v2 = {:?}",
//                 input, res1, res2, res3
//             );
//         }
//     }
// }

// #[test]
// fn test_topic_parsing_consistency() {
//     let cases = [
//         "wallet/common",
//         "wallet/common/abc123",
//         "wallet/order",
//         "wallet/order/xyz456",
//         "wallet/switch",
//         "wallet/bulletin",
//         "wallet/bulletin/notice1",
//         "wallet/token/eth/usdt",
//         "wallet/rpc/change",
//         "wallet/chain/change",
//         "wallet/unknown/topic", // Should error
//         "notwallet/common",     // Should error
//         "wallet",               // Invalid format
//     ];

//     for input in cases.iter() {
//         check_equal(input);
//     }
// }
// }
