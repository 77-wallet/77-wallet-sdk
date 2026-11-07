use crate::{
    config::runtime_defaults,
    context::Context,
    domain::{
        api_wallet::{
            account::ApiAccountDomain, adapter_factory::ApiChainAdapterFactory,
            assets::ApiAssetsDomain, coin::ApiCoinDomain,
        },
        app::config::ConfigDomain,
    },
    infrastructure::api_wallet_asset_query_guard,
    response_vo::{
        api_wallet::assets::{ApiAccountChainAsset, ApiAccountChainAssetList},
        standard_wallet::{
            account::{Balance, BalanceInfo},
            assets::{CoinAssets, GetAccountAssetsRes},
            chain::ChainList,
            coin::TokenCurrencyId,
        },
    },
};
use rust_decimal::prelude::Zero;
use std::collections::HashMap;
use wallet_database::{
    entities::{api_assets::ApiCreateAssetsVo, asset_token_key::AssetTokenKey, assets::AssetsId},
    repositories::{
        api_wallet::{
            account::ApiAccountRepo, assets::ApiAssetsRepo, chain::ApiChainRepo, coin::ApiCoinRepo,
        },
        exchange_rate::ExchangeRateRepo,
    },
};
use wallet_utils::unit;

#[derive(Debug, Clone)]
pub struct AddressChainCode {
    pub address: String,
    pub chain_code: String,
}

pub struct ApiAssetsService {
    ctx: &'static Context,
}

impl ApiAssetsService {
    pub fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub async fn add_assets(
        &self,
        req: crate::request::coin::AddCoinReq,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        // 钱包下的账号
        let accounts = ApiAccountRepo::list_by_wallet_address(
            &pool,
            &req.wallet_address,
            Some(req.account_id),
            None,
        )
        .await?;

        let coins = ApiCoinRepo::coin_list_by_chain_token_map_batch(&pool, &req.chain_list).await?;
        let mut create_assets = Vec::new();
        for coin in coins {
            if let Some(account) =
                accounts.iter().find(|account| account.chain_code == coin.chain_code)
            {
                let chain_code = account.chain_code.as_str();

                let assets_id =
                    AssetsId::new(&account.address, chain_code, coin.token_address.clone());

                let assets = ApiCreateAssetsVo::new(
                    assets_id,
                    &coin.symbol,
                    coin.decimals,
                    coin.protocol.clone(),
                    0,
                )
                .with_name(&coin.name);
                create_assets.push(assets);
            };
        }
        ApiAssetsRepo::upsert_assets_multi(&pool, create_assets).await?;

        Ok(())
    }

    pub async fn remove_assets(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_list: ChainList,
        _is_multisig: Option<bool>,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let accounts =
            ApiAccountRepo::list_by_wallet_address(&pool, wallet_address, account_id, None).await?;

        for (chain_code, token_address) in chain_list.iter() {
            // 找到对应链的地址
            let account = accounts.iter().find(|account| account.chain_code == *chain_code);

            if let Some(account) = account {
                ApiAssetsRepo::delete_assets(
                    &pool,
                    &account.address,
                    chain_code,
                    AssetTokenKey::from_raw(Some(token_address)),
                )
                .await?;
            };
        }

        Ok(())
    }

    pub async fn sync_api_assets_by_wallet(
        &self,
        wallet_address: String,
        account_id: Option<u32>,
        _symbol: Vec<String>,
    ) -> Result<(), crate::error::service::ServiceError> {
        ApiAssetsDomain::sync_assets_by_wallet_with_ctx(
            self.ctx,
            wallet_address,
            account_id,
            _symbol,
        )
        .await
    }

    pub async fn chain_balance(
        &self,
        address: &str,
        chain_code: &str,
        token_address: &str,
    ) -> Result<Balance, crate::error::service::ServiceError> {
        let adapter =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, chain_code).await?;

