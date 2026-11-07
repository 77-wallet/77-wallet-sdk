use std::fmt;

use wallet_database::entities::asset_token_key::AssetTokenKey;

pub const COLLECT_IGNORE_SENDER_RENT_METADATA: &str = "__wallet_collect_ignore_sender_rent__";

#[derive(serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiWithdrawReq {
    pub uid: String, // 钱包
    pub from: String,
    pub to: String,
    pub value: String,
    pub validate: String,
    #[serde(rename = "chain")]
    pub chain_code: String,
    #[serde(rename = "token_addr")]
    pub token_address: AssetTokenKey,
    #[serde(rename = "token_code")]
    pub symbol: String,
    pub trade_no: String,
    // 交易类型： 1 提币 / 2 归集
    pub trade_type: u8,
    pub audit: u32,
    /// 交易在商户平台的单号
    pub out_order_id: Option<String>,
    /// 客户id
    pub client_id: Option<String>,
    /// 交易申请时间
    pub create_time: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiTransferFeeReq {
    pub uid: String, // 钱包
    pub from: String,
    pub to: String,
    pub value: String,
    pub validate: String,
    #[serde(rename = "chain")]
    pub chain_code: String,
    #[serde(rename = "token_addr")]
    pub token_address: AssetTokenKey,
    #[serde(rename = "token_code")]
    pub symbol: String,
    pub trade_no: String,
    // 交易类型： 1 提币 / 2 归集
    pub trade_type: u8,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiCollectReq {
    pub uid: String, // 钱包
    pub from: String,
    pub to: String,
    pub value: String,
    pub validate: String,
    #[serde(rename = "chain")]
    pub chain_code: String,
    #[serde(rename = "token_addr")]
    pub token_address: AssetTokenKey,
    #[serde(rename = "token_code")]
    pub symbol: String,
    pub trade_no: String,
    // 交易类型： 1 提币 / 2 归集
    pub trade_type: u8,
    pub risk_addr: u8,
}

#[derive(Debug, Clone)]
pub struct ApiBaseTransferReq {
    pub from: String,
    pub to: String,
    pub value: String,
    pub chain_code: String,
    pub token_address: AssetTokenKey,
    pub decimals: u8,
    pub symbol: String,
    // 用户后端回收资源的id
    pub request_resource_id: Option<String>,
    // pub address_type: Option<String>,
    pub spend_all: bool,
    pub notes: Option<String>,
    // 认为每个必的交易参数不完全相同，每个币的adapter自行解析特殊参数
    pub metadata: Option<String>,
}

impl ApiBaseTransferReq {
    pub fn new(from: &str, to: &str, value: &str, chain_code: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            value: value.to_string(),
            chain_code: chain_code.to_string(),
            token_address: AssetTokenKey::Native,
            decimals: 0,
            symbol: "".to_string(),
            request_resource_id: None,

            // address_type: None,
            spend_all: false,
            notes: None,
            metadata: None,
        }
    }

    pub fn with_token(&mut self, token_key: impl Into<AssetTokenKey>, decimals: u8, symbol: &str) {
        self.token_address = token_key.into();
        self.decimals = decimals;
        self.symbol = symbol.to_string();
    }

    // pub fn with_request_resource_id(&mut self, request_resource_id: Option<String>) {
    //     self.request_resource_id = request_resource_id
    // }

    // pub fn with_spend_all(&mut self, spend_all: bool) {
    //     self.spend_all = spend_all;
    // }

    // pub fn with_notes(&mut self, notes: String) {
    //     self.notes = Some(notes);
    // }
}

#[derive(Clone)]
pub struct ApiTransferReq {
    pub base: ApiBaseTransferReq,
    pub password: String,
    pub nonce: u64,
}

impl fmt::Debug for ApiTransferReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiTransferReq")
            .field("base", &self.base)
            .field("password", &"<redacted>")
            .field("nonce", &self.nonce)
            .finish()
    }
}

