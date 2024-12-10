use sqlx::types::chrono::{DateTime, Utc};
#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BillEntity {
    pub hash: String,
    pub chain_code: String,
    pub symbol: String,
    // 0转入 1转出
    pub transfer_type: i8,
    // 1:普通交易，2:部署多签账号手续费 3:服务费
    pub tx_kind: i8,
    // 订单归属
    pub owner: String,
    pub from_addr: String,
    pub to_addr: String,
    pub token: Option<String>,
    pub value: String,
    // 需要跳过这个字段
    #[serde(skip_serializing)]
    pub resource_consume: String,
    pub transaction_fee: String,
    pub transaction_time: DateTime<Utc>,
    // 1-pending 2-成功 3-失败
    pub status: i8,
    // 1-是，0-否
    pub is_multisig: i8,
    pub block_height: String,
    pub queue_id: String,
    pub notes: String,
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

impl BillEntity {
    pub fn get_token(&self) -> String {
        if let Some(token) = &self.token {
            return token.clone();
        }
        "".to_string()
    }

    pub fn get_other_address(&self) -> String {
        if self.transfer_type == 1 {
            self.to_addr.clone()
        } else {
            self.from_addr.clone()
        }
    }

    // Different chains determine failure based on time (if the transaction result is not found when it expires, it is considered a failed transaction)
    pub fn is_failed(&self) -> bool {
        let chain_code =
            wallet_types::chain::chain::ChainCode::try_from(self.chain_code.as_str()).unwrap();

        match chain_code {
            wallet_types::chain::chain::ChainCode::Bitcoin => {
                self.transaction_time.timestamp() < Utc::now().timestamp() - 86400
            }
            wallet_types::chain::chain::ChainCode::Solana => {
                self.transaction_time.timestamp() < Utc::now().timestamp() - (20 * 60)
            }
            wallet_types::chain::chain::ChainCode::Ethereum => {
                self.transaction_time.timestamp() < Utc::now().timestamp() - (40 * 60)
            }
            wallet_types::chain::chain::ChainCode::Tron => {
                self.transaction_time.timestamp() < Utc::now().timestamp() - (40 * 60)
            }
            wallet_types::chain::chain::ChainCode::BnbSmartChain => {
                self.transaction_time.timestamp() < Utc::now().timestamp() - (30 * 60)
            }
        }
    }
}

pub enum BillStatus {
    Pending = 1,
    Success,
    Failed,
}

impl BillStatus {
    pub fn to_i8(&self) -> i8 {
        match self {
            BillStatus::Pending => 1,
            BillStatus::Success => 2,
            BillStatus::Failed => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BillKind {
    // 普通交易
    Transfer = 1,
    // 部署多签账号手续费
    DeployMultiSign,
    // 服务费
    ServiceCharge,
    // 多签交易
    MultiSignTx,
    // 多签交易签名费
    SigningFee,
}
impl BillKind {
    pub fn to_i8(&self) -> i8 {
        match self {
            BillKind::Transfer => 1,
            BillKind::DeployMultiSign => 2,
            BillKind::ServiceCharge => 3,
            BillKind::MultiSignTx => 4,
            BillKind::SigningFee => 5,
        }
    }
}

impl TryFrom<i8> for BillKind {
    type Error = crate::Error;

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(BillKind::Transfer),
            2 => Ok(BillKind::DeployMultiSign),
            3 => Ok(BillKind::ServiceCharge),
            4 => Ok(BillKind::MultiSignTx),
            5 => Ok(BillKind::SigningFee),
            _ => Err(crate::Error::Other(format!(
                "Invalid value for TxKind : {}",
                value
            ))),
        }
    }
}

// 创建的账单的类型
#[derive(Debug)]
pub struct NewBillEntity {
    pub hash: String,
    pub from: String,
    pub to: String,
    pub token: Option<String>,
    pub chain_code: String,
    pub symbol: String,
    pub status: i8,
    pub value: f64,
    pub transaction_fee: String,
    pub resource_consume: String,
    pub transaction_time: i64,
    pub multisig_tx: bool,
    pub tx_type: i8,
    pub tx_kind: BillKind,
    pub queue_id: String,
    pub block_height: String,
    pub notes: String,
}
impl NewBillEntity {
    pub fn new(
        hash: String,
        from: String,
        to: String,
        value: f64,
        chain_code: String,
        symbol: String,
        multisig_tx: bool,
        tx_kind: BillKind,
        notes: String,
    ) -> Self {
        Self {
            hash,
            from,
            to,
            token: None,
            value,
            multisig_tx,
            symbol,
            chain_code,
            tx_type: 1,
            tx_kind,
            status: 1,
            queue_id: "".to_owned(),
            notes,
            transaction_fee: "0".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            block_height: "0".to_string(),
        }
    }

