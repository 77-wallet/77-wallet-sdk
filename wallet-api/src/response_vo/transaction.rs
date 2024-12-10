use super::account::default_unit_price_as_zero;
use super::account::BalanceInfo;
use alloy::primitives::U256;
use wallet_chain_interact::{eth, tron};
use wallet_database::entities::{
    bill::BillEntity,
    multisig_queue::{MemberSignedResult, MultisigQueueStatus, NewMultisigQueueEntity},
};
use wallet_transport_backend::response_vo::{chain::GasOracle, coin::TokenCurrency};
use wallet_utils::{serde_func, unit};
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResult {
    pub tx_hash: String,
}

#[derive(Debug, serde::Serialize)]
pub struct MultisigQueueFeeParams {
    pub from: String,
    pub to: String,
    pub value: String,
    pub chain_code: String,
    pub symbol: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TransferParams {
    pub from: String,
    pub to: String,
    pub value: String,
    pub expiration: Option<i64>,
    pub chain_code: String,
    pub symbol: String,
    pub password: String,
    pub notes: Option<String>,
}

impl From<&TransferParams> for NewMultisigQueueEntity {
    fn from(value: &TransferParams) -> Self {
        let expiration = value.expiration.unwrap_or(1);
        let notes = value.notes.clone().unwrap_or_default();

        Self {
            id: "".to_string(),
            from_addr: value.from.clone(),
            to_addr: value.to.clone(),
            value: value.value.clone(),
            symbol: value.symbol.clone(),
            expiration,
            chain_code: value.chain_code.to_string(),
            token_addr: None,
            msg_hash: "".to_string(),
            tx_hash: "".to_string(),
            raw_data: "".to_string(),
            notes,
            status: MultisigQueueStatus::PendingSignature,
            signatures: vec![],
            account_id: "".to_string(),
            fail_reason: "".to_string(),
            create_at: wallet_utils::time::now(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillDetailVo {
    #[serde(flatten)]
    pub bill: BillEntity,
    pub resource_consume: Option<wallet_chain_interact::BillResourceConsume>,
    pub fee_symbol: String,
    pub signature: Option<Vec<MemberSignedResult>>,
}

// about fee estimate fee
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateFeeResp {
    pub symbol: String,
    pub chain_code: String,
    pub content: String,
}
impl EstimateFeeResp {
    pub fn new(symbol: String, chain_code: String, content: String) -> Self {
        Self {
            symbol,
            chain_code,
            content,
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeDetails<T>(Vec<FeeStructure<T>>);
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeDetailsVo<T> {
    default: String,
    data: Vec<FeeStructureVo<T>>,
}

impl FeeDetails<EthereumFeeDetails> {
    pub fn to_resp(
        self,
        token_price: TokenCurrency,
        currency: &str,
    ) -> FeeDetailsVo<EthereumFeeDetails> {
        let mut res = vec![];

        let unit_price = token_price.get_price(currency);
        for fee in self.0 {
            let fee_structure = FeeStructureVo {
                types: fee.types,
                estimate_fee: BalanceInfo::new(fee.estimate_fee, unit_price, currency),
                max_fee: BalanceInfo::new(fee.max_fee, unit_price, currency),
                fee_setting: fee.fee_setting,
            };
            res.push(fee_structure);
        }
        FeeDetailsVo {
            default: "propose".to_string(),
            data: res,
        }
    }
}

impl TryFrom<(GasOracle, i64)> for FeeDetails<EthereumFeeDetails> {
    type Error = crate::ServiceError;
    fn try_from((gas_oracle, gas_limit): (GasOracle, i64)) -> Result<Self, Self::Error> {
        let base =
            unit::convert_to_u256(&gas_oracle.suggest_base_fee.unwrap_or("0".to_string()), 9)?;

        // unit default is gwei , and to wei
        let mut res = vec![];

        if let Some(safe_gas_price) = gas_oracle.safe_gas_price {
            let safe = unit::convert_to_u256(&safe_gas_price, 9)?;
            let priority_fee = safe - base;
            let sales_fee = FeeStructure::new(gas_limit, base, priority_fee, "safe")?;
            res.push(sales_fee);
        }

        if let Some(propose_gas_price) = gas_oracle.propose_gas_price {
            let propose = unit::convert_to_u256(&propose_gas_price, 9)?;
            let priority_fee = propose - base;
            let propose_fee = FeeStructure::new(gas_limit, base, priority_fee, "propose")?;
            res.push(propose_fee);
        }

        if let Some(fast_gas_price) = gas_oracle.fast_gas_price {
            let fast = unit::convert_to_u256(&fast_gas_price, 9)?;
            let priority_fee = fast - base;
            let fast_fee = FeeStructure::new(gas_limit, base, priority_fee, "fast")?;
            res.push(fast_fee);
        }

        Ok(Self(res))
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeStructure<T> {
    #[serde(rename = "type")]
    pub types: String,
    pub estimate_fee: f64,
    pub max_fee: f64,
    pub fee_setting: T,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeStructureVo<T> {
    #[serde(rename = "type")]
    pub types: String,
    pub estimate_fee: BalanceInfo,
    pub max_fee: BalanceInfo,
    pub fee_setting: T,
}

impl FeeStructure<EthereumFeeDetails> {
    pub fn new(
        gas_limit: i64,
        base_fee: U256,
        priority_fee: U256,
        types: &str,
    ) -> Result<Self, crate::ServiceError> {
        let max_fee_per_gas = base_fee + priority_fee;
        // max fee add 1.5
        let multiplier = U256::from(15);
        let divisor = U256::from(10);
        let max_fee_per_gas = max_fee_per_gas * multiplier / divisor;

        let fee_setting = EthereumFeeDetails::new(
            gas_limit,
            base_fee.to_string(),
            priority_fee.to_string(),
            max_fee_per_gas.to_string(),
        );

        let gas_limit = U256::from(gas_limit);
        let max_fee = gas_limit * max_fee_per_gas;

        let estimate_fee = gas_limit * (base_fee + priority_fee);

        Ok(Self {
            estimate_fee: unit::format_to_f64(estimate_fee, eth::consts::ETH_DECIMAL)?,
            max_fee: unit::format_to_f64(max_fee, eth::consts::ETH_DECIMAL)?,
            types: types.to_string(),
            fee_setting,
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EthereumFeeDetails {
    pub gas_limit: i64,
    pub base_fee: String,
    pub priority_fee: String,
    pub max_fee_per_gas: String,
}

impl TryFrom<&str> for EthereumFeeDetails {
    type Error = crate::ServiceError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let rs: Self = serde_func::serde_from_str(value)?;
        Ok(rs)
    }
}

impl TryFrom<EthereumFeeDetails> for wallet_chain_interact::eth::FeeSetting {
    type Error = crate::ServiceError;

    fn try_from(value: EthereumFeeDetails) -> Result<Self, Self::Error> {
        Ok(Self {
            gas_limit: U256::from(value.gas_limit),
            base_fee: unit::u256_from_str(&value.base_fee)?,
            max_priority_fee_per_gas: unit::u256_from_str(&value.priority_fee)?,
            max_fee_per_gas: unit::u256_from_str(&value.max_fee_per_gas)?,
        })
    }
}
impl EthereumFeeDetails {
    pub fn new(
        gas_limit: i64,
        base_fee: String,
        priority_fee: String,
        max_fee_per_gas: String,
    ) -> Self {
        Self {
            gas_limit,
            base_fee,
            priority_fee,
            max_fee_per_gas,
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TronFeeDetails {
    pub estimate_fee: BalanceInfo,
    pub energy: u64,
    pub user_energy: u64,
    pub energy_price: f64,
    pub bandwidth: u64,
    pub user_bandwidth: u64,
    pub bandwidth_price: f64,
    pub extra_fee: f64,
}
impl TronFeeDetails {
    pub fn new(
        consumer: tron::params::ResourceConsumer,
        token_currency: TokenCurrency,
        currency: &str,
    ) -> Result<Self, crate::error::ServiceError> {
        let amount = consumer.transaction_fee();
        let amount = unit::string_to_f64(&amount)?;
        let fee = BalanceInfo::new(amount, token_currency.get_price(currency), currency);

        let (energy, energy_limit, energy_price) = if let Some(energy) = consumer.energy {
            (
                energy.consumer,
                energy.limit,
                energy.price as f64 / tron::consts::TRX_TO_SUN as f64,
            )
        } else {
            (0, 0, 0.0)
        };

        Ok(Self {
            estimate_fee: fee,
            energy: energy as u64,
            user_energy: energy_limit as u64,
            energy_price,
            bandwidth: consumer.bandwidth.consumer as u64,
            user_bandwidth: consumer.bandwidth.limit as u64,
            bandwidth_price: consumer.bandwidth.price as f64 / tron::consts::TRX_TO_SUN as f64,
            extra_fee: consumer.extra_fee as f64 / tron::consts::TRX_TO_SUN as f64,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct BitcoinFeeDetails {
    pub fee_rate: f64,
    pub size: i64,
}

// 目前在多签交易的时候使用
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonFeeDetails {
    pub estimate_fee: BalanceInfo,
}

impl CommonFeeDetails {
    // fee unit is format
    pub fn new(fee: f64, token_currency: TokenCurrency, currency: &str) -> Self {
        Self {
            estimate_fee: BalanceInfo::new(fee, token_currency.get_price(currency), currency),
        }
    }

    pub fn to_json_str(&self) -> Result<String, crate::ServiceError> {
        Ok(wallet_utils::serde_func::serde_to_string(&self)?)
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinCurrency {
    pub currency: String,
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub unit_price: Option<f64>,
}
