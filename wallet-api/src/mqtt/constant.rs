#[derive(Debug, serde::Deserialize)]
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
}

impl From<Topic> for String {
    fn from(value: Topic) -> Self {
        match value {
            Topic::Common => "wallet/common".to_string(),
            Topic::Order => "wallet/order".to_string(),
            Topic::Switch => "wallet/switch".to_string(),
            Topic::Token => "wallet/token".to_string(),
            Topic::BulletinInfo => "wallet/bulletin".to_string(),
            Topic::RpcChange => "wallet/rpc/change".to_string(),
        }
    }
}

impl Topic {
    pub(crate) fn from_bytes(raw: Vec<u8>) -> Result<TopicClientId, anyhow::Error> {
        let topic = String::from_utf8(raw)?;
        let v: Vec<&str> = topic.split('/').collect();

        if v.is_empty() {
            return Err(anyhow::anyhow!("Invalid topic format"));
        }

        let (topic_str, client_id) = if v.len() == 4 && v[0] == "wallet" && v[1] == "token" {
            // 处理新的格式: wallet/token/{chainCode}/{tokenCode}
            // let chain_code = v[2].to_string();
            // let token_code = v[3].to_string();

            // 构造 topic 为 "wallet/token"
            let topic = format!("{}/{}", v[0], v[1]);

            // 可选地将 chainCode 和 tokenCode 作为 client_id 返回
            (topic, None) // 如果不需要可以设置为 None
        } else if v.len() == 3 && v[0] == "wallet" && v[1] == "rpc" && v[2] == "change" {
            let topic = format!("{}/{}/{}", v[0], v[1], v[2]);

            // 可选地将 chainCode 和 tokenCode 作为 client_id 返回
            (topic, None) // 如果不需要可以设置为 None
        } else if v.len() > 2 {
            // 动态构造 topic，最后一个部分作为 client_id
            let topic_parts = &v[..v.len() - 1]; // 去掉最后一个部分
            let topic = topic_parts.join("/"); // 拼接成 topic
            let client_id = v.last().map(|s| s.to_string());
            (topic, client_id)
        } else if v.len() == 2 {
            // 只有两个部分的情况，没有 client_id
            let topic = format!("{}/{}", v[0], v[1]);
            (topic, None)
        } else {
            return Err(anyhow::anyhow!("Invalid topic format"));
        };

        // 反序列化 topic 并构造返回结果
        let res = TopicClientId {
            topic: serde_json::from_value(serde_json::json!(topic_str))?,
            client_id,
        };

        Ok(res)
    }
}

pub(crate) struct TopicClientId {
    pub(crate) topic: Topic,
    #[allow(dead_code)]
    pub(crate) client_id: Option<String>,
}

#[cfg(test)]
mod test {
    use super::Topic;

    #[test]
    fn test_dese() {
        let data = serde_json::json!("wallet/common");

        let res = serde_json::from_value::<Topic>(data);

        println!("res: {:?}", res);
    }
}
