use crate::domain::{account::AccountDomain, app::config::ConfigDomain};
use alloy::primitives::U256;
use rust_decimal::Decimal;
use wallet_database::entities::account::AccountEntity;
use wallet_types::chain::address::category::AddressCategory;

// 单笔交易需要花费的能量
pub const NET_CONSUME: f64 = 270.0;
// 代币转账消耗的能量
pub const ENERGY_CONSUME: f64 = 70000.0;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub balance: String,
    pub decimals: u8,
    pub original_balance: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TransactionFee {
    pub fee: String,
    pub symbol: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateAccountRes {
    pub address: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountPrivateKeyRes(pub Vec<GetAccountPrivateKey>);

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountPrivateKey {
    pub chain_code: String,
    pub name: String,
    pub address: String,
    pub address_type: AddressCategory,
    pub private_key: String,
}
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountResource {
    // 账户余额
    pub balance: f64,
    //  质押获得的票数
    pub votes: i64,
    // 公共质押了多少
    pub freeze_amount: i64,
    // 冻结的数量
    pub freeze_num: i64,
    // 解冻中的数量
    pub unfreeze_num: i64,
    // 待提取的数量
    pub pending_withdraw: i64,
    // 代理数量
    pub delegate_num: i64,
    // 带宽能量总质押数据
    pub total_freeze: BalanceInfo,
    // 总的解锁中的trx
    pub total_un_freeze: BalanceInfo,
    // 总的待提取trx
    pub total_pending_widthdraw: BalanceInfo,
    // 能量资源
    pub energy: Resource,
    // 带宽资源
    pub bandwidth: Resource,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
// unit is trx
pub struct Resource {
    // 可使用的资源
    pub limit_resource: i64,
    // 总的资源
    pub total_resource: i64,
    // 总共质押的数量
    pub total_freeze: TrxResource,
    // 自己质押的数量
    pub owner_freeze: TrxResource,
    // 解锁中的
    pub un_freeze: TrxResource,
    // 委派出去的数量
    pub delegate_freeze: TrxResource,
    // 别人给我的
    pub acquire_freeze: TrxResource,
    // 可提取的
    pub pending_withdraw: TrxResource,
    // 可以解质押的数量
    pub can_unfreeze: i64,
    // 可执行的转账次数
    pub transfer_times: f64,
    // 价格
    pub price: f64,
    // consumer 每笔交易对应的数量
    pub consumer: i64,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrxResource {
    pub amount: i64,
    pub value: f64,
}

impl TrxResource {
    pub fn new(amount: i64, price: f64) -> Self {
        Self { amount, value: (amount as f64 * price * 100.0).round() / 100.0 }
    }
}

impl Resource {
    pub fn calculate_total(&mut self) {
        let amount = self.owner_freeze.amount;

        let value = self.owner_freeze.value;

        self.total_freeze = TrxResource { amount, value }
    }

    // 计算可以转账的交易次数
    pub fn calculate_transfer_times(&mut self) {
        let rs = self.limit_resource as f64 / self.consumer as f64;

        self.transfer_times = (rs * 100.0).round() / 100.0;
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfo {
    // amount of token
    #[serde(
        serialize_with = "serialize_f64_as_string",
        deserialize_with = "wallet_utils::serde_func::string_to_f64"
    )]
    pub amount: f64,
    // currency of symbol
    pub currency: String,
    // unit price or currency
    #[serde(serialize_with = "zero_or_truncate")]
    pub unit_price: Option<f64>,
    // fiat value of token
    #[serde(serialize_with = "zero_or_truncate")]
    pub fiat_value: Option<f64>,
}

fn serialize_f64_as_string<S>(x: &f64, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if x.fract() == 0.0 {
        // 是整数，直接转为 i64 的字符串
        s.serialize_str(&(x.trunc() as i64).to_string())
    } else {
        // 使用 format 保留固定精度，避免浮点误差
        let formatted = format!("{:.8}", x);
        // 去除多余的尾部 0
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        s.serialize_str(trimmed)
    }
}

// none 为0 ，其余截断8位
pub fn zero_or_truncate<S>(price: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let number = match price {
        Some(val) => {
            // 转成字符串，截断小数点后 8 位
            let mut s = format!("{:.20}", val); // 保证有足够多的小数位
            if let Some(pos) = s.find('.') {
                let end = (pos + 9).min(s.len()); // 小数点后最多 8 位
                s.truncate(end);
            }
            s.parse::<f64>().unwrap_or(0.0)
        }
        None => 0.0,
    };
    serializer.serialize_f64(number)
}

pub fn default_unit_price_as_zero<S>(price: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match price {
        Some(val) => serializer.serialize_f64(*val),
        None => serializer.serialize_f64(0.0),
    }
}

impl BalanceInfo {
    pub fn new(amount: f64, unit_price: Option<f64>, currency: &str) -> Self {
        let fiat_value = unit_price.map(|price| amount * price);
        Self { amount, currency: currency.to_string(), unit_price, fiat_value }
    }

    pub async fn new_without_amount(
        ctx: &'static crate::context::Context,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        let currency = ConfigDomain::get_currency(ctx).await?;

        Ok(Self {
            amount: 0.0,
            currency,
            unit_price: Default::default(),
            fiat_value: Default::default(),
        })
    }

    pub fn calculate_amount_fiat_value(&mut self, amount: f64) {
        if let Some(unit_price) = self.unit_price {
            let fiat_value = amount * unit_price;
            self.fiat_value = Some(fiat_value);
        } else {
            self.fiat_value = None;
        }

        self.amount = amount;
    }

    pub fn amount_add(&mut self, amount: f64) {
        self.amount += amount;
    }

    // pub fn calculate_fiat_value(&mut self, unit_price: Option<f64>) {
    //     self.fiat_value = unit_price.map(|price| self.amount * price);
    //     self.unit_price = unit_price;
    // }

    pub fn fiat_add(&mut self, fiat_value: Option<f64>) {
        self.fiat_value = self.fiat_value.map_or(fiat_value, |current| {
            fiat_value.map(|value| current + value).or(Some(current))
        });
    }
}

// 不使用截断的返回原始的
#[derive(Debug, serde::Serialize, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceNotTruncate {
    // amount of token
    pub amount: Decimal,
    // currency of symbol
    pub currency: String,
    // unit price or currency
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub unit_price: Option<f64>,
    // fiat value of token
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub fiat_value: Option<f64>,
}

impl BalanceNotTruncate {
    pub fn new(
        amount: Decimal,
        unit_price: Option<Decimal>,
        currency: &str,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let fiat_decimal = unit_price.map(|p| amount * p);

        let unit_price_f64 =
            unit_price.map(|p| wallet_utils::conversion::decimal_to_f64(&p)).transpose()?;

        let fiat_value_f64 =
            fiat_decimal.map(|v| wallet_utils::conversion::decimal_to_f64(&v)).transpose()?;

        Ok(Self {
            amount,
            currency: currency.to_string(),
            unit_price: unit_price_f64,
            fiat_value: fiat_value_f64,
        })
    }
}

// 现在用于 quote的响应
#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BalanceStr {
    // amount of token
    pub amount: String,
    // currency of symbol
    pub currency: String,
    // unit price or currency
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub unit_price: Option<f64>,
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub fiat_value: Option<f64>,
}

impl BalanceStr {
    pub fn new(
        amount: U256,
        unit_price: Option<f64>,
        currency: &str,
        decimals: u8,
    ) -> Result<Self, crate::error::service::ServiceError> {
        let price_precision = 1_000_000_00u64;

        let fiat_value = if let Some(price) = unit_price {
            // 将价格扩大为整数
            let p = U256::from((price * price_precision as f64) as u64);

            let fiat_value = (amount * p) / U256::from(price_precision);

            Some(wallet_utils::unit::format_to_f64(fiat_value, decimals)?)
        } else {
            None
        };

        let amount = wallet_utils::unit::format_to_string(amount, decimals)?;

        Ok(Self {
            amount: amount.to_string(),
            currency: currency.to_string(),
            unit_price,
            fiat_value,
        })
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedAddressesList {
    pub address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub address_type: AddressCategory,
    pub mapping_account: Option<MappingAccount>,
    pub mapping_positive_index: Option<u32>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingAccount {
    pub account_id: u32,
    pub account_name: String,
}

impl DerivedAddressesList {
    pub fn new(
        address: &str,
        derivation_path: &str,
        chain_code: &str,
        address_type: AddressCategory,
    ) -> Self {
        Self {
            address: address.to_string(),
            derivation_path: derivation_path.to_string(),
            chain_code: chain_code.to_string(),
            address_type,
            mapping_account: None,
            mapping_positive_index: None,
        }
    }

    pub fn with_mapping_account(&mut self, account_id: u32, account_name: String) -> &mut Self {
        self.mapping_account = Some(MappingAccount { account_id, account_name });
        self
    }

    pub fn with_mapping_positive_index(&mut self, index: u32) -> &mut Self {
        self.mapping_positive_index = Some(index);
        self
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAccountDerivationPath {
    pub address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub address_type: AddressCategory,
}

impl QueryAccountDerivationPath {
    pub fn new(
        address: &str,
        derivation_path: &str,
        chain_code: &str,
        address_type: AddressCategory,
    ) -> Self {
        Self {
            address: address.to_string(),
            derivation_path: derivation_path.to_string(),
            chain_code: chain_code.to_string(),
            address_type,
        }
    }
}

impl TryFrom<AccountEntity> for QueryAccountDerivationPath {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: AccountEntity) -> Result<Self, Self::Error> {
        let address_type =
            AccountDomain::get_show_address_type(&value.chain_code, value.address_type())?;

        Ok(QueryAccountDerivationPath {
            address: value.address,
            derivation_path: value.derivation_path,
            chain_code: value.chain_code,
            address_type,
        })
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentAccountInfo {
    pub chain_code: String,
    pub address: String,
    pub address_type: AddressCategory,
    pub is_multisig: bool,
}

#[cfg(test)]
mod tests {

    #[test]
    pub fn test_balance_serializer() {
        let mut balance = crate::response_vo::standard_wallet::account::BalanceInfo::default();
        balance.calculate_amount_fiat_value(19.9);
        let json = serde_json::to_string(&balance).unwrap();
        println!("json: {}", json);

        assert!(json.contains("\"amount\":\"19.9\""), "序列化结果不包含正确的金额: {}", json);
    }

    #[test]
    pub fn test_balance_serializer1() {
        let mut balance = crate::response_vo::standard_wallet::account::BalanceInfo::default();
        balance.calculate_amount_fiat_value(1.0);
        let json = serde_json::to_string(&balance).unwrap();
        println!("json: {}", json);

        assert!(json.contains("\"amount\":\"1\""), "序列化结果不包含正确的金额: {}", json);
    }
}

// // 决定是否需要进行截断处理
// #[derive(Debug)]
// pub enum AmountFormat {
//     // 最原始的值
//     TruncateString(f64),
//     RawString(f64),
// }

// impl Default for AmountFormat {
//     fn default() -> Self {
//         AmountFormat::RawString(0.0)
//     }
// }

// impl serde::Serialize for AmountFormat {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::Serializer,
//     {
//         match self {
//             AmountFormat::TruncateString(v) => serialize_f64_as_string(v, serializer),
//             AmountFormat::RawString(v) => serializer.serialize_str(&v.to_string()),
//         }
//     }
// }

// impl<'de> serde::Deserialize<'de> for AmountFormat {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: serde::Deserializer<'de>,
//     {
//         struct AmountVisitor;

//         impl<'de> serde::de::Visitor<'de> for AmountVisitor {
//             type Value = AmountFormat;

//             fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
//                 formatter.write_str("a float or a string representing a float")
//             }

//             fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
//                 Ok(AmountFormat::RawString(value))
//             }

//             fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
//             where
//                 E: serde::de::Error,
//             {
//                 v.parse::<f64>()
//                     .map(AmountFormat::RawString)
//                     .map_err(E::custom)
//             }

//             fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
//             where
//                 E: serde::de::Error,
//             {
//                 self.visit_str(&v)
//             }

//             fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
//             where
//                 E: serde::de::Error,
//             {
//                 self.visit_f64(value as f64)
//             }

//             fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
//             where
//                 E: serde::de::Error,
//             {
//                 self.visit_f64(value as f64)
//             }
//         }

//         deserializer.deserialize_any(AmountVisitor)
//     }
// }
