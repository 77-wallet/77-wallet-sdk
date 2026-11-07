pub(crate) mod catalog_sync;
pub mod token_price;
use std::collections::{HashMap, HashSet};

use super::app::config::ConfigDomain;
use crate::{
    context::Context,
    infrastructure::parse_utc_datetime,
    response_vo::standard_wallet::{
        chain::ChainList,
        coin::{CoinInfoList, TokenCurrencies, TokenCurrencyId},
    },
};
use chrono::{DateTime, Utc};
pub use token_price::TokenCurrencyGetter;
use wallet_database::{
    CoreDbPool,
    entities::{
        asset_token_key::AssetTokenKey,
        coin::{CoinData, CoinEntity, CoinId},
    },
    repositories::{
        chain::ChainRepo, coin::CoinRepo, exchange_rate::ExchangeRateRepo, node::NodeRepo,
    },
};
use wallet_transport_backend::{
    CoinInfo, request::TokenQueryPriceReq, response_vo::coin::TokenCurrency,
};
use wallet_types::chain::{chain::ChainCode, network::NetworkKind};

mod chain_stable_coin {
    pub const ETHEREUM: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
    pub const BNB_SMART_CHAIN: &str = "0x55d398326f99059fF775485246999027B3197955";
    pub const TRON_MAINNET: &str = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
    pub const TRON_TESTNET: &str = "TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf";
    pub const SOLANA: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
}

pub struct CoinDomain {}

impl Default for CoinDomain {
    fn default() -> Self {
        Self::new()
    }
}
impl CoinDomain {
    pub(crate) fn should_restore_local_defaults_after_backend(profile: &str) -> bool {
        matches!(profile, "dev")
    }

    pub fn new() -> Self {
        Self {}
    }

    pub async fn get_coin_by_token_key_with_ctx(
        ctx: &'static Context,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<CoinEntity, crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let coin = CoinRepo::coin_by_chain_token_key(chain_code, token_key, &core_pool).await?;

        Ok(coin)
    }

    pub async fn get_token_currencies_v2_with_ctx(
        ctx: &'static Context,
    ) -> Result<TokenCurrencies, crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let currency = ConfigDomain::get_currency(ctx).await?;

        let coins = CoinRepo::coin_list_v2(core_pool.clone(), None, None).await?;

        let exchange_rate_list = ExchangeRateRepo::list(core_pool).await?;
        // 查询本地的所有币符号
        let mut map = std::collections::HashMap::new();
        for coin in coins {
            let price = coin.price.parse::<f64>().unwrap_or_default();
            let (currency_price, rate) = if let Some(rate) =
                exchange_rate_list.iter().find(|rate| rate.target_currency == currency)
            {
                (price * rate.rate, rate.rate)
            } else {
                (f64::default(), f64::default())
            };

            let symbol = &coin.symbol;
            let chain_code = &coin.chain_code;

            let token_currency_id = TokenCurrencyId::new(
                &symbol.to_ascii_lowercase(),
                chain_code,
                coin.token_address.to_option_string_for_api(),
            );

            let token_currency = TokenCurrency {
                name: coin.name,
                chain_code: coin.chain_code,
                code: symbol.clone(),
                price: Some(price),
                currency_price: Some(currency_price),
                rate,
                decimals: coin.decimals,
            };
            map.insert(token_currency_id, token_currency);
        }

