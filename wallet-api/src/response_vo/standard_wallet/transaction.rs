use super::account::{
    BalanceInfo, BalanceNotTruncate, default_unit_price_as_zero, zero_or_truncate,
};
use crate::request::transaction::Signer;
use alloy::primitives::U256;
use wallet_chain_interact::{
    BillResourceConsume,
    eth::{self, FeeSetting},
    tron,
};
use wallet_database::entities::{
    asset_token_key::AssetTokenKey,
    bill::{BillEntity, BillKind},
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
    pub token_address: Option<String>,
    pub spend_all: Option<bool>,
}

impl MultisigQueueFeeParams {
    pub fn token_key(&self) -> AssetTokenKey {
        AssetTokenKey::from_raw(self.token_address.as_deref())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TransferParams {
    pub from: String,
    pub to: String,
    pub value: String,
    pub expiration: Option<i64>,
    pub chain_code: String,
    pub symbol: String,
    pub token_address: Option<String>,
    pub notes: Option<String>,
    pub spend_all: bool,
    pub signer: Option<Signer>,
}

impl TransferParams {
    pub fn token_key(&self) -> AssetTokenKey {
        AssetTokenKey::from_raw(self.token_address.as_deref())
    }
}

impl From<&TransferParams> for NewMultisigQueueEntity {
    fn from(value: &TransferParams) -> Self {
        let notes = value.notes.clone().unwrap_or_default();

        let now = wallet_utils::time::now().timestamp();
        let expiration = value.expiration.unwrap_or(1) * 3600 + now;

        Self {
            id: "".to_string(),
            from_addr: value.from.clone(),
            to_addr: value.to.clone(),
            value: value.value.clone(),
            symbol: value.symbol.clone(),
            expiration,
            chain_code: value.chain_code.to_string(),
            token_addr: AssetTokenKey::from(value.token_address.clone()),
            msg_hash: "".to_string(),
            tx_hash: "".to_string(),
            raw_data: "".to_string(),
            notes,
            status: MultisigQueueStatus::PendingSignature,
            signatures: vec![],
            account_id: "".to_string(),
            fail_reason: "".to_string(),
            create_at: wallet_utils::time::now(),
            transfer_type: BillKind::Transfer,
            permission_id: "".to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillDetailVo {
    #[serde(flatten)]
    pub bill: BillEntity,
    pub resource_consume: Option<BillResourceConsume>,
    pub fee_symbol: String,
    pub signature: Option<Vec<MemberSignedResult>>,
    pub wallet_name: String,
    pub account_name: String,
}

impl BillDetailVo {
    pub fn new(
        bill: BillEntity,
        fee_symbol: String,
        signature: Option<Vec<MemberSignedResult>>,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let resource_consume = if !bill.resource_consume.is_empty() && bill.resource_consume != "0"
        {
            Some(BillResourceConsume::from_json_str(&bill.resource_consume)?)
        } else {
            None
        };

        Ok(Self {
            bill,
            resource_consume,
            fee_symbol,
            signature,
            wallet_name: "".to_string(),
            account_name: "".to_string(),
        })
    }
}

// about fee estimate fee
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EstimateFeeResp {
    pub symbol: String,
    pub chain_code: String,
    pub content: String,
}
impl EstimateFeeResp {
    pub fn new(symbol: String, chain_code: String, content: String) -> Self {
        Self { symbol, chain_code, content }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeDetails<T>(Vec<FeeStructure<T>>);
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeDetailsVo<T> {
    default: String,
    pub data: Vec<FeeStructureVo<T>>,
}

// Scroll base fees can be far below0.1gwei. Keep the existing20% headroom,
// rounding upward in wei after the effective priority fee has been normalized.
pub(crate) fn scroll_fee_cap(
    base: U256,
    tip: U256,
) -> Result<U256, crate::error::service::ServiceError> {
    base.checked_add(tip)
        .and_then(|total| total.checked_mul(U256::from(12)))
        .and_then(|total| total.checked_add(U256::from(9)))
        .map(|total| total / U256::from(10))
        .ok_or_else(|| {
            crate::error::service::ServiceError::Parameter("Scroll fee cap overflow".into())
        })
}

// Quotes must carry the same gas/fee widths as signing, without adding RPCs or
// consuming the caller's settings. Never truncate oversized metadata fields.
pub(crate) fn scroll_fee_transaction(
    mut tx: alloy::rpc::types::TransactionRequest,
    fee: &FeeSetting,
) -> Result<alloy::rpc::types::TransactionRequest, crate::error::service::ServiceError> {
    let invalid = || {
        crate::error::service::ServiceError::Parameter(
            "Scroll fee setting exceeds transaction field width".into(),
        )
    };
    tx.gas = Some(fee.gas_limit.try_into().map_err(|_| invalid())?);
    tx.max_fee_per_gas = Some(fee.max_fee_per_gas.try_into().map_err(|_| invalid())?);
    tx.max_priority_fee_per_gas =
        Some(fee.max_priority_fee_per_gas.try_into().map_err(|_| invalid())?);
    Ok(tx)
}

impl FeeDetails<EthereumFeeDetails> {
    pub(crate) fn for_chain(
        gas_oracle: GasOracle,
        gas_limit: i64,
        code: wallet_types::chain::chain::ChainCode,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let mut details = Self::try_from((gas_oracle, gas_limit))?;
        if code == wallet_types::chain::chain::ChainCode::Scroll {
            for tier in &mut details.0 {
                let cap = scroll_fee_cap(
                    unit::u256_from_str(&tier.fee_setting.base_fee)?,
                    unit::u256_from_str(&tier.fee_setting.priority_fee)?,
                )?;
                tier.fee_setting.max_fee_per_gas = cap.to_string();
                tier.max_fee = U256::from(tier.fee_setting.gas_limit) * cap;
            }
        }
        Ok(details)
    }
    /// Ordinary-wallet transfer recommendations; other quote entrypoints retain their policy.
    pub(crate) fn for_standard_transfer(
        gas_oracle: GasOracle,
        gas_limit: i64,
        code: wallet_types::chain::chain::ChainCode,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let mut details = Self::for_chain(gas_oracle, gas_limit, code)?;
        if code == wallet_types::chain::chain::ChainCode::Polygon {
            let minimum_tip = U256::from(25_000_000_000_u64);
            for tier in &mut details.0 {
                if unit::u256_from_str(&tier.fee_setting.priority_fee)? < minimum_tip {
                    *tier = FeeStructure::new(
                        gas_limit,
                        unit::u256_from_str(&tier.fee_setting.base_fee)?,
                        minimum_tip,
                        &tier.types,
                    )?;
                }
            }
        }
        Ok(details)
    }

    // A single L1 reserve shared by all displayed tiers must cover each tier's
    // field widths, including a faster tier the user can select afterwards.
    pub(crate) fn reserve_fee_setting(
        &self,
    ) -> Result<FeeSetting, crate::error::service::ServiceError> {
        if self.0.is_empty() {
            return Err(crate::error::service::ServiceError::Parameter(
                "empty EVM fee tiers".into(),
            ));
        }
        let mut reserve = FeeSetting::default();
        for tier in &self.0 {
            reserve.gas_limit = reserve.gas_limit.max(U256::from(tier.fee_setting.gas_limit));
            reserve.max_fee_per_gas = reserve
                .max_fee_per_gas
                .max(unit::u256_from_str(&tier.fee_setting.max_fee_per_gas)?);
            reserve.max_priority_fee_per_gas = reserve
                .max_priority_fee_per_gas
                .max(unit::u256_from_str(&tier.fee_setting.priority_fee)?);
        }
        Ok(reserve)
    }

    pub fn with_additional_native_fee(
        mut self,
        additional_fee_wei: U256,
    ) -> Result<Self, crate::error::service::ServiceError> {
        // Keep quote arithmetic in wei; display precision must not change spendable amounts.
        for fee in &mut self.0 {
            fee.estimate_fee += additional_fee_wei;
            fee.max_fee += additional_fee_wei;
        }
        Ok(self)
    }

    pub(crate) fn with_base_l1_fee_margin(
        mut self,
        l1_fee: U256,
    ) -> Result<Self, crate::error::service::ServiceError> {
        // Reserve another 20% for quote-to-send movement, rounded up to wei.
        // Estimated fees and the submitted execution settings stay unchanged.
        let margin =
            l1_fee / U256::from(5) + U256::from(u8::from(l1_fee % U256::from(5) != U256::ZERO));
        for fee in &mut self.0 {
            fee.max_fee = fee.max_fee.checked_add(margin).ok_or_else(|| {
                crate::error::service::ServiceError::Parameter(
                    "Base L1 fee reserve overflow".into(),
                )
            })?;
        }
        Ok(self)
    }

    pub fn to_resp(
        self,
        token_price: TokenCurrency,
        currency: &str,
    ) -> Result<FeeDetailsVo<EthereumFeeDetails>, crate::error::service::ServiceError> {
        let mut res = vec![];

        let unit_price = Some(token_price.get_price(currency));
        for fee in self.0 {
            let fee_structure = FeeStructureVo {
                types: fee.types,
                estimate_fee: EvmFeeBalance::new(fee.estimate_fee, unit_price, currency)?,
                max_fee: EvmFeeBalance::new(fee.max_fee, unit_price, currency)?,
                fee_setting: fee.fee_setting,
            };
            res.push(fee_structure);
        }
        Ok(FeeDetailsVo { default: "propose".to_string(), data: res })
    }
}

impl TryFrom<(GasOracle, i64)> for FeeDetails<EthereumFeeDetails> {
    type Error = crate::error::service::ServiceError;
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
    pub estimate_fee: U256,
    pub max_fee: U256,
    pub fee_setting: T,
}

// Fee amounts drive maximum-transfer arithmetic; fiat fields are display-only.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvmFeeBalance {
    pub amount: String,
    pub currency: String,
    #[serde(serialize_with = "zero_or_truncate")]
    pub unit_price: Option<f64>,
    #[serde(serialize_with = "zero_or_truncate")]
    pub fiat_value: Option<f64>,
}

impl EvmFeeBalance {
    fn new(
        amount_wei: U256,
        unit_price: Option<f64>,
        currency: &str,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let display = BalanceInfo::new(
            unit::format_to_f64(amount_wei, eth::consts::ETH_DECIMAL)?,
            unit_price,
            currency,
        );
        Ok(Self {
            amount: unit::format_to_string(amount_wei, eth::consts::ETH_DECIMAL)?,
            currency: display.currency,
            unit_price: display.unit_price,
            fiat_value: display.fiat_value,
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeStructureVo<T> {
    #[serde(rename = "type")]
    pub types: String,
    pub estimate_fee: EvmFeeBalance,
    pub max_fee: EvmFeeBalance,
    pub fee_setting: T,
}

impl FeeStructure<EthereumFeeDetails> {
    pub fn new(
        gas_limit: i64,
        base_fee: U256,
        mut priority_fee: U256,
        types: &str,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let base_plus_tip = base_fee + priority_fee;

        // 1.2 倍
        let mut max_fee_per_gas = base_plus_tip * U256::from(12) / U256::from(10);

        // 向上取整到 0.1 gwei（1e8 wei）
        let scale = U256::from(10).pow(U256::from(8)); // 0.1 gwei
        max_fee_per_gas = ((max_fee_per_gas + (scale - U256::from(1))) / scale) * scale;
        max_fee_per_gas = max_fee_per_gas.max(base_plus_tip);

        // 避免节点最小费用拦截
        if priority_fee == U256::ZERO {
            priority_fee = U256::from(10000);
        }

        let fee_setting = EthereumFeeDetails::new(
            gas_limit,
            base_fee.to_string(),
            priority_fee.to_string(),
            max_fee_per_gas.to_string(),
        );

        let gas_limit = U256::from(gas_limit);
        let max_fee = gas_limit * max_fee_per_gas;

        let estimate_fee = gas_limit * (base_fee + priority_fee);

        Ok(Self { estimate_fee, max_fee, types: types.to_string(), fee_setting })
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
    type Error = crate::error::service::ServiceError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let rs: Self = serde_func::serde_from_str(value)?;
        Ok(rs)
    }
}

impl TryFrom<EthereumFeeDetails> for wallet_chain_interact::eth::FeeSetting {
    type Error = crate::error::service::ServiceError;

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
        Self { gas_limit, base_fee, priority_fee, max_fee_per_gas }
    }
}
impl From<FeeSetting> for EthereumFeeDetails {
    fn from(value: FeeSetting) -> Self {
        Self {
            gas_limit: value.gas_limit.to::<u64>() as i64,
            base_fee: value.base_fee.to_string(),
            priority_fee: value.max_priority_fee_per_gas.to_string(),
            max_fee_per_gas: value.max_fee_per_gas.to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
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
    ) -> Result<Self, crate::error::service::ServiceError> {
        let amount = consumer.transaction_fee();
        let amount = unit::string_to_f64(&amount)?;
        let fee = BalanceInfo::new(amount, Some(token_currency.get_price(currency)), currency);

        let (energy, energy_limit, energy_price) = if let Some(energy) = consumer.energy {
            (energy.consumer, energy.limit, energy.price as f64 / tron::consts::TRX_TO_SUN as f64)
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

// 目前在多签交易的时候使用,以及不需要显示块中慢
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonFeeDetails {
    pub estimate_fee: BalanceNotTruncate,
}

impl CommonFeeDetails {
    // fee unit is format
    pub fn new(
        fee: f64,
        token_currency: TokenCurrency,
        currency: &str,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let amount = wallet_utils::conversion::decimal_from_f64(fee).unwrap_or_default();

        let unit_price =
            Some(wallet_utils::conversion::decimal_from_f64(token_currency.get_price(currency))?);

        Ok(Self { estimate_fee: BalanceNotTruncate::new(amount, unit_price, currency)? })
    }

    pub fn to_json_str(&self) -> Result<String, crate::error::service::ServiceError> {
        Ok(wallet_utils::serde_func::serde_to_string(&self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_fee_json_preserves_wei_for_maximum_deduction() {
        let oracle = || GasOracle {
            safe_gas_price: Some("0.00501".into()),
            propose_gas_price: Some("0.00501".into()),
            fast_gas_price: Some("0.00501".into()),
            suggest_base_fee: Some("0.005".into()),
            gas_used_ratio: None,
        };
        let price =
            || TokenCurrency::new("base", "ETH", "ETH", Some(2000.0), Some(2000.0), 1.0, 18);
        for additional in [0_u64, 1, 1_591_910_335, 9_007_199_254_740_993] {
            let quote = FeeDetails::try_from((oracle(), 23101))
                .unwrap()
                .with_additional_native_fee(U256::from(additional))
                .unwrap()
                .to_resp(price(), "USD")
                .unwrap();
            let json = serde_json::to_value(quote).unwrap();
            for tier in json["data"].as_array().unwrap() {
                let fee = &tier["feeSetting"];
                let gas = U256::from(fee["gasLimit"].as_i64().unwrap());
                let max = gas * unit::u256_from_str(fee["maxFeePerGas"].as_str().unwrap()).unwrap()
                    + U256::from(additional);
                let estimate = gas
                    * (unit::u256_from_str(fee["baseFee"].as_str().unwrap()).unwrap()
                        + unit::u256_from_str(fee["priorityFee"].as_str().unwrap()).unwrap())
                    + U256::from(additional);
                let quoted =
                    unit::convert_to_u256(tier["maxFee"]["amount"].as_str().unwrap(), 18).unwrap();
                assert_eq!(quoted, max, "serialized maximum must retain every wei");
                let expected_display = serde_json::to_value(BalanceInfo::new(
                    unit::format_to_f64(max, 18).unwrap(),
                    Some(2000.0),
                    "USD",
                ))
                .unwrap();
                for field in ["currency", "unitPrice", "fiatValue"] {
                    assert_eq!(
                        tier["maxFee"][field], expected_display[field],
                        "display field {field}"
                    );
                }
                let roundtrip: FeeStructureVo<EthereumFeeDetails> =
                    serde_json::from_value(tier.clone()).unwrap();
                assert_eq!(roundtrip.max_fee.amount, tier["maxFee"]["amount"].as_str().unwrap());
                assert_eq!(
                    unit::convert_to_u256(tier["estimateFee"]["amount"].as_str().unwrap(), 18)
                        .unwrap(),
                    estimate
                );
                let balance = U256::from(1_000_000_000_000_000_000_u64);
                let sent = balance - quoted;
                assert_eq!(balance - sent, max, "App deduction must leave full fee reserve");
            }
        }
    }

    #[test]
    fn polygon_quote_floors_wallet_2935_priority_fee_at_node_minimum() {
        // Only the priority fee comes from the incident screenshot. The base and
        // total price are synthetic inputs; this does not identify its quote source.
        let oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some("124.991445339".into()),
            fast_gas_price: None,
            suggest_base_fee: Some("100".into()),
            gas_used_ratio: None,
        };
        let quote = FeeDetails::for_standard_transfer(
            oracle,
            23_100,
            wallet_types::chain::chain::ChainCode::Polygon,
        )
        .unwrap()
        .to_resp(TokenCurrency::new("polygon", "POL", "POL", None, None, 1.0, 18), "USD")
        .unwrap();

        // The App response and execution metadata must both carry the floor.
        let json = serde_json::to_value(&quote).unwrap();
        let tiers = json["data"].as_array().unwrap();
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0]["type"], "propose");
        assert_eq!(tiers[0]["feeSetting"]["priorityFee"], "25000000000");
        let settings: EthereumFeeDetails =
            serde_json::from_value(tiers[0]["feeSetting"].clone()).unwrap();
        let execution: wallet_chain_interact::eth::FeeSetting = settings.try_into().unwrap();
        assert_eq!(execution.max_priority_fee_per_gas, U256::from(25_000_000_000_u64));
        assert_eq!(execution.max_fee_per_gas, U256::from(150_000_000_000_u64));
        assert_eq!(execution.base_fee, U256::from(100_000_000_000_u64));
    }

    #[test]
    fn polygon_priority_floor_recalculates_low_tier_and_preserves_equal_and_higher_tiers() {
        let oracle = || GasOracle {
            safe_gas_price: Some("100".into()),
            propose_gas_price: Some("125".into()),
            fast_gas_price: Some("128".into()),
            suggest_base_fee: Some("100".into()),
            gas_used_ratio: None,
        };
        let original = FeeDetails::try_from((oracle(), 23_100)).unwrap();
        let shared =
            FeeDetails::for_chain(oracle(), 23_100, wallet_types::chain::chain::ChainCode::Polygon)
                .unwrap();
        assert_eq!(
            serde_json::to_value(&shared).unwrap(),
            serde_json::to_value(&original).unwrap(),
            "the shared API-wallet quote entry must retain its original policy"
        );
        let fixed = FeeDetails::for_standard_transfer(
            oracle(),
            23_100,
            wallet_types::chain::chain::ChainCode::Polygon,
        )
        .unwrap();
        assert_eq!(fixed.0.len(), 3);
        let low = &fixed.0[0];
        assert_eq!(low.types, "safe");
        assert_eq!(low.fee_setting.priority_fee, "25000000000");
        assert_eq!(low.fee_setting.max_fee_per_gas, "150000000000");
        assert_eq!(low.estimate_fee, U256::from(2_887_500_000_000_000_u64));
        assert_eq!(low.max_fee, U256::from(3_465_000_000_000_000_u64));
        for index in [1, 2] {
            assert_eq!(
                serde_json::to_value(&fixed.0[index]).unwrap(),
                serde_json::to_value(&original.0[index]).unwrap()
            );
        }
    }

    #[test]
    fn polygon_priority_floor_keeps_other_evm_chain_quotes_unchanged() {
        use wallet_types::chain::chain::ChainCode;
        for code in [
            ChainCode::Ethereum,
            ChainCode::BnbSmartChain,
            ChainCode::Avalanche,
            ChainCode::Arbitrum,
            ChainCode::Base,
            ChainCode::Scroll,
        ] {
            let oracle = GasOracle {
                safe_gas_price: None,
                propose_gas_price: Some("124.991445339".into()),
                fast_gas_price: None,
                suggest_base_fee: Some("100".into()),
                gas_used_ratio: None,
            };
            let quote = FeeDetails::for_standard_transfer(oracle, 23_100, code).unwrap();
            let tier = &quote.0[0];
            assert_eq!(tier.fee_setting.priority_fee, "24991445339", "{code:?}");
            let cap = if code == ChainCode::Scroll { "149989734407" } else { "150000000000" };
            assert_eq!(tier.fee_setting.max_fee_per_gas, cap, "{code:?}");
            assert_eq!(tier.estimate_fee, U256::from(2_887_302_387_330_900_u64), "{code:?}");
            assert_eq!(
                tier.max_fee,
                U256::from(23_100) * unit::u256_from_str(cap).unwrap(),
                "{code:?}"
            );
        }
    }

    #[test]
    fn ethereum_fee_details_preserve_fractional_bnb_gas_tracker_price() {
        let gas_oracle = GasOracle {
            safe_gas_price: None,
            propose_gas_price: Some("0.1".to_string()),
            fast_gas_price: None,
            suggest_base_fee: Some("0".to_string()),
            gas_used_ratio: None,
        };
        let token_currency =
            TokenCurrency::new("bnb", "BNB", "BNB", Some(643.98037376), None, 1.0, 18);

        let fee = FeeDetails::try_from((gas_oracle, 331_311))
            .unwrap()
            .to_resp(token_currency, "USD")
            .unwrap();

        assert_eq!(fee.data[0].fee_setting.gas_limit, 331_311);
        assert_eq!(fee.data[0].fee_setting.priority_fee, "100000000");
        assert!(
            (fee.data[0].estimate_fee.amount.parse::<f64>().unwrap() - 0.00003313).abs()
                < 0.00000001
        );
    }

    #[test]
    fn evm_additional_fee_is_added_to_each_quote_without_changing_execution_fee_setting() {
        let gas_oracle = || GasOracle {
            safe_gas_price: Some("1".to_string()),
            propose_gas_price: Some("2".to_string()),
            fast_gas_price: Some("3".to_string()),
            suggest_base_fee: Some("1".to_string()),
            gas_used_ratio: None,
        };
        let token_currency =
            || TokenCurrency::new("eth", "ETH", "ETH", Some(2_000.0), Some(2_000.0), 1.0, 18);

        let baseline = FeeDetails::try_from((gas_oracle(), 21_000))
            .unwrap()
            .to_resp(token_currency(), "USD")
            .unwrap();

        let fee = FeeDetails::try_from((gas_oracle(), 21_000))
            .unwrap()
            .with_additional_native_fee(U256::from(10_000_000_000_000_u64))
            .unwrap()
            .to_resp(token_currency(), "USD")
            .unwrap();

        assert_eq!(fee.data.len(), 3);
        for (actual, original) in fee.data.iter().zip(&baseline.data) {
            assert!(
                (actual.estimate_fee.amount.parse::<f64>().unwrap()
                    - original.estimate_fee.amount.parse::<f64>().unwrap()
                    - 0.00001)
                    .abs()
                    < 1e-12
            );
            assert!(
                (actual.max_fee.amount.parse::<f64>().unwrap()
                    - original.max_fee.amount.parse::<f64>().unwrap()
                    - 0.00001)
                    .abs()
                    < 1e-12
            );
            assert_eq!(
                serde_json::to_value(&actual.fee_setting).unwrap(),
                serde_json::to_value(&original.fee_setting).unwrap()
            );
        }
        assert!((fee.data[1].estimate_fee.amount.parse::<f64>().unwrap() - 0.000052).abs() < 1e-12);
        assert!((fee.data[1].max_fee.amount.parse::<f64>().unwrap() - 0.0000604).abs() < 1e-12);
        assert_eq!(fee.data[1].estimate_fee.fiat_value, Some(0.104));
        assert_eq!(fee.data[1].fee_setting.gas_limit, 21_000);
        assert_eq!(fee.data[1].fee_setting.max_fee_per_gas, "2400000000");
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinCurrency {
    pub currency: String,
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub unit_price: Option<f64>,
}
