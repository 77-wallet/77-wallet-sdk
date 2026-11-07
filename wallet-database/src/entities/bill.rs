use super::has_expiration;
use crate::entities::asset_token_key::AssetTokenKey;
use serde::{Serialize, Serializer};
use sqlx::types::chrono::{DateTime, Utc};
use wallet_types::constant::chain_code;

#[derive(Debug, Default, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BillEntity {
    pub id: i32,
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
    pub token: AssetTokenKey,
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
    pub signer: String,
    // 针对每种订单的额外数据类 默认 “”
    pub extra: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl BillEntity {
    pub fn get_other_address(&self) -> String {
        if self.transfer_type == 1 { self.to_addr.clone() } else { self.from_addr.clone() }
    }

    // Different chains determine failure based on time (if the transaction result is not found when it expires, it is considered a failed transaction)
    pub fn is_failed(&self) -> bool {
        let chain_code =
            wallet_types::chain::chain::ChainCode::try_from(self.chain_code.as_str()).unwrap();

        has_expiration(self.transaction_time.timestamp(), chain_code)
    }

    // 截断金额
    pub fn truncate_to_8_decimals(&mut self) {
        let plain = expand_scientific_bill_value(&self.value);
        self.value =
            wallet_utils::unit::truncate_to_8_decimals(plain.as_deref().unwrap_or(&self.value));
    }
}

// Legacy SQLite REAL-to-TEXT writes can contain exponents. Move the decimal
// point in the original digits so expanding them never rounds through f64.
fn expand_scientific_bill_value(input: &str) -> Option<String> {
    let input = input.trim();
    let (mantissa, exponent) = input.split_once(['e', 'E'])?;
    let exponent = exponent.parse::<i64>().ok()?;
    // Historical values originate from finite f64 amounts. This also prevents
    // an invalid positive exponent from requesting an unbounded allocation.
    if !input.parse::<f64>().ok()?.is_finite() {
        return None;
    }
    let (sign, unsigned) = if let Some(rest) = mantissa.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = mantissa.strip_prefix('+') {
        ("+", rest)
    } else {
        ("", mantissa)
    };
    if unsigned.matches('.').count() > 1 {
        return None;
    }
    let digits = unsigned.replace('.', "");
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let integer_len = unsigned.find('.').unwrap_or(unsigned.len());
    let point = i64::try_from(integer_len).ok()?.checked_add(exponent)?;
    if digits.bytes().all(|b| b == b'0') || point <= -8 {
        return Some(format!("{sign}0"));
    }
    if point <= 0 {
        return Some(format!("{sign}0.{}{digits}", "0".repeat((-point) as usize)));
    }
    let point = usize::try_from(point).ok()?;
    if point >= digits.len() {
        Some(format!("{sign}{digits}{}", "0".repeat(point - digits.len())))
    } else {
        Some(format!("{sign}{}.{}", &digits[..point], &digits[point..]))
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

#[derive(Debug, Clone, Copy, serde_repr::Deserialize_repr)]
#[repr(u8)]
pub enum BillKind {
    // 普通交易
    Transfer = 1,
    // 部署多签账号手续费
    DeployMultiSign = 2,
    // 服务费
    ServiceCharge = 3,
    // 多签交易(not used)
    MultiSignTx = 4,
    // 多签交易签名费
    SigningFee = 5,
    // 质押带宽
    FreezeBandwidth = 6,
    // 质押能量
    FreezeEnergy = 7,
    // 解质带宽
    UnFreezeBandwidth = 8,
    // 解质能量
    UnFreezeEnergy = 9,
    // 全部取消质押
    CancelAllUnFreeze = 10,
    // 质押提取
    WithdrawUnFreeze = 11,
    // 委派带宽
    DelegateBandwidth = 12,
    // 委派能量
    DelegateEnergy = 13,
    // 批量委派(带宽)
    BatchDelegateBandwidth = 14,
    // 批量委派(能量)
    BatchDelegateEnergy = 15,
    // 回收带宽
    UnDelegateBandwidth = 16,
    // 回收能量
    UnDelegateEnergy = 17,
    // 批量回收带宽
    BatchUnDelegateBandwidth = 18,
    // 批量回收能量
    BatchUnDelegateEnergy = 19,
    // 投票
    Vote = 20,
    // 奖励提取
    WithdrawReward = 21,
    // 更新权限
    UpdatePermission = 22,
    // 授权交易
    Approve = 23,
    // 授权交易
    UnApprove = 24,
    // swap 交易
    Swap = 25,
    // Api Withdraw
    ApiWithdraw = 26,
}
impl Serialize for BillKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i8(*self as i8)
    }
}

