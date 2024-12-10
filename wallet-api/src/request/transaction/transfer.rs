use wallet_chain_interact::eth;
use wallet_utils::unit;

#[derive(Debug, Clone)]
pub struct TransferReq {
    pub base: BaseTransferReq,
    pub password: String,
    pub fee_setting: String,
    // pub notes: Option<String>,
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
    pub decimals: u8,
    pub token_address: Option<String>,
    // pub address_type: Option<String>,
    pub spend_all: bool,
    pub notes: Option<String>,
}

impl BaseTransferReq {
    pub fn new(
        from: String,
        to: String,
        value: String,
        chain_code: String,
        symbol: String,
    ) -> Self {
        Self {
            from,
            to,
            value,
            chain_code,
            symbol,
            decimals: 0,
            request_resource_id: None,
            token_address: None,
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

    pub fn with_token(&mut self, token_address: Option<String>) {
        self.token_address = token_address;
    }

    pub fn with_notes(&mut self, notes: String) {
        self.notes = Some(notes);
    }
}

impl TryFrom<&BaseTransferReq> for eth::operations::TransferOpt {
    type Error = crate::ServiceError;

    fn try_from(req: &BaseTransferReq) -> Result<Self, Self::Error> {
        let value = unit::convert_to_u256(&req.value, req.decimals)?;

        let params = eth::operations::TransferOpt::new(
            &req.from,
            &req.to,
            value,
            req.token_address.clone(),
        )?;

        Ok(params)
    }
}

impl TryFrom<&TransferReq> for wallet_database::entities::bill::NewBillEntity {
    type Error = crate::ServiceError;

    fn try_from(req: &TransferReq) -> Result<Self, Self::Error> {
        let value = wallet_utils::unit::string_to_f64(&req.base.value)?;
        let res = Self {
            hash: "".to_string(),
            from: req.base.from.clone(),
            to: req.base.to.clone(),
            token: req.base.token_address.clone(),
            value,
            multisig_tx: false,
            symbol: req.base.symbol.clone(),
            chain_code: req.base.chain_code.clone(),
            tx_type: 1,
            tx_kind: wallet_database::entities::bill::BillKind::Transfer,
            status: 1,
            queue_id: "".to_owned(),
            notes: req.base.notes.clone().unwrap_or_default(),
            transaction_fee: "0".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            block_height: "0".to_string(),
        };
        Ok(res)
    }
}

#[derive(Debug)]
pub struct QueryBillReusltReq {
    pub tx_hash: String,
    pub transfer_type: i64,
}
