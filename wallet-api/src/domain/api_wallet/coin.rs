use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Utc};
use wallet_database::{
    entities::{
        api_account::ApiAccountEntity,
        api_assets::ApiCreateAssetsVo,
        api_coin::{ApiCoinData, ApiCoinEntity},
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
    },
    repositories::{
        api_wallet::{account::ApiAccountRepo, assets::ApiAssetsRepo, coin::ApiCoinRepo},
        exchange_rate::ExchangeRateRepo,
    },
};
use wallet_transport_backend::response_vo::{api_wallet::coin::ApiCoinInfo, coin::TokenCurrency};

use crate::{
    context::Context,
    domain::app::config::ConfigDomain,
    response_vo::standard_wallet::{
        chain::ChainList,
        coin::{CoinInfoList, TokenCurrencies, TokenCurrencyId},
    },
};

impl From<crate::default_data::coin::DefaultCoin> for ApiCoinData {
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
            price: Some("0".to_string()),
            status: if coin.active { 1 } else { 0 },
            created_at: DateTime::<Utc>::default(),
            updated_at: Some(DateTime::<Utc>::default()),
        }
    }
}
pub struct ApiCoinDomain {}

static BACKFILL_RUNNING: AtomicBool = AtomicBool::new(false);

impl ApiCoinDomain {
    #[deprecated(note = "use get_coin_by_token_key_exact_with_ctx")]
    pub async fn get_coin_by_token_key_exact(
        ctx: &'static Context,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<ApiCoinEntity, crate::error::service::ServiceError> {
        Self::get_coin_by_token_key_exact_with_ctx(ctx, chain_code, token_key).await
    }

    fn active_chain_codes(coins: &[ApiCoinEntity]) -> HashSet<String> {
        coins.iter().filter(|coin| coin.status == 1).map(|coin| coin.chain_code.clone()).collect()
    }

    fn active_coins_by_chain(coins: &[ApiCoinEntity]) -> HashMap<String, Vec<ApiCoinEntity>> {
        let mut grouped: HashMap<String, Vec<ApiCoinEntity>> = HashMap::new();
        for coin in coins.iter().filter(|coin| coin.status == 1) {
            grouped.entry(coin.chain_code.clone()).or_default().push(coin.clone());
        }
        grouped
    }

    pub(crate) async fn upsert_hot_coin_list(
        ctx: &Context,
        coins: Vec<ApiCoinData>,
    ) -> Result<Vec<ApiCoinEntity>, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
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