        let pool = self.ctx.api_wallet_pool()?;
        let api_coins = ApiCoinRepo::coin_list(&pool).await?;
        let data = wallet_utils::serde_func::serde_to_string(&api_coins)?;
        tracing::info!("有这些币： {:?}", data);
        let token_key = AssetTokenKey::from_raw(Some(token_address));
        let coin =
            ApiCoinRepo::coin_by_chain_token_key(chain_code, token_key.clone(), &pool).await?;
        let data = wallet_utils::serde_func::serde_to_string(&coin)?;
        tracing::info!("查询到这个币： {:?}", data);

        let balance = adapter.balance_token_key(address, token_key).await?;
        let format_balance = unit::format_to_string(balance, coin.decimals)?;

        let balance = Balance {
            balance: format_balance.clone(),
            decimals: coin.decimals,
            original_balance: balance.to_string(),
        };

        // 更新本地余额
        ApiAssetsDomain::update_balance(
            self.ctx,
            address,
            chain_code,
            coin.token_address.clone(),
            &format_balance,
        )
        .await?;

        Ok(balance)
    }

    pub async fn get_api_wallet_assets_v2(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        // let balance_info =
        //     ApiAssetsDomain::get_api_wallet_assets(wallet_address, account_id, chain_code).await?;
        let balance_info = ApiAssetsDomain::get_api_wallet_assets_v2(
            self.ctx,
            wallet_address,
            account_id,
            chain_code,
        )
        .await?;

        Ok(balance_info)
    }

    pub async fn get_api_wallet_assets(
        &self,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        // Service 层仅做 API 入口封装，聚合策略逻辑统一下沉到 domain。
        ApiAssetsDomain::get_api_wallet_assets(self.ctx, wallet_address, account_id, chain_code)
            .await
    }

    pub async fn get_api_wallet_assets_v3(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
        let balance_info = ApiAssetsDomain::get_api_wallet_assets_v3(
            self.ctx,
            Some(wallet_address),
            account_id,
            chain_code,
        )
        .await?;

        Ok(balance_info)
    }

    // pub async fn get_api_wallet_assets(
    //     &self,
    //     wallet_address: &str,
    // ) -> Result<BalanceInfo, crate::error::service::ServiceError> {
    //     ApiAssetsDomain::get_api_wallet_assets(wallet_address).await
    // }

    // pub async fn get_all_account_assets(
    //     &mut self,
    //     account_id: u32,
    //     wallet_address: Option<&str>,
    // ) -> Result<GetAccountAssetsRes, crate::error::service::ServiceError> {
    //     let accounts = ApiAccountRepo::get_account_list_by_wallet_address_and_account_id(wallet_address, Some(account_id))
    //         .await?;
    //     let token_currencies = CoinDomain::get_token_currencies_v2().await?;

    //     let addresses = accounts.into_iter().map(|info| info.address).collect();

    //     let mut data = tx.get_coin_assets_in_address(addresses).await?;

    //     let account_total_assets =
    //         token_currencies.calculate_account_total_assets(&mut data).await?;
    //     Ok(GetAccountAssetsRes { account_total_assets })
    // }

    pub async fn get_api_assets_list(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> Result<ApiAccountChainAssetList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let chain_codes = chain_code.clone().map(|c| vec![c]).unwrap_or_default();
        let account_addresses = ApiAccountDomain::get_addresses_with_ctx(
            self.ctx,
            wallet_address,
            account_id,
            chain_codes,
        )
        .await?;

        let mut res = ApiAccountChainAssetList::default();
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;

        // 根据账户地址、网络查询币资产
        for address in account_addresses {
            let assets_list = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
                &pool,
                vec![address.address],
                Some(address.chain_code),
                None,
                None,
            )
            .await?;
            for assets in assets_list {
                let coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
                    self.ctx,
                    &assets.chain_code,
                    assets.token_key(),
                )
                .await?;

                if let Some(existing_asset) = res
                    .iter_mut()
                    .find(|a| a.symbol == assets.symbol && a.is_default && coin.is_default == 1)
                {
                    token_currencies.calculate_api_assets(assets, existing_asset, self.ctx).await?;
                    existing_asset
                        .chain_list
                        .entry(coin.chain_code.clone())
                        .or_insert(coin.token_address.as_db_str().to_string());
                } else {
                    let balance =
                        token_currencies.calculate_api_assets_entity(&assets, self.ctx).await?;
                    if balance.amount.is_zero() {
                        continue;
                    }
                    let chain_code = if chain_code.is_none()
                        && let Some(chain) =
                            ApiChainRepo::detail_with_main_symbol(&pool, &assets.symbol).await?
                    {
                        chain.chain_code.clone()
                    } else {
                        assets.chain_code
                    };

                    res.push(ApiAccountChainAsset {
                        chain_code: chain_code.clone(),
                        symbol: assets.symbol,
                        name: assets.name,
                        chain_list: ChainList(HashMap::from([(
                            chain_code,
                            assets.token_address.as_db_str().to_string(),
                        )])),
                        balance,
                        is_multisig: assets.is_multisig, // chains: vec![chain_assets],
                        is_default: coin.is_default == 1,
                    })
                }
            }
        }

        // 过滤掉multisig的资产
        if let Some(is_multisig) = is_multisig {
            res.retain(|asset| {
                if is_multisig {
                    asset.is_multisig == 1
                } else {
                    asset.is_multisig == 0 || asset.is_multisig == 2
                }
            });
        }
        // res.mark_multichain_assets();
        res.sort_account_chain_assets();
        Ok(res)
    }

    // 已添加的资产
    pub async fn get_added_coin_list(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        _is_multisig: Option<bool>,
    ) -> Result<
        crate::response_vo::standard_wallet::coin::CoinInfoList,
        crate::error::service::ServiceError,
    > {
        let pool = self.ctx.api_wallet_pool()?;

        let chain_codes = chain_code.clone().map(|c| vec![c]).unwrap_or_default();
        let account_addresses = ApiAccountDomain::get_addresses_with_ctx(
            self.ctx,
            wallet_address,
            account_id,
            chain_codes,
        )
        .await?;

        let address = account_addresses.into_iter().map(|a| a.address).collect::<Vec<_>>();

        let assets = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
            &pool, address, None, None, None,
        )
        .await?;

        let show_contract = keyword.is_some();
        let mut res = crate::response_vo::standard_wallet::coin::CoinInfoList::default();
        for assets in assets {
            let coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
                self.ctx,
                &assets.chain_code,
                assets.token_key(),
            )
            .await?;
            if let Some(info) =
                res.iter_mut().find(|info| info.symbol == assets.symbol && coin.is_default == 1)
            {
                info.chain_list
                    .entry(assets.chain_code.clone())
                    .or_insert(assets.token_address.as_db_str().to_string());
            } else {
                res.push(crate::response_vo::standard_wallet::coin::CoinInfo {
                    symbol: assets.symbol,
                    name: Some(assets.name),

                    chain_list: ChainList(HashMap::from([(
                        assets.chain_code.clone(),
                        assets.token_address.as_db_str().to_string(),
                    )])),
                    is_default: coin.is_default == 1,
                    hot_coin: coin.status == 1,
                    show_contract,
                });
            }
        }

        Ok(res)
    }

    // 单个索引下的所有资产总和
    pub async fn get_account_assets(
        &self,
        account_id: u32,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> Result<GetAccountAssetsRes, crate::error::service::ServiceError> {
        let cache_key = api_wallet_asset_query_guard::account_assets_key(
            wallet_address,
            account_id,
            chain_code.as_deref(),
        );
        let fresh_ttl = runtime_defaults::api_assets().total_cache_ttl;

        api_wallet_asset_query_guard::load_account_assets(&cache_key, fresh_ttl, || async move {
            self.query_account_assets_uncached(account_id, wallet_address, chain_code).await
        })
        .await
    }

    async fn query_account_assets_uncached(
        &self,
        account_id: u32,
        wallet_address: &str,
        chain_code: Option<String>,
    ) -> Result<GetAccountAssetsRes, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let account = ApiAccountRepo::list_by_wallet_address(
            &pool,
            wallet_address,
            Some(account_id),
            chain_code.as_deref(),
        )
        .await?;
        let address = account.iter().map(|a| a.address.clone()).collect::<Vec<_>>();

        let mut assets = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
            &pool, address, chain_code, None, None,
        )
        .await?;

        // 币符号
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;

        let mut account_total_assets = Some(wallet_types::Decimal::default());
        let mut amount = wallet_types::Decimal::default();

        let currency = ConfigDomain::get_currency(self.ctx).await?;

        for assets in assets.iter_mut() {
            let token_currency_id = TokenCurrencyId::new(
                &assets.symbol,
                &assets.chain_code,
                assets.token_key().to_option_string_for_api(),
            );

            let value = if let Some(token_currency) = token_currencies.get(&token_currency_id) {
                // if assets.address == "TAcyQRGXhmSRGYn8q9UHQr6VFyQcgKPvc5"
                //     && assets.chain_code == "tron"
                //     && assets.token_address == ""
                // {
                //     tracing::info!("get_account_assets token_currency{:?}", token_currency);
                // }
                let balance = wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
                let price = token_currency.get_price(&currency);
                let price = wallet_types::Decimal::from_f64_retain(price).unwrap_or_default();
                Some(price * balance)
            } else {
                None
            };

            amount += wallet_utils::parse_func::decimal_from_str(&assets.balance)?;
            account_total_assets =
                account_total_assets.map(|total| total + value.unwrap_or_default());
        }

        let bal = BalanceInfo {
            amount: wallet_utils::conversion::decimal_to_f64(&amount)?,
            currency: currency.to_string(),
            unit_price: Default::default(),
            fiat_value: account_total_assets
                .map(|total| wallet_utils::conversion::decimal_to_f64(&total))
                .transpose()?,
        };

        Ok(GetAccountAssetsRes { account_total_assets: bal })
    }

    // 资产列表
    pub async fn get_account_chain_assets(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        _is_multisig: Option<bool>,
        hide_zero_balance: bool,
    ) -> Result<ApiAccountChainAssetList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let accounts = ApiAccountRepo::list_by_wallet_address(
            &pool,
            wallet_address,
            account_id,
            chain_code.as_deref(),
        )
        .await?;

        let mut res = ApiAccountChainAssetList::default();
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;

        // 根据账户地址、网络查询币资产
        for address in accounts {
            let assets_list = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
                &pool,
                vec![address.address],
                Some(address.chain_code),
                None,
                None,
            )
            .await?;
            for assets in assets_list {
                if hide_zero_balance && assets.balance == "0" {
                    continue;
                }

                let coin = ApiCoinDomain::get_coin_by_token_key_exact_with_ctx(
                    self.ctx,
                    &assets.chain_code,
                    assets.token_key(),
                )
                .await?;
                tracing::info!(
                    "get_account_chain_assets----get_coin--coin: {:?}, {:?} ,{:?},{:?}",
                    coin,
                    assets.chain_code,
                    assets.symbol,
                    assets.token_key().to_option_string_for_api()
                );
                if let Some(existing_asset) = res
                    .iter_mut()
                    .find(|a| a.symbol == assets.symbol && a.is_default && coin.is_default == 1)
                {
                    token_currencies.calculate_api_assets(assets, existing_asset, self.ctx).await?;
                    existing_asset
                        .chain_list
                        .entry(coin.chain_code.clone())
                        .or_insert(coin.token_address.as_db_str().to_string());
                } else {
                    let balance =
                        token_currencies.calculate_api_assets_entity(&assets, self.ctx).await?;

                    res.push(ApiAccountChainAsset {
                        chain_code: assets.chain_code.clone(),
                        symbol: assets.symbol,
                        name: assets.name,
                        chain_list: ChainList(HashMap::from([(
                            assets.chain_code,
                            assets.token_address.as_db_str().to_string(),
                        )])),
                        balance,
                        is_multisig: assets.is_multisig,
                        is_default: coin.is_default == 1,
                    })
                }
            }
        }

        res.sort_account_chain_assets();
        tracing::info!("get_account_chain_assets: {res:?}");
        Ok(res)
    }

    pub async fn get_account_chain_assets_v2(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        _is_multisig: Option<bool>,
        hide_zero_balance: bool,
    ) -> Result<ApiAccountChainAssetList, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let core_pool = self.ctx.core_pool()?;

        let account_assert = ApiAssetsRepo::get_api_wallet_assets_v2(
            &pool,
            wallet_address,
            account_id,
            chain_code.as_deref(),
            hide_zero_balance,
        )
        .await?;

        let currency = ConfigDomain::get_currency(self.ctx).await?;
        let exchange_rate =
            ExchangeRateRepo::get_by_target_currency_or_default(core_pool, &currency).await?;
        let cal_exchange_rate = |value: f64| {
            if exchange_rate.target_currency.to_uppercase() == "USD" {
                value
            } else {
                value * exchange_rate.rate
            }
        };

        let mut result: Vec<_> = vec![];
        for acc in account_assert {
            result.push(ApiAccountChainAsset {
                chain_list: ChainList(acc.get_chain_token_map()?),
                chain_code: acc.chain_code,
                name: acc.api_assets_name,
                balance: BalanceInfo {
                    amount: acc.total_coins_quantity,
                    currency: exchange_rate.target_currency.clone(),
                    unit_price: acc.coin_unit_price.map(cal_exchange_rate),
                    fiat_value: acc.total_account_amount.map(cal_exchange_rate),
                },
                is_multisig: acc.assets_is_multisig,
                symbol: acc.symbol,
                is_default: acc.coin_is_default,
            })
        }

        Ok(ApiAccountChainAssetList(result))
    }

    pub async fn detail(
        &mut self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<CoinAssets, crate::error::service::ServiceError> {
        let token_address = token_key.to_option_string_for_api();
        let cache_key = api_wallet_asset_query_guard::asset_detail_key(
            address,
            account_id,
            chain_code,
            token_address.as_deref(),
        );
        let fresh_ttl = runtime_defaults::api_assets().total_cache_ttl;

        api_wallet_asset_query_guard::load_asset_detail(&cache_key, fresh_ttl, || async move {
            self.query_detail_uncached(address, account_id, chain_code, token_key).await
        })
        .await
    }

    async fn query_detail_uncached(
        &mut self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<CoinAssets, crate::error::service::ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let token_currencies = ApiCoinDomain::get_api_token_currencies(self.ctx).await?;
        let address = if let Some(account_id) = account_id {
            let account = ApiAccountRepo::find_one_by_wallet_address_account_id_chain_code(
                &pool, address, account_id, chain_code,
            )
            .await?
            .ok_or(crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::wallet::WalletError::NotFound.into(),
            ))?;
            account.address
        } else {
            address.to_string()
        };
        let assets_id = AssetsId::new(&address, chain_code, token_key);
        let assets = ApiAssetsRepo::find_by_id(&pool, &assets_id).await?.ok_or(
            crate::error::business::BusinessError::Assets(
                crate::error::business::assets::AssetsError::NotFound,
            ),
        )?;

        let balance = token_currencies.calculate_api_assets_entity(&assets, self.ctx).await?;
        let data: CoinAssets = (balance, assets).into();
        tracing::info!("[api assets detail] data: {data:?}");
        Ok(data)
    }
}
