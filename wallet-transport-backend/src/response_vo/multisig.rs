use serde::{Deserialize, Serialize};
use std::fmt;

// #[derive(Deserialize, Debug, Serialize)]
// #[serde(rename_all = "camelCase")]
// pub struct MultisigServiceFee {
//     pub id: String,
//     pub name: String,
//     pub code: String,
//     pub chain_code: String,
//     pub free: f64,
//     pub price: f64,
// }

// #[derive(Deserialize, Debug, Serialize)]
// #[deprecated = "Use MultisigServiceFeeInfo instead"]
// pub struct MultisigServiceFees {
//     pub list: Vec<MultisigServiceFee>,
// }

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultisigServiceFeeInfo {
    // 合约地址
    pub token_address: String,
    // 符号
    pub fee_token_code: String,
    // 手续费(抵扣后的手续费)
    pub free: f64,
    // 单价
    pub price: f64,
    // 是否使用积分
    pub use_score: bool,
    // 剩余积分
    pub score: i32,
    // 当前花费后剩余的积分
    pub current_cost_score: i32,
    // 抵扣前的手续费
    pub old_free: f64,
    // 积分交易ID
    pub score_trans_id: String,
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

#[derive(Deserialize, Serialize)]
pub struct SignedElement {
    pub salt: String,
    pub authority_addr: String,
}

impl fmt::Debug for SignedElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedElement")
            .field("salt", &"<redacted>")
            .field("authority_addr", &self.authority_addr)
            .finish()
    }
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
    pub fn with_elements(mut self, elements: &str, authority_addr: &str) -> Self {
        let elements = SignedElement {
            salt: elements.to_string(),
            authority_addr: authority_addr.to_string(),
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
#[derive(serde::Deserialize, serde::Serialize)]
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

impl fmt::Debug for OrderMultisigUpdateArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrderMultisigUpdateArg")
            .field("multisig_account_id", &self.multisig_account_id)
            .field("multisig_account_address", &self.multisig_account_address)
            .field("address_type", &self.address_type)
            .field("salt", &"<redacted>")
            .field("authority_addr", &self.authority_addr)
            .finish()
    }
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
        salt: &str,
        authority_addr: &str,
        tx_str: String,
    ) -> Self {
        let element =
            SignedElement { salt: salt.to_string(), authority_addr: authority_addr.to_string() };
        Self {
            order_id: order_id.to_string(),
            hash: hash.to_string(),
            multi_sig_address: multisig_address.to_string(),
            multi_sig_elements: element,
            tx_str,
            raw_data: "".to_string(),
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
    pub score_trans_id: String,
}

// 改版后 v2 接口 address-list
#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedSaveAddressReq {
    pub order_id: String,
    pub target_chain_code: String,
    pub target_address: String,
    // v1
    // pub address_list: Vec<String>,
    // v2
    pub address_list: Vec<AddressList>,
    pub tx_str: String,
    pub raw_data: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct AddressList {
    // 参与方名称
    pub name: String,
    // 参与方地址
    pub address: String,
    pub pubkey: String,
    // 确认状态
    pub confirmed: i8,
    pub uid: String,
}

impl fmt::Debug for AddressList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddressList")
            .field("name", &self.name)
            .field("address", &self.address)
            .field("pubkey", &self.pubkey)
            .field("confirmed", &self.confirmed)
            .field("uid", &self.uid)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressList, OrderMultisigUpdateArg, SignedElement};

    #[test]
    fn signed_element_debug_redacts_salt() {
        let req = SignedElement {
            salt: "salt-bytes".to_string(),
            authority_addr: "authority".to_string(),
        };
        let debug = format!("{req:?}");
        assert!(!debug.contains("salt-bytes"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn order_multisig_update_arg_debug_redacts_salt() {
        let req = OrderMultisigUpdateArg {
            multisig_account_id: "1".to_string(),
            multisig_account_address: "addr".to_string(),
            address_type: "type".to_string(),
            salt: "salt-bytes".to_string(),
            authority_addr: "authority".to_string(),
        };
        let debug = format!("{req:?}");
        assert!(!debug.contains("salt-bytes"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn address_list_debug_keeps_non_sensitive_fields() {
        let req = AddressList {
            name: "name".to_string(),
            address: "addr".to_string(),
            pubkey: "pubkey".to_string(),
            confirmed: 1,
            uid: "uid".to_string(),
        };
        let debug = format!("{req:?}");
        assert!(debug.contains("pubkey"));
    }
}

impl SignedSaveAddressReq {
    pub fn new(
        order_id: &str,
        target_chain_code: &str,
        target_address: &str,
        address_list: Vec<AddressList>,
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

#[derive(Deserialize, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultisigAccountIsCancelRes {
    pub status: bool,
}
