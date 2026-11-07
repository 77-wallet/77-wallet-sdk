use std::fmt;

use crate::request::{api_wallet::trans::ApiBaseTransferReq, transaction::Signer};

#[derive(Clone)]
pub struct ApiTransferExReq {
    pub base: ApiBaseTransferReq,
    pub password: String,
    pub fee_setting: String,
    pub signer: Option<Signer>,
}

impl fmt::Debug for ApiTransferExReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiTransferExReq")
            .field("base", &self.base)
            .field("password", &"<redacted>")
            .field("fee_setting", &self.fee_setting)
            .field("signer", &self.signer)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::ApiTransferExReq;
    use crate::request::api_wallet::trans::ApiBaseTransferReq;
    use wallet_database::entities::asset_token_key::AssetTokenKey;

    #[test]
    fn api_transfer_ex_req_debug_redacts_password() {
        let req = ApiTransferExReq {
            base: ApiBaseTransferReq {
                from: "from".to_string(),
                to: "to".to_string(),
                value: "1".to_string(),
                chain_code: "eth".to_string(),
                token_address: AssetTokenKey::Native,
                decimals: 18,
                symbol: "ETH".to_string(),
                request_resource_id: None,
                spend_all: false,
                notes: None,
                metadata: None,
            },
            password: "super-secret".to_string(),
            fee_setting: "fee".to_string(),
            signer: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