        Ok(TokenCurrencies(map))
    }

    pub(crate) fn merge_coin_to_list(
        coins: Vec<CoinEntity>,
        show_contract: bool,
    ) -> Result<CoinInfoList, crate::error::service::ServiceError> {
        let mut data = CoinInfoList::default();

        for coin in coins.into_iter() {
            if let Some(d) = data
                .iter_mut()
                .find(|info| info.symbol == coin.symbol && info.is_default && coin.is_default == 1)
            {
                d.chain_list
                    .entry(coin.chain_code.clone())
                    .or_insert(coin.token_address.as_db_str().to_string());
            } else {
                data.push(crate::response_vo::standard_wallet::coin::CoinInfo {
                    symbol: coin.symbol.clone(),
                    name: Some(coin.name.clone()),
                    chain_list: ChainList(HashMap::from([(
                        coin.chain_code.clone(),
                        coin.token_address.as_db_str().to_string(),
                    )])),
                    is_default: coin.is_default == 1,
                    hot_coin: coin.status == 1,
                    show_contract,
                })
            }
        }
        Ok(data)
    }

    pub(crate) async fn upsert_hot_coin_list(
        pool: &CoreDbPool,
        coins: Vec<CoinData>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let mut seen = std::collections::HashSet::new();
        let mut coin_data = Vec::with_capacity(coins.len());

        // filter repeat
        for coin in coins {
            let key = (
                coin.symbol.clone(),
                coin.chain_code.clone(),
                coin.token_address.as_db_str().to_string(),
            );

            if seen.insert(key) {
                coin_data.push(coin);
            }
        }

        CoinRepo::upsert_multi_coin(pool, coin_data).await?;
        Ok(())
    }

    pub async fn init_coins(
        core_pool: &CoreDbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        // check 本地表是否有数据,有则不进行新增
        let count = CoinRepo::coin_count(core_pool).await?;
        if count <= 0 {
            let list: Vec<CoinData> = crate::default_data::coin::mainnet_default_coins_list()?
                .coins
                .iter()
                .filter(|coin| {
                    crate::config::Config::active_feature_profile() == "dev"
                        || !crate::default_data::chain::is_dev_evm_chain(&coin.chain_code)
                })
                .chain(crate::default_data::coin::testnet_default_coins_list()?.coins.iter())
                .map(|coin| coin.to_owned().into())
                .collect();
            Self::upsert_hot_coin_list(core_pool, list).await?;
        }

        Ok(())
    }

    fn normalize_node_network(network: &str) -> &'static str {
        if network.eq_ignore_ascii_case("testnet") { "testnet" } else { "mainnet" }
    }

    pub async fn sync_default_coins_by_bound_nodes_with_ctx(
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let chains = ChainRepo::get_chain_list(&core_pool).await?;

        if chains.is_empty() {
            return Ok(());
        }

        let mut mainnet_defaults = crate::default_data::coin::mainnet_default_coins_list()?.clone();
        mainnet_defaults.coins.retain(|coin| {
            crate::config::Config::active_feature_profile() == "dev"
                || !crate::default_data::chain::is_dev_evm_chain(&coin.chain_code)
        });
        let testnet_defaults = crate::default_data::coin::testnet_default_coins_list()?;

        let mut activate: Vec<CoinData> = Vec::new();
        let mut deactivate_ids: Vec<CoinId> = Vec::new();
        let mut active_symbols: HashSet<(String, String)> = HashSet::new();
        let mut deactivated_key_set: HashSet<(String, String, String)> = HashSet::new();

        for chain in chains {
            let network = match &chain.node_id {
                Some(node_id) => {
                    if let Some(node) = NodeRepo::detail(&core_pool, node_id).await? {
                        Self::normalize_node_network(&node.network)
                    } else {
                        tracing::warn!(chain_code = %chain.chain_code, node_id = %node_id, "bound node missing, fallback to mainnet for default coin sync");
                        "mainnet"
                    }
                }
                None => {
                    tracing::warn!(chain_code = %chain.chain_code, "chain has no bound node, fallback to mainnet for default coin sync");
                    "mainnet"
                }
            };

            let (active_profile, inactive_profile) = if network == "testnet" {
                (&testnet_defaults.coins, &mainnet_defaults.coins)
            } else {
                (&mainnet_defaults.coins, &testnet_defaults.coins)
            };

            let chain_code = chain.chain_code.to_ascii_lowercase();

            for coin in
                active_profile.iter().filter(|c| c.chain_code.eq_ignore_ascii_case(&chain_code))
            {
                active_symbols.insert((coin.chain_code.clone(), coin.symbol.clone()));
                activate.push(CoinData::from(coin.clone()).with_status(1));
            }

            for coin in
                inactive_profile.iter().filter(|c| c.chain_code.eq_ignore_ascii_case(&chain_code))
            {
                if !active_symbols.contains(&(coin.chain_code.clone(), coin.symbol.clone())) {
                    continue;
                }
                // Native coins can have the same key in both network catalogs.
                // Do not disable a key activated for the selected network above.
                if active_profile.iter().any(|active| {
                    active.chain_code.eq_ignore_ascii_case(&chain_code)
                        && active.token_address.as_db_str() == coin.token_address.as_db_str()
                }) {
                    continue;
                }
                let key = (
                    coin.chain_code.clone(),
                    coin.symbol.clone(),
                    coin.token_address.as_db_str().to_string(),
                );
                if deactivated_key_set.insert(key.clone()) {
                    deactivate_ids.push(CoinId::new(
                        &key.0,
                        &key.1,
                        wallet_database::entities::asset_token_key::AssetTokenKey::from_raw(Some(
                            key.2.as_str(),
                        )),
                    ));
                }
            }
        }

        if !activate.is_empty() {
            Self::upsert_hot_coin_list(&core_pool, activate).await?;
        }
        if !deactivate_ids.is_empty() {
            CoinRepo::batch_update_default_coin_status(core_pool.clone(), &deactivate_ids, 0)
                .await?;
        }

        Ok(())
    }

    pub async fn get_stable_coin_with_ctx(
        ctx: &'static Context,
        chain_code: ChainCode,
    ) -> Result<String, crate::error::service::ServiceError> {
        let core_pool = ctx.core_pool()?;
        let chain_code_str = chain_code.to_string();
        let usdt_coins = CoinRepo::coin_list_v2(
            core_pool,
            Some("USDT".to_string()),
            Some(chain_code_str.clone()),
        )
        .await?;
        if let Some(token) = usdt_coins.into_iter().find_map(|coin| match coin.token_address {
            wallet_database::entities::asset_token_key::AssetTokenKey::Contract(token) => {
                Some(token)
            }
            wallet_database::entities::asset_token_key::AssetTokenKey::Native => None,
        }) {
            return Ok(token);
        }

        let network_kind =
            match crate::domain::chain::ChainDomain::network_kind_by_chain_code_with_ctx(
                ctx,
                &chain_code_str,
            )
            .await
            {
                Ok(kind) => kind,
                Err(err) => {
                    tracing::warn!(chain_code = %chain_code_str, error = ?err, "failed to resolve chain network by node, fallback to mainnet stable coin");
                    NetworkKind::Mainnet
                }
            };
        match chain_code {
            ChainCode::Ethereum => Ok(chain_stable_coin::ETHEREUM.to_string()),
            ChainCode::BnbSmartChain => Ok(chain_stable_coin::BNB_SMART_CHAIN.to_string()),
            ChainCode::Tron => Ok(match network_kind {
                NetworkKind::Mainnet => chain_stable_coin::TRON_MAINNET,
                NetworkKind::Testnet | NetworkKind::Regtest => chain_stable_coin::TRON_TESTNET,
            }
            .to_string()),
            ChainCode::Solana => Ok(chain_stable_coin::SOLANA.to_string()),
            _ => Err(crate::error::business::BusinessError::Coin(
                crate::error::business::coin::CoinError::NotFound(chain_code.to_string()),
            ))?,
        }
    }

    pub async fn fetch_all_coin_with_ctx(
        ctx: &'static Context,
        pool: &CoreDbPool,
    ) -> Result<Vec<CoinInfo>, crate::error::service::ServiceError> {
        Ok(catalog_sync::fetch(ctx, pool).await?.coins)
    }

    pub(crate) async fn query_token_price_with_ctx(
        ctx: &'static Context,
        req: &TokenQueryPriceReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        let backend_api = ctx.get_global_backend_api();
        let pool = ctx.core_pool()?;
        let tokens = backend_api.token_query_price(req).await?.list;
        for token in tokens {
            let coin_id = CoinId {
                chain_code: token.chain_code.clone(),
                symbol: token.symbol.clone(),
                token_address: token.token_address.clone().into(),
            };
            let status = token.get_status();
            let time = None;
            CoinRepo::update_price_unit(
                pool.clone(),
                &coin_id,
                &token.price.to_string(),
                Some(token.unit),
                status,
                token.swappable,
                time,
                None,
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod profile_tests {
    use super::CoinDomain;

    #[test]
    fn only_dev_restores_local_defaults_after_backend_coin_sync() {
        assert!(CoinDomain::should_restore_local_defaults_after_backend("dev"));
        assert!(!CoinDomain::should_restore_local_defaults_after_backend("test"));
        assert!(!CoinDomain::should_restore_local_defaults_after_backend("prod"));
    }
}

impl From<crate::default_data::coin::DefaultCoin> for CoinData {
    fn from(coin: crate::default_data::coin::DefaultCoin) -> Self {
        // 默认的代币:默认值支持兑换的
        Self {
            name: Some(coin.name),
            chain_code: coin.chain_code,
            symbol: coin.symbol,
            token_address: coin.token_address,
            decimals: coin.decimals,
            protocol: coin.protocol,
            is_default: if coin.default { 1 } else { 0 },
            is_popular: if coin.popular { 1 } else { 0 },
            is_custom: 0,
            price: None,
            status: if coin.active { 1 } else { 0 },
            swappable: true,
            created_at: DateTime::<Utc>::default(),
            updated_at: DateTime::<Utc>::default(),
        }
    }
}

pub fn coin_info_to_coin_data(coin: CoinInfo) -> CoinData {
    CoinData {
        chain_code: coin.chain_code.unwrap_or_default(),
        symbol: coin.symbol.unwrap_or_default(),
        name: coin.name,
        token_address: coin.token_address.into(),
        decimals: coin.decimals.unwrap_or_default(),
        protocol: coin.protocol,
        is_default: if coin.default_token { 1 } else { 0 },
        is_popular: if coin.popular_token { 1 } else { 0 },
        is_custom: 0,
        price: Some(coin.price.unwrap_or_default().to_string()),
        status: if coin.enable { 1 } else { 0 },
        swappable: coin.swappable,
        created_at: parse_utc_datetime(&coin.create_time),
        updated_at: parse_utc_datetime(&coin.update_time),
    }
}
