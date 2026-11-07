use anyhow::{Context, Result};
use std::time::Duration;
use tokio::{task::JoinSet, time::sleep};

use crate::{
    manager::WalletManager,
    request::api_wallet::{trans::ApiBaseTransferReq, transfer::ApiTransferExReq},
    response_vo::api_wallet::account::ApiAccountInfo,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchTransferConfig {
    pub chain_code: String,
    pub from_address: String,
    pub to_addresses: Vec<String>,
    pub value: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub max_in_flight: usize,
    pub start_interval_ms: u64,
    pub password: String,
    pub fee_setting: String,
}

impl Default for BatchTransferConfig {
    fn default() -> Self {
        Self {
            chain_code: "tron".to_string(),
            from_address: "".to_string(),
            to_addresses: vec![],
            value: "0".to_string(),
            token_symbol: "TRX".to_string(),
            token_decimals: 6,
            max_in_flight: 3,
            start_interval_ms: 300,
            password: String::new(),
            fee_setting: String::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchTransferResultItem {
    pub to_address: String,
    pub success: bool,
    pub detail: String,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchTransferSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub logs: Vec<BatchTransferResultItem>,
}

pub fn collect_target_addresses(list: Vec<ApiAccountInfo>, chain_code: &str) -> Vec<String> {
    list.into_iter()
        .filter_map(|account| {
            account
                .chain
                .into_iter()
                .find(|chain| chain.chain_code == chain_code)
                .map(|chain| chain.address)
        })
        .collect()
}

pub async fn fetch_subaccount_addresses(
    wallet_manager: &WalletManager,
    sub_wallet_address: &str,
    chain_code: &str,
) -> Result<Vec<String>> {
    let subaccounts = wallet_manager
        .list_api_wallet_account(sub_wallet_address, None, Some(chain_code.to_string()), 0, 500)
        .await?;

    Ok(collect_target_addresses(subaccounts.data, chain_code))
}

pub async fn run_batch_transfer(
    wallet_manager: WalletManager,
    config: &BatchTransferConfig,
) -> Result<BatchTransferSummary> {
    let mut summary =
        BatchTransferSummary { total: config.to_addresses.len(), ..Default::default() };

    let mut join_set = JoinSet::new();
    let mut submitted = 0usize;

    while submitted < config.to_addresses.len() || !join_set.is_empty() {
        while submitted < config.to_addresses.len() && join_set.len() < config.max_in_flight {
            let to_address = config.to_addresses[submitted].clone();
            submitted += 1;

            let wallet_manager = wallet_manager.clone();
            let from_address = config.from_address.to_string();
            let chain_code = config.chain_code.to_string();
            let value = config.value.to_string();
            let token_symbol = config.token_symbol.to_string();
            let token_decimals = config.token_decimals;
            let password = config.password.clone();
            let fee_setting = config.fee_setting.clone();

            join_set.spawn(async move {
                let mut base =
                    ApiBaseTransferReq::new(&from_address, &to_address, &value, &chain_code);
                base.with_token(None, token_decimals, &token_symbol);
                let req = ApiTransferExReq { base, password, fee_setting, signer: None };

                let res = wallet_manager.api_transfer(req).await;
                (to_address, res)
            });

            if submitted < config.to_addresses.len() {
                sleep(Duration::from_millis(config.start_interval_ms)).await;
            }
        }

        if let Some(joined) = join_set.join_next().await {
            match joined {
                Ok((to_address, res)) => {
                    let (success, detail) = match res {
                        Ok(resp) => (true, format!("{resp:?}")),
                        Err(err) => (false, format!("{err:?}")),
                    };

                    if success {
                        summary.success += 1;
                    } else {
                        summary.failed += 1;
                    }

                    summary.logs.push(BatchTransferResultItem { to_address, success, detail });
                }
                Err(err) => {
                    summary.failed += 1;
                    summary.logs.push(BatchTransferResultItem {
                        to_address: "unknown".to_string(),
                        success: false,
                        detail: format!("task join error: {err:?}"),
                    });
                }
            }
        }
    }

    Ok(summary)
}

pub async fn run_batch_transfer_from_subwallet(
    wallet_manager: &WalletManager,
    sub_wallet_address: &str,
    config: &BatchTransferConfig,
) -> Result<Vec<String>> {
    let mut addresses =
        fetch_subaccount_addresses(wallet_manager, sub_wallet_address, &config.chain_code)
            .await?
            .into_iter()
            .filter(|addr| addr != &config.from_address)
            .collect::<Vec<_>>();

    addresses.sort_unstable();
    Ok(addresses)
}

pub fn parse_usize(text: &str, default: usize) -> Result<usize> {
    text.trim().parse().with_context(|| format!("invalid usize: {text}")).or_else(|_| Ok(default))
}

pub fn parse_u64(text: &str, default: u64) -> Result<u64> {
    text.trim().parse().with_context(|| format!("invalid u64: {text}")).or_else(|_| Ok(default))
}

pub fn parse_u8(text: &str, default: u8) -> Result<u8> {
    text.trim().parse().with_context(|| format!("invalid u8: {text}")).or_else(|_| Ok(default))
}