impl BillKind {
    pub fn to_i8(&self) -> i8 {
        *self as i8
    }

    // // 订单类型进行转换(不需要了-按照最细的订单进行查询)
    // pub fn get_kinds(&self) -> Vec<i8> {
    //     match self {
    //         BillKind::Transfer => vec![BillKind::Transfer.to_i8()],
    //         BillKind::ServiceCharge => vec![BillKind::ServiceCharge.to_i8()],
    //         BillKind::MultiSignTx => vec![BillKind::MultiSignTx.to_i8()],
    //         BillKind::SigningFee => vec![BillKind::SigningFee.to_i8()],
    //         BillKind::DeployMultiSign => vec![BillKind::DeployMultiSign.to_i8()],
    //         BillKind::FreezeBandwidth | BillKind::FreezeEnergy => vec![
    //             BillKind::FreezeBandwidth.to_i8(),
    //             BillKind::FreezeEnergy.to_i8(),
    //         ],
    //         BillKind::UnFreezeBandwidth | BillKind::UnFreezeEnergy => vec![
    //             BillKind::UnFreezeBandwidth.to_i8(),
    //             BillKind::UnFreezeEnergy.to_i8(),
    //         ],
    //         BillKind::CancelAllUnFreeze => vec![BillKind::CancelAllUnFreeze.to_i8()],
    //         BillKind::WithdrawUnFreeze => vec![BillKind::WithdrawUnFreeze.to_i8()],
    //         BillKind::DelegateBandwidth | BillKind::DelegateEnergy => vec![
    //             BillKind::DelegateBandwidth.to_i8(),
    //             BillKind::DelegateEnergy.to_i8(),
    //         ],
    //         BillKind::BatchDelegateBandwidth | BillKind::BatchDelegateEnergy => vec![
    //             BillKind::BatchDelegateBandwidth.to_i8(),
    //             BillKind::BatchDelegateEnergy.to_i8(),
    //         ],

    //         BillKind::UnDelegateBandwidth | BillKind::UnDelegateEnergy => vec![
    //             BillKind::UnDelegateBandwidth.to_i8(),
    //             BillKind::UnDelegateEnergy.to_i8(),
    //         ],
    //         BillKind::BatchUnDelegateBandwidth | BillKind::BatchUnDelegateEnergy => vec![
    //             BillKind::BatchUnDelegateBandwidth.to_i8(),
    //             BillKind::BatchUnDelegateEnergy.to_i8(),
    //         ],
    //         BillKind::Vote => vec![BillKind::Vote.to_i8()],
    //         BillKind::WithdrawReward => vec![BillKind::WithdrawReward.to_i8()],
    //         BillKind::UpdatePermission => vec![BillKind::UpdatePermission.to_i8()],
    //         BillKind::Approve => vec![BillKind::Approve.to_i8()],
    //         BillKind::Swap => vec![BillKind::Swap.to_i8()],
    //     }
    // }

    // 金额是否转出的交易类型(针对本地的交易)
    pub fn out_transfer_type(&self) -> bool {
        matches!(
            self,
            BillKind::Transfer
                | BillKind::ServiceCharge
                | BillKind::MultiSignTx
                | BillKind::SigningFee
                | BillKind::DeployMultiSign
                | BillKind::FreezeBandwidth
                | BillKind::FreezeEnergy
        )
    }

