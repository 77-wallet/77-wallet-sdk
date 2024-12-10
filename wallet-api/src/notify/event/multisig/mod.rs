// biz_type = ORDER_MULTI_SIGN_ACCEPT
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignAcceptFrontend {
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
    pub(crate) memeber: Vec<wallet_database::entities::multisig_member::MemberVo>,
}

// biz_type = ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignAcceptCompleteMsgFrontend {
    /// 参与状态(同意1,不同意0)
    pub status: i8,
    /// 多签钱包地址
    pub multisign_address: String,
    /// 参与方地址
    pub address_list: Vec<wallet_transport_backend::ConfirmedAddress>,
    pub accept_status: bool, // 参与人全部确认完
                             // pub confirm_list: Vec<crate::mqtt::payload::incoming::signature::Confirm>,
}

// biz_type = ORDER_MULTI_SIGN_SERVICE_COMPLETE
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignServiceCompleteFrontend {
    /// 多签钱包地址
    pub multisign_address: String,
    // 多签账号结果 true 多签账号或服务费执行完成  false 失败
    pub status: bool,
    // 1手续费 2服务费
    pub r#type: u8,
}

// biz_type = ORDER_MULTI_SIGN_CREATED
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignCreatedFrontend {
    /// 多签账户id
    pub multisig_account_id: String,
    /// 多签账户地址
    pub multisig_account_address: String,
    /// 地址类型
    pub address_type: String,
}

// biz_type = ORDER_MULTI_SIGN_CANCEL
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultisignCanceledFrontend {
    /// 多签账户id
    pub multisig_account_id: String,
    /// 多签账户地址
    pub multisig_account_address: String,
    /// 地址类型
    pub address_type: String,
}
