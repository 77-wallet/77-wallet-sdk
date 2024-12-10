pub mod order_multisign_accept;
pub mod order_multisign_accept_complete_msg;
pub mod order_multisign_cancel;
pub mod order_multisign_created;
pub mod order_multisign_service_complete;

// biz_type = ORDER_MULTI_SIGN_ACCEPT
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignAccept {
    /// uuid
    pub(crate) id: String,
    /// order_id
    // pub(crate) order_id: String,
    /// 钱包名称
    pub(crate) name: String,
    /// 发起方地址
    pub(crate) initiator_addr: String,
    /// 多签钱包地址
    pub(crate) address: String,
    /// 链编码
    pub(crate) chain_code: String,
    /// 签名阀值
    pub(crate) threshold: i32,
    pub(crate) address_type: String,
    pub(crate) memeber: Vec<wallet_database::entities::multisig_member::MemberVo>,
}

impl OrderMultiSignAccept {
    pub fn to_json_str(&self) -> Result<String, crate::error::ServiceError> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderMultiSignAcceptCompleteMsg {
    /// 参与状态(同意1,不同意0)
    status: i32,
    /// 多签账户id
    multisig_account_id: String,
    /// 本次同意的参与方地址
    accept_address_list: Vec<String>,
    /// 所有参与方地址
    address_list: Vec<wallet_transport_backend::ConfirmedAddress>,
    accept_status: bool,
}

// biz_type = ORDER_MULTI_SIGN_CREATED
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignCreated {
    /// 多签账户id
    pub multisig_account_id: String,
    /// 多签账户地址
    pub multisig_account_address: String,
    /// 地址类型
    pub address_type: String,
    /// btc solana 盐
    pub salt: String,
    /// solana 管理地址
    pub authority_addr: String,
    /// 部署的hash
    pub deploy_hash: String,
    /// 服务费hash
    pub fee_hash: String,
}

impl OrderMultiSignAcceptCompleteMsg {
    pub(crate) fn name(&self) -> String {
        "ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG".to_string()
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Confirm {
    /// 参与方地址
    accept_address: String,
    /// 参与方确认状态
    accept_status: bool,
    status: i32,
    #[serde(flatten)]
    ext: serde_json::Value,
}

// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderMultiSignServiceComplete {
    // 多签账户id
    multisig_account_id: String,
    // 多签账号结果 true 多签账号或服务费执行完成  false 失败
    status: bool,
    // 1: 多签账号手续费 2: 服务费
    r#type: u8,
}

impl OrderMultiSignServiceComplete {
    pub(crate) fn name(&self) -> String {
        "ORDER_MULTI_SIGN_ACCEPT_COMPLETE".to_string()
    }
}

// biz_type = ORDER_MULTI_SIGN_CANCEL
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrderMultiSignCancel {
    // 多签账户id
    multisig_account_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serde_from_str() {
        let data = r#"
                {
                    "multisigAccountId": "193172761043144704",
                    "multisigAccountAddress": "TEJ4P1oYxXGMJ98tien1kLQbrRFGMu7YbT",
                    "addressType": "",
                    "salt": "",
                    "authorityAddr": "",
                    "deployHash": "c3592c1493a82281a2d8ba13128f6785970a74e4929bc11db93e064c830f9b60",
                    "feeHash": "c3592c1493a82281a2d8ba13128f6785970a74e4929bc11db93e064c830f9b60"
                }
        "#;

        let res = serde_json::from_str::<OrderMultiSignCreated>(data);
        println!("{:?}", res);
    }
}