    // 哪些交易类型是转入的的(在freeze中)
    pub fn in_transfer_type(&self) -> bool {
        matches!(
            self,
            BillKind::WithdrawUnFreeze
                | BillKind::WithdrawReward
                | BillKind::Swap
                | BillKind::Approve
                | BillKind::UnApprove
        )
    }

    // 这个交易类型是否需要创建系统通知
    pub fn needs_system_notify(&self) -> bool {
        matches!(self, BillKind::Transfer)
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
            6 => Ok(BillKind::FreezeBandwidth),
            7 => Ok(BillKind::FreezeEnergy),
            8 => Ok(BillKind::UnFreezeBandwidth),
            9 => Ok(BillKind::UnFreezeEnergy),
            10 => Ok(BillKind::CancelAllUnFreeze),
            11 => Ok(BillKind::WithdrawUnFreeze),
            12 => Ok(BillKind::DelegateBandwidth),
            13 => Ok(BillKind::DelegateEnergy),
            14 => Ok(BillKind::BatchDelegateBandwidth),
            15 => Ok(BillKind::BatchDelegateEnergy),
            16 => Ok(BillKind::UnDelegateBandwidth),
            17 => Ok(BillKind::UnDelegateEnergy),
            18 => Ok(BillKind::BatchUnDelegateBandwidth),
            19 => Ok(BillKind::BatchUnDelegateEnergy),
            20 => Ok(BillKind::Vote),
            21 => Ok(BillKind::WithdrawReward),
            22 => Ok(BillKind::UpdatePermission),
            23 => Ok(BillKind::Approve),
            24 => Ok(BillKind::UnApprove),
            25 => Ok(BillKind::Swap),
            _ => Err(crate::Error::Other(format!("Invalid value for TxKind : {}", value))),
        }
    }
}

// 创建的账单的类型
#[derive(Debug)]
pub struct NewBillEntity<T: serde::Serialize = String> {
    pub hash: String,
    pub from: String,
    pub to: String,
    pub token: AssetTokenKey,
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
    pub signer: Vec<String>,
    pub extra: Option<T>,
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
        let tx_type = if tx_kind.in_transfer_type() { 0 } else { 1 };

        Self {
            hash,
            from,
            to,
            token: AssetTokenKey::Native,
            value,
            multisig_tx,
            symbol,
            chain_code,
            tx_type,
            tx_kind,
            status: 1,
            queue_id: "".to_owned(),
            notes,
            transaction_fee: "0".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            block_height: "0".to_string(),
            signer: vec![],
            extra: None,
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
            token: AssetTokenKey::Native,
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
            signer: vec![],
            extra: None,
        }
    }

    pub fn new_signed_bill(hash: String, from: String, chain_code: String) -> Self {
        Self {
            hash,
            from,
            to: "".to_string(),
            token: AssetTokenKey::Native,
            chain_code,
            symbol: "SOL".to_string(),
            status: 1,
            value: 0.0,
            transaction_fee: "0.000005".to_string(),
            resource_consume: "".to_string(),
            transaction_time: 0,
            multisig_tx: false,
            tx_type: 1,
            tx_kind: BillKind::SigningFee,
            queue_id: "".to_string(),
            block_height: "0".to_string(),
            notes: "sign multisig tx transaction".to_string(),
            signer: vec![],
            extra: None,
        }
    }

    pub fn with_notes(mut self, notes: &str) -> Self {
        self.notes = notes.to_owned();
        self
    }

    pub fn with_token(mut self, token: &str) -> Self {
        self.token = AssetTokenKey::from(token);
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

    pub fn with_signer(mut self, signer: Vec<String>) -> Self {
        self.signer = signer;
        self
    }
}

