use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultisigServiceFee {
    pub id: String,
    pub name: String,
    pub code: String,
    pub chain_code: String,
    pub free: f64,
    pub price: f64,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct MultisigServiceFees {
    pub list: Vec<MultisigServiceFee>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositAddress {
    pub id: String,
    pub chain_code: String,
    pub address: String,
    pub enable: bool,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedCreateOrderReq {
    pub product_code: String,
    pub target_chain_code: String,
    pub target_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_sig_elements: Option<SignedElement>,
    pub multi_sig_address: String,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct SignedElement {
    pub salt: String,
}

impl SignedCreateOrderReq {
    pub fn new(chain_code: &str, address: &str, multisig_address: &str) -> Self {
        Self {
            product_code: "".to_string(),
            target_chain_code: chain_code.to_string(),
            target_address: address.to_string(),
            multi_sig_elements: None,
            multi_sig_address: multisig_address.to_string(),
        }
    }
    pub fn with_elements(mut self, elements: &str) -> Self {
        let elements = SignedElement {
            salt: elements.to_string(),
        };
        self.multi_sig_elements = Some(elements);
        self
    }
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedCreateOrderResp {
    pub order_id: String,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedUpdateSignedHashReq {
    pub order_id: String,
    pub hash: String,
    pub tx_str: String,
    pub multi_sig_address: String,
    pub multi_sig_elements: SignedElement,
    pub raw_data: String,
}

// biz_type = ORDER_MULTI_SIGN_CREATED
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultisigUpdateArg {
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
}
impl OrderMultisigUpdateArg {
    pub fn to_json_str(&self) -> Result<String, crate::Error> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

impl SignedUpdateSignedHashReq {
    pub fn new(
        order_id: &str,
        hash: &str,
        multisig_address: &str,
        multi_sig_elements: &str,
        tx_str: String,
        raw_data: String,
    ) -> Self {
        let element = SignedElement {
            salt: multi_sig_elements.to_string(),
        };
        Self {
            order_id: order_id.to_string(),
            hash: hash.to_string(),
            multi_sig_address: multisig_address.to_string(),
            multi_sig_elements: element,
            tx_str,
            raw_data,
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedUpdateRechargeHashReq {
    pub order_id: String,
    pub hash: String,
    pub product_code: String,
    pub receive_chain_code: String,
    pub receive_address: String,
    pub raw_data: String,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedSaveAddressReq {
    pub order_id: String,
    pub target_chain_code: String,
    pub target_address: String,
    pub address_list: Vec<String>,
    pub tx_str: String,
    pub raw_data: String,
}
impl SignedSaveAddressReq {
    pub fn new(
        order_id: &str,
        target_chain_code: &str,
        target_address: &str,
        address_list: Vec<String>,
        tx_str: &str,
        raw_data: String,
    ) -> Self {
        Self {
            order_id: order_id.to_string(),
            target_chain_code: target_chain_code.to_string(),
            target_address: target_address.to_string(),
            address_list,
            tx_str: tx_str.to_string(),
            raw_data,
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingedOrderCancelReq {
    pub order_id: String,
    pub raw_data: String,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOrderAcceptReq {
    pub order_id: String,
    pub accept_address: Vec<ConfirmedAddress>,
    pub status: i8,
    pub raw_data: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct ConfirmedAddress {
    pub address: String,
    pub pubkey: String,
    pub status: i8,
    pub uid: String,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindAddressRawData {
    pub chain_code: Option<String>,
    pub business_id: Option<String>,
    pub r#type: Option<String>,
    pub address: Option<String>,
    pub raw_data: Option<String>,
    pub raw_time: Option<String>,
}

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindAddressRawDataRes {
    pub list: Vec<FindAddressRawData>,
}
