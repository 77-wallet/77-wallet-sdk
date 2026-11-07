use crate::request::api_wallet::transfer::ApiTransferExReq;
use std::fmt;
use wallet_chain_interact::eth;
use wallet_database::{
    entities::{
        asset_token_key::AssetTokenKey,
        bill::{BillKind, NewBillEntity},
    },
    repositories::bill::BillRepo,
};
use wallet_utils::unit;

#[derive(Clone)]
pub struct TransferReq {
    pub base: BaseTransferReq,
    pub password: String,
    pub fee_setting: String,
    pub signer: Option<Signer>,
}

impl fmt::Debug for TransferReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferReq")
            .field("base", &self.base)
            .field("password", &"<redacted>")
            .field("fee_setting", &self.fee_setting)
            .field("signer", &self.signer)
            .finish()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signer {
    pub address: String,
    pub permission_id: i64,
}

#[derive(Debug, Clone)]
pub struct BaseTransferReq {
    pub from: String,
    pub to: String,
    pub value: String,
    pub chain_code: String,
    pub symbol: String,
    // 用户后端回收资源的id
    pub request_resource_id: Option<String>,
    // need
    pub token_address: AssetTokenKey,
    pub decimals: u8,
    pub spend_all: bool,
    pub notes: Option<String>,
}

impl BaseTransferReq {
    pub fn new(from: &str, to: &str, value: &str, chain_code: &str, symbol: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            value: value.to_string(),
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
            decimals: 0,
            request_resource_id: None,
            token_address: AssetTokenKey::Native,
            // address_type: None,
            spend_all: false,
            notes: None,
        }
    }
    pub fn with_decimals(&mut self, decimals: u8) {
        self.decimals = decimals;
    }

    pub fn with_spend_all(&mut self, spend_all: bool) {
        self.spend_all = spend_all;
    }

    pub fn with_token(&mut self, token_key: impl Into<AssetTokenKey>) {
        self.token_address = token_key.into();
    }

    pub fn with_notes(&mut self, notes: String) {
        self.notes = Some(notes);
    }
}

impl TryFrom<&BaseTransferReq> for eth::operations::TransferOpt {
    type Error = crate::error::service::ServiceError;

    fn try_from(req: &BaseTransferReq) -> Result<Self, Self::Error> {
        let value = unit::convert_to_u256(&req.value, req.decimals)?;

        let params = eth::operations::TransferOpt::new(
            &req.from,
            &req.to,
            value,
            req.token_address.to_chain_token_option(),
        )?;

        Ok(params)
    }
}

impl TryFrom<&TransferReq> for NewBillEntity {
    type Error = crate::error::service::ServiceError;

    fn try_from(req: &TransferReq) -> Result<Self, Self::Error> {
        let value = wallet_utils::unit::string_to_f64(&req.base.value)?;
        let mut res = BillRepo::build_bill(
            "".to_string(),
            req.base.from.clone(),
            req.base.to.clone(),
            value,
            req.base.chain_code.clone(),
            req.base.symbol.clone(),
            false,
            BillKind::Transfer,
            req.base.notes.clone().unwrap_or_default(),
        );
        res.token = req.base.token_address.clone();
        Ok(res)
    }
}

impl TryFrom<&ApiTransferExReq> for NewBillEntity {
    type Error = crate::error::service::ServiceError;

    fn try_from(req: &ApiTransferExReq) -> Result<Self, Self::Error> {
        let value = wallet_utils::unit::string_to_f64(&req.base.value)?;
        let mut res = BillRepo::build_bill(
            "".to_string(),
            req.base.from.clone(),
            req.base.to.clone(),
            value,
            req.base.chain_code.clone(),
            req.base.symbol.clone(),
            false,
            BillKind::Transfer,
            req.base.notes.clone().unwrap_or_default(),
        );
        res.token = req.base.token_address.clone();
        Ok(res)
    }
}

#[derive(Debug)]
pub struct QueryBillResultReq {
    pub tx_hash: String,
    pub owner: String,
}

#[cfg(test)]
mod tests {
    use super::{BaseTransferReq, TransferReq};
    use wallet_chain_interact::eth;
    use wallet_database::entities::asset_token_key::AssetTokenKey;

    fn make_base_req() -> BaseTransferReq {
        let mut req = BaseTransferReq::new(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            "1",
            "eth",
            "ETH",
        );
        req.with_decimals(18);
        req
    }

    #[test]
    fn base_transfer_try_from_uses_none_for_native_token_key() {
        let mut req = make_base_req();
        req.with_token(AssetTokenKey::Native);

        let result = eth::operations::TransferOpt::try_from(&req);
        assert!(result.is_ok());
    }

    #[test]
    fn base_transfer_try_from_rejects_invalid_contract_address() {
        let mut req = make_base_req();
        req.with_token(AssetTokenKey::Contract("not-a-contract-address".to_string()));

        let result = eth::operations::TransferOpt::try_from(&req);
        assert!(result.is_err());
    }

    #[test]
    fn base_transfer_with_token_normalizes_blank_as_native() {
        let mut req = make_base_req();
        req.with_token(Some("   ".to_string()));

        assert!(req.token_address.is_native());
    }

    #[test]
    fn base_transfer_try_from_treats_blank_contract_as_native() {
        let mut req = make_base_req();
        req.with_token(AssetTokenKey::Contract("   ".to_string()));

        let result = eth::operations::TransferOpt::try_from(&req);
        assert!(result.is_ok());
    }

    #[test]
    fn transfer_req_debug_redacts_password() {
        let mut base = make_base_req();
        base.with_notes("note".to_string());

        let req = TransferReq {
            base,
            password: "super-secret".to_string(),
            fee_setting: "fee".to_string(),
            signer: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
