use anyhow::Result;

use crate::batch_transfer::{BatchTransferConfig, parse_u8, parse_u64, parse_usize};

#[derive(Debug, Clone)]
pub struct TransferForm {
    pub chain_code: String,
    pub from_address: String,
    pub sub_wallet_address: String,
    pub value: String,
    pub symbol: String,
    pub decimals: String,
    pub max_in_flight: String,
    pub start_interval_ms: String,
    pub fee_setting: String,
    pub to_addresses_raw: String,
}

impl Default for TransferForm {
    fn default() -> Self {
        Self {
            chain_code: "tron".to_string(),
            from_address: "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5".to_string(),
            sub_wallet_address: "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34".to_string(),
            value: "5".to_string(),
            symbol: "TRX".to_string(),
            decimals: "6".to_string(),
            max_in_flight: "3".to_string(),
            start_interval_ms: "300".to_string(),
            fee_setting: String::new(),
            to_addresses_raw: String::new(),
        }
    }
}

impl TransferForm {
    pub fn for_client(name: &str) -> Self {
        let mut form = Self::default();
        match name {
            "client4" => {
                form.sub_wallet_address = "0x5489c657Be2504D657f1F56AB04abfE3C77ceC34".to_string();
            }
            _ => {
                form.from_address.clear();
                form.sub_wallet_address.clear();
            }
        }
        form
    }

    pub fn build_config(&self, password: String) -> Result<BatchTransferConfig> {
        let decimals = parse_u8(&self.decimals, 6)?;
        let max_in_flight = parse_usize(&self.max_in_flight, 3)?;
        let start_interval_ms = parse_u64(&self.start_interval_ms, 300)?;
        let to_addresses = self
            .to_addresses_raw
            .lines()
            .map(str::trim)
            .filter(|addr| !addr.is_empty())
            .map(ToString::to_string)
            .collect();

        Ok(BatchTransferConfig {
            chain_code: self.chain_code.trim().to_string(),
            from_address: self.from_address.trim().to_string(),
            to_addresses,
            value: self.value.trim().to_string(),
            token_symbol: self.symbol.trim().to_string(),
            token_decimals: decimals,
            max_in_flight,
            start_interval_ms,
            password,
            fee_setting: self.fee_setting.clone(),
        })
    }
}
