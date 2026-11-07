//! API dev application policy over shared metadata. Excluded from prod/test-only builds.
use crate::{context::Context, error::service::ServiceError};
use wallet_database::repositories::{
    api_wallet::{chain::ApiChainRepo, coin::ApiCoinRepo},
    node::NodeRepo,
};

pub(super) fn contains(chain_code: &str) -> bool {
    crate::default_data::chain::is_dev_evm_chain(chain_code)
}

pub(super) fn chain_codes() -> Vec<String> {
    crate::default_data::chain::DEV_EVM_CHAIN_CODES.into_iter().map(str::to_owned).collect()
}

pub(super) async fn sync_native_coins(ctx: &Context) -> Result<(), ServiceError> {
    let api = ctx.api_wallet_pool()?;
    let core = ctx.core_pool()?;
    for chain in crate::default_data::chain::testnet_default_chains_list()?
        .chains
        .values()
        .filter(|chain| contains(&chain.chain_code))
    {
        let Some(bound) = ApiChainRepo::detail_with_node(&core, &api, &chain.chain_code).await?
        else {
            continue;
        };
        let Some(node) = NodeRepo::detail(&core, &bound.node_id).await? else {
            continue;
        };
        if node.status != 1 || !node.network.eq_ignore_ascii_case("mainnet") {
            continue;
        }
        if bound.main_symbol != chain.main_symbol {
            tracing::warn!(chain_code = %chain.chain_code, "API dev native default skipped: configured master token does not match");
            continue;
        }
        let native = crate::default_data::coin::mainnet_default_coins_list()?
            .coins
            .iter()
            .find(|coin| {
                coin.chain_code == chain.chain_code
                    && coin.symbol == chain.main_symbol
                    && coin.token_address.is_native()
                    && coin.active
            })
            .ok_or_else(|| {
                ServiceError::Parameter(format!(
                    "missing mainnet native coin for {}",
                    chain.chain_code
                ))
            })?;
        ApiCoinRepo::insert_missing_native_coin_for_bound_chain(
            &api,
            native.clone().into(),
            &bound.node_id,
        )
        .await?;
    }
    Ok(())
}

/// Only return fallback coins whose current API chain and Core binding remain eligible.
pub(super) async fn eligible_native_coins(
    ctx: &Context,
) -> Result<Vec<wallet_database::entities::api_coin::ApiCoinEntity>, ServiceError> {
    let api = ctx.api_wallet_pool()?;
    let core = ctx.core_pool()?;
    let mut eligible = Vec::new();
    for chain in crate::default_data::chain::testnet_default_chains_list()?
        .chains
        .values()
        .filter(|chain| contains(&chain.chain_code))
    {
        let Some(bound) = ApiChainRepo::detail_with_node(&core, &api, &chain.chain_code).await?
        else {
            continue;
        };
        let Some(node) = NodeRepo::detail(&core, &bound.node_id).await? else {
            continue;
        };
        if node.status == 1
            && node.network.eq_ignore_ascii_case("mainnet")
            && bound.main_symbol == chain.main_symbol
        {
            eligible.push((chain.chain_code.clone(), chain.main_symbol.clone()));
        }
    }
    Ok(ApiCoinRepo::coin_list(&api)
        .await?
        .into_iter()
        .filter(|coin| {
            coin.status == 1
                && coin.token_address.is_native()
                && eligible
                    .iter()
                    .any(|(code, symbol)| coin.chain_code == *code && coin.symbol == *symbol)
        })
        .collect())
}