impl fmt::Debug for ApiWithdrawReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiWithdrawReq")
            .field("uid", &self.uid)
            .field("from", &self.from)
            .field("to", &self.to)
            .field("value", &self.value)
            .field("validate", &"<redacted>")
            .field("chain_code", &self.chain_code)
            .field("token_address", &self.token_address)
            .field("symbol", &self.symbol)
            .field("trade_no", &self.trade_no)
            .field("trade_type", &self.trade_type)
            .field("audit", &self.audit)
            .finish()
    }
}

impl fmt::Debug for ApiTransferFeeReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiTransferFeeReq")
            .field("uid", &self.uid)
            .field("from", &self.from)
            .field("to", &self.to)
            .field("value", &self.value)
            .field("validate", &"<redacted>")
            .field("chain_code", &self.chain_code)
            .field("token_address", &self.token_address)
            .field("symbol", &self.symbol)
            .field("trade_no", &self.trade_no)
            .field("trade_type", &self.trade_type)
            .finish()
    }
}

impl fmt::Debug for ApiCollectReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiCollectReq")
            .field("uid", &self.uid)
            .field("from", &self.from)
            .field("to", &self.to)
            .field("value", &self.value)
            .field("validate", &"<redacted>")
            .field("chain_code", &self.chain_code)
            .field("token_address", &self.token_address)
            .field("symbol", &self.symbol)
            .field("trade_no", &self.trade_no)
            .field("trade_type", &self.trade_type)
            .field("risk_addr", &self.risk_addr)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApiBaseTransferReq, ApiCollectReq, ApiTransferFeeReq, ApiTransferReq, ApiWithdrawReq,
    };
    use wallet_database::entities::asset_token_key::AssetTokenKey;

    #[test]
    fn api_transfer_req_debug_redacts_password() {
        let req = ApiTransferReq {
            base: ApiBaseTransferReq {
                from: "from".to_string(),
                to: "to".to_string(),
                value: "1".to_string(),
                chain_code: "eth".to_string(),
                token_address: AssetTokenKey::Native,
                decimals: 18,
                symbol: "ETH".to_string(),
                request_resource_id: Some("request-1".to_string()),
                spend_all: false,
                notes: Some("note".to_string()),
                metadata: Some("meta".to_string()),
            },
            password: "super-secret".to_string(),
            nonce: 42,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn api_withdraw_req_debug_redacts_validate() {
        let req = ApiWithdrawReq {
            uid: "uid".to_string(),
            from: "from".to_string(),
            to: "to".to_string(),
            value: "1".to_string(),
            validate: "validate-secret".to_string(),
            chain_code: "eth".to_string(),
            token_address: AssetTokenKey::Native,
            symbol: "ETH".to_string(),
            trade_no: "trade".to_string(),
            trade_type: 1,
            audit: 0,
            out_order_id: None,
            client_id: None,
            create_time: None,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("validate-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn api_transfer_fee_req_debug_redacts_validate() {
        let req = ApiTransferFeeReq {
            uid: "uid".to_string(),
            from: "from".to_string(),
            to: "to".to_string(),
            value: "1".to_string(),
            validate: "validate-secret".to_string(),
            chain_code: "eth".to_string(),
            token_address: AssetTokenKey::Native,
            symbol: "ETH".to_string(),
            trade_no: "trade".to_string(),
            trade_type: 3,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("validate-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn api_collect_req_debug_redacts_validate() {
        let req = ApiCollectReq {
            uid: "uid".to_string(),
            from: "from".to_string(),
            to: "to".to_string(),
            value: "1".to_string(),
            validate: "validate-secret".to_string(),
            chain_code: "eth".to_string(),
            token_address: AssetTokenKey::Native,
            symbol: "ETH".to_string(),
            trade_no: "trade".to_string(),
            trade_type: 2,
            risk_addr: 1,
        };

        let debug = format!("{req:?}");
        assert!(!debug.contains("validate-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