        let res = ApiCoinRepo::upsert_multi_coin(&pool, coin_data).await?;
        Ok(res)
    }

    pub(crate) async fn sync_dev_native_coins(
        _ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        #[cfg(all(feature = "dev", not(feature = "prod")))]
        super::dev_defaults::sync_native_coins(_ctx).await?;
        Ok(())
    }

    pub async fn pull_api_coins(
        ctx: &Context,
    ) -> Result<Vec<ApiCoinEntity>, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        // 删除掉无效的token
        ApiCoinRepo::drop_coin_just_null_token_address(&pool).await?;

        // 拉所有的币
        let coins = ApiCoinDomain::fetch_all_coin(ctx).await?;
        let data =
            coins.into_iter().map(|d| coin_info_to_coin_data(d)).collect::<Vec<ApiCoinData>>();

        let res = ApiCoinDomain::upsert_hot_coin_list(ctx, data).await?;
        #[cfg(all(feature = "dev", not(feature = "prod")))]
        let mut res = res;
        #[cfg(all(feature = "dev", not(feature = "prod")))]
        {
            // Only after a valid backend response; transport/protocol errors still propagate.
            super::dev_defaults::sync_native_coins(ctx).await?;
            for coin in super::dev_defaults::eligible_native_coins(ctx).await? {
                if !res.iter().any(|row| {
                    row.chain_code == coin.chain_code && row.token_address == coin.token_address
                }) {
                    res.push(coin);
                }
            }
        }

        Ok(res)
    }

    /// 查询代币汇率
    pub async fn get_api_token_currencies(
        ctx: &Context,
    ) -> Result<TokenCurrencies, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let core_pool = ctx.core_pool()?;
        let currency = ConfigDomain::get_currency(ctx).await?;

        let coins = ApiCoinRepo::coin_list_v2(&pool, None, None).await?;

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
        coins: Vec<ApiCoinEntity>,
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

    pub async fn fetch_all_coin(
        ctx: &Context,
    ) -> Result<Vec<ApiCoinInfo>, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        // 本地没有币拉服务端所有的币,有拉去创建时间后的币种
        let backend_api = ctx.get_global_backend_api();
        let mut coins = Vec::new();

        // TODO 1.5 版本验证币数量如果大于500说明已经同步过最新的币了,拉最新的。
        // let create_at = None;
        let count = ApiCoinRepo::coin_count(&pool).await?;
        let create_at = if count > 500 {
            if let Some(last_coin) = ApiCoinRepo::last_coin(&pool, true).await? {
                let formatted = last_coin.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                Some(formatted)
            } else {
                None
            }
        } else {
            None
        };

        coins.append(&mut backend_api.fetch_all_api_tokens(create_at.clone(), None).await?);

        Ok(coins)
    }

    pub async fn init_token_price(
        ctx: &Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let backend_api = ctx.get_global_backend_api();

        let update_at = if let Some(last_coin) = ApiCoinRepo::last_coin(&pool, false).await? {
            last_coin.updated_at.map(|s| s.format("%Y-%m-%d %H:%M:%S").to_string())
        } else {
            None
        };

        let coins = backend_api.fetch_all_api_tokens(None, update_at).await?;
        Self::apply_token_updates(&pool, coins).await
    }

    async fn apply_token_updates(
        pool: &wallet_database::ApiWalletDbPool,
        coins: Vec<ApiCoinInfo>,
    ) -> Result<(), crate::error::service::ServiceError> {
        for token in coins {
            let status = token.get_status();
            let time =
                // crate::infrastructure::parse_utc_with_error(&token.update_time.to_string()).ok();
                token.update_time;
            let symbol = token.symbol.clone();
            let name = token.name;

            let coin_id = wallet_database::entities::coin::CoinId {
                chain_code: token.chain_code.unwrap_or_default(),
                symbol: token.symbol.unwrap_or_default(),
                token_address: token.token_address.clone().into(),
            };

            ApiCoinRepo::update_price_unit(
                &coin_id,
                &token.price.unwrap_or_default().to_string(),
                token.decimals,
                status,
                time,
                symbol,
                name,
                pool,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn add_supported_coin(
        ctx: &Context,
        coins: Vec<ApiCoinEntity>,
    ) -> Result<(), crate::error::service::ServiceError> {
        if BACKFILL_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::info!(
                "ApiCoinDomain::add_supported_coin backfill already running, skip duplicate schedule"
            );
            return Ok(());
        }

        let pool = ctx.api_wallet_pool()?;
        let wallets =
            wallet_database::repositories::api_wallet::wallet::ApiWalletRepo::list(&pool, None)
                .await?;
        let active_coins: Vec<ApiCoinEntity> =
            coins.into_iter().filter(|coin| coin.status == 1).collect();
        let active_chain_codes = Self::active_chain_codes(&active_coins);
        let active_coins_by_chain = Self::active_coins_by_chain(&active_coins);
        tracing::debug!(
            coin_count = active_coins.len(),
            wallet_count = wallets.len(),
            chain_count = active_chain_codes.len(),
            "ApiCoinDomain::add_supported_coin -> schedule paged wallet/chain account scan"
        );
        let background_task_pool = ctx.get_global_background_task_pool();
        let active_chain_codes: Vec<String> = active_chain_codes.into_iter().collect();
        struct BackfillRunningGuard;
        impl Drop for BackfillRunningGuard {
            fn drop(&mut self) {
                BACKFILL_RUNNING.store(false, Ordering::SeqCst);
            }
        }

        background_task_pool
            .push(async move {
                let _guard = BackfillRunningGuard;
                const PAGE_SIZE: i64 = 1000;
                let mut scanned_accounts = 0usize;
                let mut created_assets = 0usize;
                tracing::debug!(
                    wallet_count = wallets.len(),
                    chain_count = active_chain_codes.len(),
                    page_size = PAGE_SIZE,
                    "ApiCoinDomain::add_supported_coin background scan start"
                );

                for wallet in &wallets {
                    for chain_code in &active_chain_codes {
                        let Some(coins_for_chain) = active_coins_by_chain.get(chain_code) else {
                            continue;
                        };

                        let mut page = 0i64;
                        loop {
                            let account_summers = ApiAccountRepo::lists_acc_by_wallet_address_v3(
                                &pool,
                                &wallet.address,
                                None,
                                Some(chain_code.clone()),
                                page,
                                PAGE_SIZE,
                            )
                            .await?;
                            if account_summers.is_empty() {
                                break;
                            }

                            let mut accounts = Vec::new();
                            for item in &account_summers {
                                let mut rows = ApiAccountRepo::find_all_by_wallet_address_index(
                                    &pool,
                                    &wallet.address,
                                    chain_code,
                                    item.account_id,
                                )
                                .await?;
                                accounts.append(&mut rows);
                            }
                            accounts.sort_by(|a, b| {
                                a.account_id.cmp(&b.account_id).then(a.address.cmp(&b.address))
                            });
                            accounts.dedup_by(|a, b| {
                                a.account_id == b.account_id
                                    && a.address == b.address
                                    && a.chain_code == b.chain_code
                            });

                            scanned_accounts += accounts.len();
                            let existing_assets = ApiAssetsRepo::get_api_assets_by_address(
                                &pool,
                                accounts.iter().map(|a| a.address.clone()).collect(),
                                None,
                            )
                            .await?;
                            let existing_keys = existing_assets
                                .into_iter()
                                .map(|asset| {
                                    (
                                        asset.address,
                                        asset.chain_code,
                                        asset.token_address.as_db_str().to_string(),
                                    )
                                })
                                .collect::<std::collections::HashSet<_>>();

                            let (create_assets, _) = build_supported_coin_assets_with_existing(
                                &accounts,
                                coins_for_chain,
                                &existing_keys,
                            )?;
                            if !create_assets.is_empty() {
                                created_assets += create_assets.len();
                                ApiAssetsRepo::upsert_assets_multi(&pool, create_assets).await?;
                            }

                            tracing::debug!(
                                wallet_address = %wallet.address,
                                chain_code = %chain_code,
                                page,
                                batch_accounts = account_summers.len(),
                                scanned_accounts,
                                created_assets,
                                "ApiCoinDomain::add_supported_coin background chain finished"
                            );

                            if account_summers.len() < PAGE_SIZE as usize {
                                break;
                            }
                            page += 1;
                        }
                    }
                    tokio::task::yield_now().await;
                }

                tracing::debug!(
                    wallet_count = wallets.len(),
                    scanned_accounts,
                    created_assets,
                    "ApiCoinDomain::add_supported_coin background scan finished"
                );
                Ok(())
            })
            .await;

        Ok(())
    }

    pub async fn get_coin_by_token_key_exact_with_ctx(
        ctx: &Context,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<ApiCoinEntity, crate::error::service::ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let coin = ApiCoinRepo::coin_by_chain_token_key(chain_code, token_key, &pool).await?;

        Ok(coin)
    }
}

fn build_supported_coin_assets(
    accounts: &[ApiAccountEntity],
    coins_for_chain: &[ApiCoinEntity],
) -> Result<(Vec<ApiCreateAssetsVo>, usize), crate::error::service::ServiceError> {
    let existing_keys = std::collections::HashSet::<(String, String, String)>::new();
    build_supported_coin_assets_with_existing(accounts, coins_for_chain, &existing_keys)
}

fn build_supported_coin_assets_with_existing(
    accounts: &[ApiAccountEntity],
    coins_for_chain: &[ApiCoinEntity],
    existing_keys: &std::collections::HashSet<(String, String, String)>,
) -> Result<(Vec<ApiCreateAssetsVo>, usize), crate::error::service::ServiceError> {
    let mut known_keys: std::collections::HashSet<_> = existing_keys
        .iter()
        .map(|(address, chain, token)| {
            (
                address.clone(),
                chain.clone(),
                AssetTokenKey::from_raw(Some(token)).normalized_for_chain(chain),
            )
        })
        .collect();
    let mut create_assets = Vec::new();
    for account in accounts {
        for coin in coins_for_chain {
            let token = coin.token_address.normalized_for_chain(&account.chain_code);
            let key = (account.address.clone(), account.chain_code.clone(), token.clone());
            if !known_keys.insert(key) {
                continue;
            }
            let assets_id = AssetsId::new(&account.address, &account.chain_code, token);
            let assets = ApiCreateAssetsVo::new(
                assets_id,
                &coin.symbol,
                coin.decimals,
                coin.protocol.clone(),
                0,
            )
            .with_name(&coin.name)
            .with_u256(alloy::primitives::U256::default(), coin.decimals)?;
            create_assets.push(assets);
        }
    }

    Ok((create_assets, accounts.len()))
}

#[cfg(test)]
mod tests {
    use super::ApiCoinDomain;
    use sqlx::types::chrono::Utc;
    use wallet_database::entities::{
        api_account::ApiAccountEntity, api_coin::ApiCoinEntity, api_wallet::ApiWalletType,
        asset_token_key::AssetTokenKey,
    };

    fn coin(chain_code: &str, status: u8) -> ApiCoinEntity {
        ApiCoinEntity {
            id: 0,
            name: "coin".to_string(),
            chain_code: chain_code.to_string(),
            symbol: "SYM".to_string(),
            token_address: AssetTokenKey::from(String::new()),
            decimals: 18,
            protocol: None,
            is_default: 0,
            is_popular: 0,
            is_custom: 0,
            price: "0".to_string(),
            status,
            created_at: chrono::DateTime::<chrono::Utc>::default(),
            updated_at: Some(chrono::DateTime::<chrono::Utc>::default()),
        }
    }

    fn account(address: &str, chain_code: &str) -> ApiAccountEntity {
        ApiAccountEntity {
            id: 0,
            account_id: 1,
            name: "account".to_string(),
            address: address.to_string(),
            pubkey: None,
            address_type: String::new(),
            wallet_address: "wallet".to_string(),
            uid: "uid".to_string(),
            derivation_path: "m/44'/60'/0'/0/0".to_string(),
            derivation_path_index: 0,
            chain_code: chain_code.to_string(),
            api_wallet_type: ApiWalletType::Withdrawal,
            status: 1,
            is_init: 1,
            is_expand: 0,
            is_used: false,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn evm_token_identity_backfill_reuses_legacy_key_and_deduplicates_batch() {
        let accounts = vec![account("address", "base")];
        let mut mixed = coin("base", 1);
        mixed.token_address = AssetTokenKey::from_raw(Some("0xAbC"));
        let mut lower = mixed.clone();
        lower.token_address = AssetTokenKey::from_raw(Some("0xabc"));
        let existing =
            std::collections::HashSet::from([("address".into(), "base".into(), "0xABC".into())]);
        let (assets, _) = super::build_supported_coin_assets_with_existing(
            &accounts,
            &[mixed.clone(), lower.clone()],
            &existing,
        )
        .unwrap();
        assert!(assets.is_empty());
        let (assets, _) = super::build_supported_coin_assets(&accounts, &[mixed, lower]).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].assets_id.token_address.as_db_str(), "0xabc");
    }

    #[test]
    fn evm_token_identity_backfill_keeps_non_evm_case_distinct() {
        let accounts = vec![account("address", "tron")];
        let mut first = coin("tron", 1);
        first.token_address = AssetTokenKey::from_raw(Some("TAbC"));
        let mut second = first.clone();
        second.token_address = AssetTokenKey::from_raw(Some("Tabc"));
        let (assets, _) = super::build_supported_coin_assets(&accounts, &[first, second]).unwrap();
        assert_eq!(assets.len(), 2);
    }

    #[test]
    fn active_chain_codes_filters_inactive_coins() {
        let coins = vec![coin("eth", 1), coin("tron", 0), coin("bnb", 1), coin("eth", 1)];

        let chains = ApiCoinDomain::active_chain_codes(&coins);

        assert!(chains.contains("eth"));
        assert!(chains.contains("bnb"));
        assert!(!chains.contains("tron"));
        assert_eq!(chains.len(), 2);
    }

    #[test]
    fn supported_coin_backfill_uses_api_account_rows_directly() {
        let accounts = vec![account("0xabc", "bnb"), account("0xdef", "bnb")];
        let coins = vec![coin("bnb", 1)];

        let (assets, account_count) =
            super::build_supported_coin_assets(&accounts, &coins).unwrap();

        assert_eq!(account_count, 2);
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].assets_id.address, "0xabc");
        assert_eq!(assets[1].assets_id.address, "0xdef");
    }
}

pub fn coin_info_to_coin_data(coin: ApiCoinInfo) -> ApiCoinData {
    ApiCoinData {
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
        created_at: coin.create_time,
        updated_at: coin.update_time,
    }
}

#[cfg(test)]
#[path = "coin/metadata_rename_tests.rs"]
mod metadata_rename_tests;
