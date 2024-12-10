use serde_json::Value;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTokenInitReq {
    pub chain_code: String,
    pub symbol: String,
    pub token_name: String,
    pub contract_address: Option<String>,
    pub master: bool,
    pub unit: u8,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenSubscribeReq {
    pub chain_code: String,
    pub address: String,
    pub index: Option<u32>,
    pub contract_account_address: Option<String>,
    pub uid: String,
    pub sn: String,
    pub app_id: String,
    pub device_type: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInitReq {
    pub device_type: String,
    pub sn: String,
    pub code: String,
    pub system_ver: String,
    pub iemi: Option<String>,
    pub meid: Option<String>,
    pub iccid: Option<String>,
    pub mem: Option<String>,
    // pub app_id: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDeleteReq {
    pub sn: String,
    pub uid_list: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBindAddressReq {
    pub sn: String,
    pub address: Vec<DeviceUnbindAddress>,
}

impl DeviceBindAddressReq {
    pub fn new(sn: &str) -> Self {
        Self {
            sn: sn.to_string(),
            address: Default::default(),
        }
    }

    pub fn push(&mut self, chain_code: &str, address: &str) {
        self.address
            .push(DeviceUnbindAddress::new(chain_code, address));
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceUnbindAddress {
    pub chain_code: String,
    pub address: String,
}

impl DeviceUnbindAddress {
    pub fn new(chain_code: &str, address: &str) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            address: address.to_string(),
        }
    }
}

impl DeviceDeleteReq {
    pub fn new(sn: &str, uid_list: &[String]) -> Self {
        Self {
            sn: sn.to_string(),
            uid_list: uid_list.to_vec(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindConfigByKey {
    pub key: String,
}

impl FindConfigByKey {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysInitReq {
    pub uid: String,
    pub sn: String,
    pub client_id: Option<String>,
    pub app_id: Option<String>,
    pub device_type: Option<String>,
    pub name: String,
}

impl KeysInitReq {
    pub fn new(
        uid: &str,
        sn: &str,
        client_id: Option<String>,
        app_id: Option<String>,
        device_type: Option<String>,
        name: &str,
    ) -> Self {
        Self {
            uid: uid.to_string(),
            sn: sn.to_string(),
            client_id,
            app_id,
            device_type,
            name: name.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryPriceReq(pub Vec<TokenQueryPrice>);

impl TokenQueryPriceReq {
    pub fn new(token_query_price: Vec<TokenQueryPrice>) -> Self {
        Self(token_query_price)
    }

    pub fn insert(&mut self, chain_code: &str, contract_address: &str) {
        // 尝试查找已存在的请求
        if let Some(existing_req) = self.0.iter_mut().find(|r| r.chain_code == chain_code) {
            // 如果找到相同 chain_code 的请求，合并 contract_address
            if !existing_req
                .contract_address_list
                .contains(&contract_address.to_string())
            {
                existing_req
                    .contract_address_list
                    .push(contract_address.to_string());
            }
        } else {
            // 如果没有找到，则创建一个新的请求
            self.0.push(crate::request::TokenQueryPrice {
                chain_code: chain_code.to_string(),
                contract_address_list: vec![contract_address.to_string()],
            });
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryPrice {
    pub chain_code: String,
    pub contract_address_list: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryByContractAddressReq {
    pub chain_code: String,
    pub contract_address: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryHistoryPrice {
    #[serde(serialize_with = "wallet_utils::serde_func::serialize_lowercase")]
    pub chain_code: String,
    #[serde(
        rename = "code",
        serialize_with = "wallet_utils::serde_func::serialize_lowercase"
    )]
    pub symbol: String,
    pub contract_address: String,
    pub date_type: String,
    #[serde(serialize_with = "wallet_utils::serde_func::serialize_lowercase")]
    pub currency: String,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryByPageReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_token: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popular_token: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_name_list: Option<Vec<String>>,
    #[serde(default)]
    pub page_num: Option<i32>,
    #[serde(default)]
    pub page_size: Option<i32>,
}

impl TokenQueryByPageReq {
    pub fn new_default_token(
        exclude_name_list: Vec<String>,
        page_num: i32,
        page_size: i32,
    ) -> Self {
        Self {
            order_column: Some("create_time".to_string()),
            order_type: Some("DESC".to_string()),
            chain_code: None,
            code: None,
            default_token: Some(true),
            popular_token: None,
            exclude_name_list: Some(exclude_name_list),
            page_num: Some(page_num),
            page_size: Some(page_size),
        }
    }

    pub fn new_popular_token(page_num: i32, page_size: i32) -> Self {
        Self {
            order_column: Some("create_time".to_string()),
            order_type: Some("DESC".to_string()),
            chain_code: None,
            code: None,
            default_token: None,
            popular_token: Some(true),
            exclude_name_list: None,
            page_num: Some(page_num),
            page_size: Some(page_size),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCancelSubscribeReq {
    pub address: String,
    pub contract_address: String,
    pub sn: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInstallSaveReq {
    pub sn: String,
    pub device_type: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionViewReq {
    pub device_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

impl VersionViewReq {
    pub fn new(device_type: &str, r#type: Option<String>) -> Self {
        Self {
            device_type: device_type.to_string(),
            r#type,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageInitReq {
    pub client_id: String,
    pub lan: String,
}

impl LanguageInitReq {
    pub fn new(client_id: &str, lan: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
            lan: lan.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementListReq {
    pub client_id: String,
    pub order_column: String,
    pub page_num: u32,
    pub page_size: u32,
}

impl AnnouncementListReq {
    pub fn new(client_id: String, page_num: u32, page_size: u32) -> Self {
        Self {
            client_id,
            order_column: "create_time".to_string(),
            page_num,
            page_size,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressInitReq {
    pub uid: String,
    pub address: String,
    pub index: i32,
    pub chain_code: String,
    pub sn: String,
    #[serde(default)]
    pub contract_address: Vec<String>,
    pub name: String,
}

impl AddressInitReq {
    pub fn new(
        uid: &str,
        address: &str,
        index: i32,
        chain_code: &str,
        sn: &str,
        contract_address: Vec<String>,
        name: &str,
    ) -> Self {
        Self {
            uid: uid.to_string(),
            address: address.to_string(),
            index,
            chain_code: chain_code.to_string(),
            sn: sn.to_string(),
            contract_address,
            name: name.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindAddressRawDataReq {
    pub uid: String,
    /// 类型：multisig：多签账户创建流程，trans：交易流程
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_time: Option<String>,
}

impl FindAddressRawDataReq {
    pub fn new(
        uid: &str,
        r#type: Option<String>,
        raw_time: Option<String>,
    ) -> Self {
        Self {
            uid: uid.to_string(),
            r#type,
            raw_time,
        }
    }

    pub fn new_multisig(uid: &str) -> Self {
        Self {
            uid: uid.to_string(),
            r#type: Some("multisig".to_string()),
            raw_time: None,
        }
    }

    pub fn new_trans(uid: &str, raw_time: Option<String>) -> Self {
        Self {
            uid: uid.to_string(),
            r#type: Some("trans".to_string()),
            raw_time,
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDetailsReq {
    pub address: String,
    pub chain_code: String,
}

impl AddressDetailsReq {
    pub fn new(address: &str, chain_code: &str) -> Self {
        Self {
            address: address.to_string(),
            chain_code: chain_code.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedSaveOrderReq {
    pub product_code: Option<String>,
    pub receive_chain_code: Option<String>,
    pub receive_address: Option<String>,
    pub receive_height: Option<u64>,
    pub target_chain_code: Option<String>,
    pub target_address: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedSaveHashReq {
    pub id: String,
    pub receive_trans_hash: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedFindAddressReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub chain_code: String,
}
impl SignedFindAddressReq {
    pub fn new(chain_code: &str) -> Self {
        Self {
            name: None,
            code: None,
            chain_code: chain_code.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SignedFeeListReq {
    #[serde(default)]
    pub chain_code: String,
}

impl SignedFeeListReq {
    pub fn new(chain_code: &str) -> Self {
        Self {
            chain_code: chain_code.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedTranCreateReq {
    pub withdraw_id: String,
    pub chain_code: String,
    pub address: String,
    pub tx_str: String,
    pub raw_data: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedTranAcceptReq {
    pub withdraw_id: String,
    pub accept_address: Vec<String>,
    pub tx_str: Value,
    pub status: i8,
    pub raw_data: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedTranUpdateHashReq {
    pub withdraw_id: String,
    pub hash: String,
    pub remark: String,
    pub raw_data: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBillReq {
    pub chain_code: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
}
impl SyncBillReq {
    pub fn new(chain_code: &str, address: &str, start_time: Option<String>) -> Self {
        Self {
            chain_code: chain_code.to_string(),
            address: address.to_string(),
            start_time,
            end_time: None,
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQueryPopularByPageReq {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<String>,
    pub page_num: i64,
    pub page_size: i64,
}

impl TokenQueryPopularByPageReq {
    pub fn new(
        code: Option<String>,
        chain_code: Option<String>,
        order_column: Option<String>,
        order_type: Option<String>,
        page_num: i64,
        page_size: i64,
    ) -> Self {
        Self {
            code,
            chain_code,
            order_column,
            order_type,
            page_num,
            page_size,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMsgConfirmReq {
    pub list: Vec<SendMsgConfirm>,
}

impl SendMsgConfirmReq {
    pub fn new(list: Vec<SendMsgConfirm>) -> Self {
        Self { list }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMsgConfirm {
    pub id: String,
    pub source: String,
}

impl SendMsgConfirm {
    pub fn new(id: String, source: &str) -> Self {
        Self {
            id,
            source: source.to_owned(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMsgQueryUnconfirmMsgReq {
    pub client_id: String,
}