    pub fn new_deploy_bill(
        hash: String,
        initiator_addr: String,
        chain_code: String,
        symbol: String,
    ) -> Self {
        Self {
            hash,
            from: initiator_addr.clone(),
            to: "".to_string(),
            token: None,
            chain_code,
            symbol,
            status: 1,
            value: 0.0,
            transaction_fee: "0".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            multisig_tx: false,
            tx_type: 1,
            tx_kind: BillKind::DeployMultiSign,
            queue_id: "".to_string(),
            block_height: "0".to_string(),
            notes: "".to_string(),
        }
    }

    pub fn new_signed_bill(hash: String, from: String, chain_code: String, symbol: String) -> Self {
        Self {
            hash,
            from,
            to: "".to_string(),
            token: None,
            chain_code,
            symbol,
            status: 1,
            value: 0.0,
            transaction_fee: "0".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            multisig_tx: false,
            tx_type: 1,
            tx_kind: BillKind::SigningFee,
            queue_id: "".to_string(),
            block_height: "0".to_string(),
            notes: "sign multisig tx transaction".to_string(),
        }
    }

    pub fn with_notes(mut self, notes: &str) -> Self {
        self.notes = notes.to_owned();
        self
    }

    pub fn with_token(mut self, token: &str) -> Self {
        self.token = Some(token.to_owned());
        self
    }
    pub fn with_status(mut self, status: i8) -> Self {
        self.status = status;
        self
    }
    pub fn with_tx_type(mut self, types: i8) -> Self {
        self.tx_type = types;
        self
    }
    pub fn with_queue_id(mut self, queue_id: &str) -> Self {
        self.queue_id = queue_id.to_owned();
        self
    }
    pub fn with_transaction_fee(mut self, transaction_fee: &str) -> Self {
        self.transaction_fee = transaction_fee.to_owned();
        self
    }
    pub fn with_transaction_time(mut self, transaction_time: i64) -> Self {
        self.transaction_time = transaction_time;
        self
    }
    pub fn with_block_height(mut self, block_height: &str) -> Self {
        self.block_height = block_height.to_owned();
        self
    }
    pub fn with_resource_consume(mut self, resource_consume: &str) -> Self {
        self.resource_consume = resource_consume.to_owned();
        self
    }

    pub fn get_owner(&self) -> String {
        if self.tx_type == 0 {
            self.to.clone()
        } else {
            self.from.clone()
        }
    }
}

#[derive(Debug)]
pub struct SyncBillEntity {
    pub tx_update: BillUpdateEntity,
    pub balance: String,
}

#[derive(Debug)]
pub struct BillUpdateEntity {
    pub hash: String,
    pub format_fee: String,
    pub transaction_time: u128,
    pub status: i8,
    pub block_height: u128,
    pub resource_consume: String,
}

impl BillUpdateEntity {
    pub fn new(
        hash: String,
        format_fee: String,
        time: u128,
        status: i8,
        block_height: u128,
        resource_consume: String,
    ) -> Self {
        Self {
            hash,
            format_fee,
            transaction_time: time,
            status,
            block_height,
            resource_consume,
        }
    }
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentBillListVo {
    pub chain_code: String,
    pub symbol: String,
    pub tx_hash: String,
    pub value: String,
    pub address: String,
    pub transaction_time: DateTime<Utc>,
    pub transfer_type: i8,
    pub created_at: DateTime<Utc>,
}

impl From<&BillEntity> for RecentBillListVo {
    fn from(item: &BillEntity) -> Self {
        Self {
            chain_code: item.chain_code.clone(),
            symbol: item.symbol.clone(),
            tx_hash: item.hash.clone(),
            value: item.value.clone(),
            address: item.get_other_address(),
            transaction_time: item.transaction_time,
            transfer_type: item.transfer_type,
            created_at: item.created_at,
        }
    }
}
