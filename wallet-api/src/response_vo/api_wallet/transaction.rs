use wallet_chain_interact::BillResourceConsume;
use wallet_database::entities::{bill::BillEntity, multisig_queue::MemberSignedResult};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBillDetailVo {
    #[serde(flatten)]
    pub bill: BillEntity,
    pub resource_consume: Option<ApiWalletResourceConsume>,
    pub fee_symbol: String,
    pub signature: Option<Vec<MemberSignedResult>>,
    pub wallet_name: String,
    pub account_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWalletResourceConsume {
    pub bandwidth: u64,
    pub energy: u64,
}

impl ApiWalletResourceConsume {
    pub fn new(bandwidth: u64, energy: u64) -> Self {
        Self { bandwidth, energy }
    }

    pub fn from_bill_resource_consume(consume: BillResourceConsume) -> Self {
        Self::new(consume.net_used, consume.energy_used)
    }

    pub fn to_json_string(&self) -> Result<String, crate::error::service::ServiceError> {
        Ok(wallet_utils::serde_func::serde_to_string(self)?)
    }
}

pub(crate) fn api_wallet_resource_consume(raw: &str) -> Option<ApiWalletResourceConsume> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" {
        return None;
    }

    if let Ok(energy) = raw.parse::<u64>() {
        return Some(ApiWalletResourceConsume::new(0, energy));
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return None;
    };
    let bandwidth = first_u64(&value, &["bandwidth", "net_used", "netUsed"]);
    let energy = first_u64(&value, &["energy", "energy_used", "energyUsed"]);

    match (bandwidth, energy) {
        (Some(bandwidth), Some(energy)) => Some(ApiWalletResourceConsume::new(bandwidth, energy)),
        (Some(bandwidth), None) => Some(ApiWalletResourceConsume::new(bandwidth, 0)),
        (None, Some(energy)) => Some(ApiWalletResourceConsume::new(0, energy)),
        (None, None) => None,
    }
}

fn first_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| value.get(*key).and_then(serde_json::Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_wallet_resource_consume_reads_current_api_resource_keys() {
        let consume = api_wallet_resource_consume(r#"{"bandwidth":0,"energy":0}"#).unwrap();

        assert_eq!(consume, ApiWalletResourceConsume::new(0, 0));
    }

    #[test]
    fn api_wallet_resource_consume_reads_chain_resource_keys() {
        let consume = api_wallet_resource_consume(r#"{"net_used":12,"energy_used":34}"#).unwrap();

        assert_eq!(consume, ApiWalletResourceConsume::new(12, 34));
    }

    #[test]
    fn api_wallet_resource_consume_reads_energy_only_value() {
        let consume = api_wallet_resource_consume("56").unwrap();

        assert_eq!(consume, ApiWalletResourceConsume::new(0, 56));
    }
}
