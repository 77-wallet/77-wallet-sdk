use crate::response_vo;
use reqwest::Url;
use wallet_chain_interact::{btc::ParseBtcAddress, eth::FeeSetting, BillResourceConsume};
use wallet_types::chain::network;

pub mod adapter;
pub mod transaction;

pub struct TransferResp {
    pub tx_hash: String,
    pub fee: String,
    pub consumer: Option<BillResourceConsume>,
}
impl TransferResp {
    pub fn new(tx_hash: String, fee: String) -> Self {
        Self {
            tx_hash,
            fee,
            consumer: None,
        }
    }
    pub fn with_consumer(&mut self, consumer: BillResourceConsume) {
        self.consumer = Some(consumer);
    }

    pub fn resource_consume(&self) -> Result<String, crate::ServiceError> {
        if let Some(consumer) = &self.consumer {
            Ok(consumer.to_json_str()?)
        } else {
            Ok(String::new())
        }
    }
}

/// Parses a fee setting string into a `FeeSetting` struct.
pub fn pare_fee_setting(fee_setting: &str) -> Result<FeeSetting, crate::ServiceError> {
    fee_setting
        .try_into()
        .and_then(|s: response_vo::EthereumFeeDetails| FeeSetting::try_from(s))
}

pub fn rpc_need_header(url: &str) -> Result<bool, crate::ServiceError> {
    let url = Url::parse(url).expect("Invalid URL");

    Ok(url.host_str() == Some(wallet_transport_backend::consts::BASE_RPC_URL))
}

pub fn check_address(
    address: &str,
    chain: wallet_types::chain::chain::ChainCode,
    network: network::NetworkKind,
) -> Result<(), crate::error::ServiceError> {
    match chain {
        wallet_types::chain::chain::ChainCode::Bitcoin => {
            let parse = ParseBtcAddress::new(network);
            parse.parse_address(address).map(|_| true).map_err(|_| {
                crate::BusinessError::Account(crate::AccountError::AddressNotCorrect)
            })?
        }
        wallet_types::chain::chain::ChainCode::BnbSmartChain
        | wallet_types::chain::chain::ChainCode::Ethereum => {
            wallet_utils::address::parse_eth_address(address)
                .map(|_| true)
                .map_err(|_| {
                    crate::BusinessError::Account(crate::AccountError::AddressNotCorrect)
                })?
        }
        wallet_types::chain::chain::ChainCode::Tron => {
            if wallet_utils::address::is_tron_address(address) {
                true
            } else {
                return Err(crate::BusinessError::Account(
                    crate::AccountError::AddressNotCorrect,
                ))?;
            }
        }
        wallet_types::chain::chain::ChainCode::Solana => {
            wallet_utils::address::parse_sol_address(address)
                .map(|_| true)
                .map_err(|_| {
                    crate::BusinessError::Account(crate::AccountError::AddressNotCorrect)
                })?
        }
    };
    Ok(())
}

pub struct ChainDomain;

impl ChainDomain {
    pub(crate) async fn toggle_chains(
        chains: wallet_transport_backend::response_vo::chain::ChainList,
    ) -> Result<(), crate::ServiceError> {
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());
        let chains = chains
            .list
            .into_iter()
            .filter(|c| c.enable.is_some_and(|c| c) || c.enable.is_none())
            .map(|c| c.chain_code)
            .collect();

        wallet_database::repositories::chain::ChainRepoTrait::toggle_chains_status(
            &mut repo, chains,
        )
        .await?;
        Ok(())
    }
}
