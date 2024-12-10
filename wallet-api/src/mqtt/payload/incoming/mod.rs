pub mod announcement;
pub mod init;
pub mod rpc;
pub mod signature;
#[cfg(feature = "token")]
pub mod token;
pub mod transaction;

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Message {
    // 消息id
    pub(crate) msg_id: String,
    // 业务类型(一个枚举值)
    pub(crate) biz_type: BizType,
    // 业务数据 T 泛型
    pub(crate) body: serde_json::Value,
    // 客户端标识
    #[allow(dead_code)]
    pub(crate) client_id: String,
    // 设备号
    #[allow(dead_code)]
    pub(crate) sn: String,
    // 设备类型
    #[allow(dead_code)]
    pub(crate) device_type: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum BizType {
    // 多签订单通知
    /// ORDER_MULTI_SIGN_ACCEPT
    /// 订单多签-发起签名受理   同步多签账号(已测试)
    OrderMultiSignAccept,
    /// ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
    /// 订单多签-发起签名受理完成-消息通知
    OrderMultiSignAcceptCompleteMsg,
    /// ORDER_MULTI_SIGN_SERVICE_COMPLETE
    /// 订单多签-服务费收取完成
    OrderMultiSignServiceComplete,
    /// 订单多签-账户创建完成
    OrderMultiSignCreated,
    /// 订单多签-账户创建取消
    OrderMultiSignCancel,

    // /// 同步参与状态消息 发给发送者
    // SyncMultisigAccountStatus,
    // /// 订单多签-订单全部完成 多签订单完成后，所有人（发起人，参与人）都要收到消息
    // OrderMultiSignAllComplete,
    /// 多签转账-发起签名受理完成
    MultiSignTransAccept,
    /// 多签转账-发起签名受理完成
    MultiSignTransCancel,
    /// 多签转账-发起签名受理完成
    MultiSignTransAcceptCompleteMsg,

    /// 多签转账-确认完成
    MultiSignTransAcceptHashComplete,

    /// 账变
    AcctChange,

    /// 账户余额初始化
    Init,
    /// 代币价格变动
    TokenPriceChange,
    /// 公告
    BulletinMsg,
    /// 节点变动
    RpcAddressChange,
}

// impl<'de> Deserialize<'de> for BizType {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: Deserializer<'de>,
//     {
//         struct BizTypeVisitor;

//         impl<'de> Visitor<'de> for BizTypeVisitor {
//             type Value = BizType;

//             fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
//                 formatter.write_str("an integer between 1001 and 1006")
//             }

//             fn visit_i64<E>(self, value: i64) -> Result<BizType, E>
//             where
//                 E: de::Error,
//             {
//                 match value {
//                     1001 => Ok(BizType::ORDER_MULTI_SIGN_ACCEPT),
//                     1002 => Ok(BizType::MultisigMemberParticipationConfirmation),
//                     1003 => Ok(BizType::SyncMultisigAccountStatus),
//                     1004 => Ok(BizType::CompletePayment),
//                     1005 => Ok(BizType::SyncTransactionQueue),
//                     1006 => Ok(BizType::SyncTransactionSignature),
//                     _ => Err(de::Error::invalid_value(Unexpected::Signed(value), &self)),
//                 }
//             }

//             fn visit_u64<E>(self, value: u64) -> Result<BizType, E>
//             where
//                 E: de::Error,
//             {
//                 self.visit_i64(value as i64)
//             }
//         }

//         deserializer.deserialize_i64(BizTypeVisitor)
//     }
// }

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum Body {
    OrderMultiSignAccept(signature::OrderMultiSignAccept),
    OrderMultiSignAcceptCompleteMsg(signature::OrderMultiSignAcceptCompleteMsg),
    // SyncMultisigAccountStatus(signature::SyncMultisigAccountStatus),
    OrderMultiSignServiceComplete(signature::OrderMultiSignServiceComplete),
    OrderMultiSignCreated(signature::OrderMultiSignCreated),
    OrderMultiSignCancel(signature::OrderMultiSignCancel),
    MultiSignTransAccept(transaction::MultiSignTransAccept),
    MultiSignTransCancel(transaction::MultiSignTransCancel),

    MultiSignTransAcceptCompleteMsg(transaction::MultiSignTransAcceptCompleteMsg),
    /// 账变
    AcctChange(transaction::AcctChange),
    Init(init::Init),
    #[cfg(feature = "token")]
    TokenPriceChange(token::TokenPriceChange),
    BulletinMsg(announcement::BulletinMsg),
    RpcChange(rpc::RpcChange),
}

// transaction

#[cfg(test)]
mod test {
    use super::BizType;

    #[test]
    fn test_dese() {
        let data = serde_json::json!(1001);

        let res = serde_json::from_value::<BizType>(data);

        println!("res: {:?}", res);
    }
}

/*
test data
[
    {
        "client_id": "client126",
        "sn": "device459",
        "device_type": "typeD",
        "biz_type": 1004,
        "body": {
            "status": false,
            "multi_account_id": "uuid-2"
        }
    }

    {
        "client_id": "client128",
        "sn": "device461",
        "device_type": "typeF",
        "biz_type": 1006,
        "body": {
            "hash": "txhash-1",
            "address": "address2",
            "signature": "signature-1"
        }
    }
]


*/
