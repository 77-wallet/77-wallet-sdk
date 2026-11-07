use std::collections::{HashMap, HashSet};

use crate::{
    context::Context,
    domain::api_wallet::{chain::ApiChainDomain, coin::ApiCoinDomain, wallet::ApiWalletDomain},
    error::service::ServiceError,
    response_vo::standard_wallet::{account::BalanceInfo, chain::ChainAssets},
};
use wallet_database::{
    entities::{api_chain::ApiChainEntity, asset_token_key::AssetTokenKey},
    repositories::api_wallet::{
        account::ApiAccountRepo, assets::ApiAssetsRepo, chain::ApiChainRepo,
    },
};

pub(crate) struct ApiChainApplication {
    ctx: &'static Context,
}

#[cfg(test)]
#[path = "api_wallet_chain_visibility_tests.rs"]
mod api_asset_visibility_tests;

impl ApiChainApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn get_chain_assets_list(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_list: HashMap<String, String>,
    ) -> Result<Vec<ChainAssets>, ServiceError> {
        let chain_list = normalize_chain_list(chain_list);
        let pool = self.ctx.api_wallet_pool()?;
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;

        let mut account_addresses = Vec::<String>::new();
        let accounts =
            ApiAccountRepo::list_by_wallet_address_account_id(&pool, Some(address), account_id)
                .await?;
        // This DAO treats an empty address list as unfiltered. Keep an empty
        // wallet/account scope empty instead of querying unrelated holdings.
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        for account in accounts {
            if !account_addresses.iter().any(|address| address == &account.address) {
                account_addresses.push(account.address);
            }
        }

        let datas = ApiAssetsRepo::get_api_assets_by_address(&pool, account_addresses, None)
            .await?
            .into_iter()
            .filter(|data| {
                chain_list
                    .get(&data.chain_code)
                    .is_some_and(|token_address| data.token_address.as_db_str() == token_address)
            })
            .collect();

        let chains = ApiChainRepo::get_chain_list(&pool).await?;

        let res = token_currencies.calculate_api_chain_assets_list(datas, chains, self.ctx).await?;
        tracing::info!("[get_chain_assets_list] res: {res:?}");
        Ok(res)
    }

    pub(crate) async fn get_chain_assets_list_v2(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_list: HashMap<String, String>,
    ) -> Result<Vec<ChainAssets>, ServiceError> {
        let chain_list = normalize_chain_list(chain_list);
        let pool = self.ctx.api_wallet_pool()?;
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;

        let mut account_addresses = HashSet::<String>::new();
        let accounts =
            ApiAccountRepo::list_by_wallet_address_account_id(&pool, Some(address), account_id)
                .await?;
        // This DAO treats an empty address list as unfiltered. Keep an empty
        // wallet/account scope empty instead of querying unrelated holdings.
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        for account in accounts {
            account_addresses.insert(account.address);
        }

        let account_addresses: Vec<_> = account_addresses.into_iter().collect();

        let datas =
            ApiAssetsRepo::get_api_assets_by_address(&pool, account_addresses, None).await?;

        let filtered_datas: Vec<_> = datas
            .into_iter()
            .filter(|data| {
                chain_list
                    .get(&data.chain_code)
                    .is_some_and(|token_address| data.token_address.as_db_str() == token_address)
            })
            .collect();

        let chains = ApiChainRepo::get_chain_list(&pool).await?;

        let calculated = token_currencies
            .calculate_api_chain_assets_list(filtered_datas, chains, self.ctx)
            .await?;

        let mut group_map: HashMap<(String, String), Vec<ChainAssets>> = HashMap::new();
        for asset in calculated {
            let key = (asset.chain_code.clone(), asset.token_address.clone());
            group_map.entry(key).or_default().push(asset);
        }

        let request_tokens: HashSet<(String, String)> =
            chain_list.iter().map(|(chain, token)| (chain.clone(), token.clone())).collect();

        let mut token_meta_map: HashMap<
            (String, String),
            wallet_transport_backend::response_vo::coin::TokenCurrency,
        > = HashMap::new();
        for (id, currency) in token_currencies.iter() {
            let token_address = id.token_address.as_db_str().to_string();
            let key = (id.chain_code.clone(), token_address);
            if request_tokens.contains(&key) {
                token_meta_map.insert(key, currency.clone());
            }
        }

        let mut reqs: Vec<_> = chain_list.iter().map(|(c, t)| (c.clone(), t.clone())).collect();
        reqs.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut res = Vec::with_capacity(reqs.len());
        let currency = crate::domain::app::config::ConfigDomain::get_currency(self.ctx).await?;

        for (chain_code, token_address) in reqs {
            let key = (chain_code.clone(), token_address.clone());

            if let Some(assets) = group_map.remove(&key) {
                res.extend(assets);
            } else {
                let (name, symbol, unit_price) = if let Some(token_meta) = token_meta_map.get(&key)
                {
                    (token_meta.name.clone(), token_meta.code.clone(), token_meta.price)
                } else {
                    ("Unknown Token".to_string(), "UNKNOWN".to_string(), None)
                };

                let balance = BalanceInfo {
                    amount: 0.0,
                    currency: currency.clone(),
                    unit_price,
                    fiat_value: Some(0.0),
                };

                res.push(ChainAssets {
                    chain_code: chain_code.clone(),
                    name,
                    symbol,
                    address: "".to_string(),
                    token_address: token_address.clone(),
                    balance,
                    is_multisig: 0,
                    asset_quantity_ratio: 0.0,
                });
            }
        }

        recompute_asset_ratios(&mut res);

        tracing::info!("[get_chain_assets_list] assets_count={}", res.len());
        Ok(res)
    }

    pub(crate) async fn get_hot_chain_list(&self) -> Result<Vec<ApiChainEntity>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let res = ApiChainRepo::get_chain_list(&pool).await?;

        Ok(res)
    }

    pub(crate) async fn sync_chains(&self) -> Result<Vec<String>, ServiceError> {
        ApiChainDomain::sync_chains(self.ctx).await
    }

    pub(crate) async fn sync_wallet_chain_data(&self) -> Result<(), ServiceError> {
        let unlock_token = ApiWalletDomain::get_wallet_unlock_token().await?;
        ApiChainDomain::sync_wallet_chain_data(self.ctx, &unlock_token).await
    }
}

fn recompute_asset_ratios(assets: &mut [ChainAssets]) {
    let total_fiat: f64 = assets.iter().map(|asset| asset.balance.fiat_value.unwrap_or(0.0)).sum();

    if total_fiat > 0.0 {
        for asset in assets {
            let fiat_value = asset.balance.fiat_value.unwrap_or(0.0);
            asset.asset_quantity_ratio = fiat_value / total_fiat;
        }
    }
}

#[cfg(test)]
mod evm_token_identity_tests {
    use super::*;
    #[test]
    fn evm_token_identity_chain_requests_match_canonical_metadata() {
        let requests = HashMap::from([
            ("base".into(), "0xAbC".into()),
            ("tron".into(), "TAbC".into()),
            ("eth".into(), "".into()),
        ]);
        let normalized = normalize_chain_list(requests);
        assert_eq!(normalized["base"], "0xabc");
        assert_eq!(normalized["tron"], "TAbC");
        assert_eq!(normalized["eth"], "");
    }
}

fn normalize_chain_list(chain_list: HashMap<String, String>) -> HashMap<String, String> {
    chain_list
        .into_iter()
        .map(|(chain, token)| {
            let token = AssetTokenKey::from_raw(Some(&token)).normalized_for_chain(&chain);
            (chain, token.as_db_str().to_string())
        })
        .collect()
}
