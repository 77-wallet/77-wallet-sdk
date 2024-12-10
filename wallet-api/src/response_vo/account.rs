use wallet_types::chain::address::{category::AddressCategory, r#type::AddressType};

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
    // pub wallet_tree: wallet_keystore::WalletTree,
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

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountAddressRes(pub Vec<GetAccountAddress>);

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAccountAddress {
    pub chain_code: String,
    pub address: String,
    pub address_type: AddressType,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountResource {
    pub balance: String,
    pub energy: Resource,
    pub bandwidth: Resource,
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub limit_resource: i64,
    pub total_resource: i64,
    // trx
    pub total_freeze_amount: f64,
    pub owner_freeze_amount: f64,
    pub delegate_freeze_amount: f64,
    pub acquire_freeze_amount: f64,
    pub can_unfreeze_amount: f64,
    pub can_withdraw_unfreeze_amount: f64,
    pub price: f64,
}
impl Resource {
    pub fn set_total_freeeze_amount(&mut self, value: i64) {
        self.total_freeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_owner_freeze_amount(&mut self, value: i64) {
        self.owner_freeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_delegate_freeze_amount(&mut self, value: i64) {
        self.delegate_freeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_acquire_freeze_amount(&mut self, value: i64) {
        self.acquire_freeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_can_unfreeze_amount(&mut self, value: i64) {
        self.can_unfreeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_can_withdraw_unfreeze_amount(&mut self, value: i64) {
        self.can_withdraw_unfreeze_amount = (value / 100_0000) as f64;
    }

    pub fn set_price(&mut self, value: f64) {
        self.price = value;
    }
}

#[derive(Debug, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfo {
    // amount of token
    #[serde(serialize_with = "serialize_f64_as_string")]
    pub amount: f64,
    // currency of symbol
    pub currency: String,
    // unit price or currency
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub unit_price: Option<f64>,
    // fiat value of token
    #[serde(serialize_with = "default_unit_price_as_zero")]
    pub fiat_value: Option<f64>,
}

fn serialize_f64_as_string<S>(x: &f64, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    // if x.fract() == 0.0 {
    //     // 如果小数部分是 0，则序列化为整数
    //     s.serialize_str(&(*x as i64).to_string())
    // } else {
    //     // 否则序列化为浮点数
    //     s.serialize_str(&x.to_string())
    // }

    let rounded = if x.fract() == 0.0 {
        *x // 如果是整数，直接返回
    } else {
        let multiplier = 10f64.powi(8);
        (*x * multiplier).trunc() / multiplier // 截断到 8 位小数
    };

    // 根据是否是整数选择序列化方式
    if rounded.fract() == 0.0 {
        s.serialize_str(&(rounded as i64).to_string()) // 序列化为整数
    } else {
        s.serialize_str(&rounded.to_string()) // 序列化为浮点数
    }
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

// fn _serialize_f64_as_int<S>(x: &f64, s: S) -> Result<S::Ok, S::Error>
// where
//     S: serde::Serializer,
// {
//     if x.fract() == 0.0 {
//         // 如果小数部分是 0，则序列化为整数
//         s.serialize_i64(*x as i64)
//     } else {
//         // 否则序列化为浮点数
//         s.serialize_f64(*x)
//     }
// }

impl BalanceInfo {
    pub fn new(amount: f64, unit_price: Option<f64>, currency: &str) -> Self {
        let fiat_value = unit_price.map(|price| amount * price);
        Self {
            amount,
            currency: currency.to_string(),
            unit_price,
            fiat_value,
        }
    }

    pub async fn new_without_amount() -> Result<BalanceInfo, crate::ServiceError> {
        let config = crate::app_state::APP_STATE.read().await;
        let currency = config.currency();
        Ok(Self {
            amount: Default::default(),
            currency: currency.to_string(),
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

    pub fn calculate_fiat_value(&mut self, unit_price: Option<f64>) {
        self.fiat_value = unit_price.map(|price| self.amount * price);
        self.unit_price = unit_price;
    }

    pub fn fiat_add(&mut self, fiat_value: Option<f64>) {
        self.fiat_value = self.fiat_value.map_or(fiat_value, |current| {
            fiat_value.map(|value| current + value).or(Some(current))
        });
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedAddressesList {
    pub address: String,
    pub derivation_path: String,
    pub chain_code: String,
    pub address_type: AddressCategory,
}

impl DerivedAddressesList {
    pub fn new(
        adddress: &str,
        derivation_path: &str,
        chain_code: &str,
        address_type: AddressCategory,
    ) -> Self {
        Self {
            address: adddress.to_string(),
            derivation_path: derivation_path.to_string(),
            chain_code: chain_code.to_string(),
            address_type,
        }
    }
}