// 代理类型的资源梳理额外信息
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillExtraResourceValue {
    // 具体代理了多少资源(质押，代理，回收)
    pub value: i64,
    // 资源类型:energy ,bandwidth
    pub resource_type: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillExtraVotes {
    // 投票的地址
    pub vote_addr: String,
    // 投票的数量
    pub votes: i64,
    // 节点名称
    pub name: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillExtraSwap {
    // 代币符号
    pub from_token_symbol: String,
    pub from_token_address: String,
    // 金额
    pub from_token_amount: f64,
    // 代币符号
    pub to_token_symbol: String,
    pub to_token_address: String,
    // 代币金额
    pub to_token_amount: f64,
    // 兑换比例
    pub price: f64,
}

impl<T: serde::Serialize> NewBillEntity<T> {
    // 构建质押相关的交易
    pub fn new_stake_bill(
        hash: String,
        from: String,
        to: String,
        value: f64,
        bill_kind: BillKind,
        bill_consumer: String,
        transaction_fee: String,
        extra: Option<T>,
    ) -> Self {
        let tx_type = if bill_kind.in_transfer_type() { 0 } else { 1 };

        Self {
            hash,
            from,
            to,
            token: AssetTokenKey::Native,
            chain_code: chain_code::TRON.to_string(),
            symbol: "TRX".to_string(),
            status: 1,
            value,
            transaction_fee,
            resource_consume: bill_consumer,
            transaction_time: 0,
            multisig_tx: false,
            tx_type,
            tx_kind: bill_kind,
            queue_id: "".to_string(),
            block_height: "0".to_string(),
            notes: "".to_string(),
            signer: vec![],
            extra,
        }
    }

    // 获取到订单的拥有者
    pub fn get_owner(&self) -> String {
        if self.tx_kind.in_transfer_type() {
            self.from.clone()
        } else if self.tx_type == 0 {
            self.to.clone()
        } else {
            self.from.clone()
        }
    }

    pub fn get_multisig_i32(&self) -> i32 {
        if self.multisig_tx { 1 } else { 0 }
    }

    pub fn get_symbol_and_to(&self) -> (String, String) {
        // 一笔代币转账两次账变通知、如果第一次是手续费的通知，symbol 为空，等第二次代币的通知在修改symbol
        // TODO 此处需要优化 不能简单的判断value = 0,在部署多签账号的时候有问题
        if self.value == 0.0
            && self.tx_kind.to_i8() == BillKind::Transfer.to_i8()
            && self.chain_code == chain_code::TRON
        {
            ("".to_string(), "".to_string())
        } else {
            (self.symbol.to_uppercase(), self.to.clone())
        }
    }

    pub fn get_singer_str(&self) -> String {
        let signer = self.signer.join(",");
        signer.trim_end_matches(",").to_string()
    }

    pub fn get_extra_str(&self) -> Result<String, crate::Error> {
        if let Some(extra) = &self.extra {
            Ok(wallet_utils::serde_func::serde_to_string(extra)?)
        } else {
            Ok("".to_string())
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
        Self { hash, format_fee, transaction_time: time, status, block_height, resource_consume }
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

#[cfg(test)]
mod decimal_display_tests {
    use super::BillEntity;

    #[test]
    fn bill_amount_displays_legacy_exponents_as_plain_decimals() {
        for (input, expected) in [
            ("1.118e-05", "0.00001118"),
            ("1.118e-08", "0.00000001"),
            ("1e-8", "0.00000001"),
            ("9e-9", "0"),
            ("5e-324", "0"),
            ("-1.118E-05", "-0.00001118"),
            ("1.234567891E+3", "1234.567891"),
            ("1.2345678901234567e+20", "123456789012345670000"),
            ("0e+308", "0"),
            ("0.842433", "0.842433"),
            ("0.123456789", "0.12345678"),
            ("12.34000000", "12.34"),
            ("", "0"),
        ] {
            let mut bill = BillEntity { value: input.into(), ..Default::default() };
            bill.truncate_to_8_decimals();
            assert_eq!(bill.value, expected, "input={input}");
        }
    }
}
