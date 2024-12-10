use crate::{
    domain::{
        self, account::AccountDomain, assets::AssetsDomain, coin::CoinDomain, task_queue::Tasks,
    },
    request::assets::GetChain,
    response_vo::assets::{
        AccountChainAsset, AccountChainAssetList, CoinAssets, GetAccountAssetsRes,
        GetChainAssetsRes,
    },
};
use wallet_database::{
    dao::assets::CreateAssetsVo,
    entities::{
        assets::{AssetsEntity, AssetsId},
        coin::SymbolId,
    },
    repositories::{
        account::AccountRepoTrait, assets::AssetsRepoTrait, chain::ChainRepoTrait,
        coin::CoinRepoTrait, ResourcesRepo,
    },
};
use wallet_transport_backend::request::TokenQueryPriceReq;

#[derive(Debug, Clone)]
pub struct AddressChainCode {
    pub address: String,
    pub chain_code: String,
}

pub struct AssetsService {
    pub repo: ResourcesRepo,
    account_domain: AccountDomain,
    assets_domain: AssetsDomain,
    coin_domain: CoinDomain, // keystore: wallet_keystore::Keystore
                             // keystore: wallet_keystore::Keystore
}

impl AssetsService {
    pub fn new(repo: ResourcesRepo) -> Self {
        Self {
            repo,
            account_domain: AccountDomain::new(),
            assets_domain: AssetsDomain::new(),
            coin_domain: CoinDomain::new(),
        }
    }

    pub async fn get_multisig_account_assets(
        &mut self,
        address: &str,
    ) -> Result<GetAccountAssetsRes, crate::ServiceError> {
        let tx = &mut self.repo;
        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;

        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let multisig =
            domain::multisig::MultisigDomain::account_by_address(address, true, &pool).await?;
        let address = vec![multisig.address];

        let mut data = tx.get_coin_assets_in_address(address).await?;
        let account_total_assets = token_currencies
            .calculate_account_total_assets(&mut data)
            .await?;

        Ok(GetAccountAssetsRes {
            account_total_assets,
        })
    }

    pub async fn get_account_assets(
        &mut self,
        account_id: u32,
        wallet_address: &str,
    ) -> Result<GetAccountAssetsRes, crate::ServiceError> {
        let tx = &mut self.repo;
        let mut data = self
            .assets_domain
            .get_account_assets_entity(tx, account_id, wallet_address, Some(false))
            .await?;
        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;

        let account_total_assets = token_currencies
            .calculate_account_total_assets(&mut data)
            .await?;

        Ok(GetAccountAssetsRes {
            account_total_assets,
        })
    }

    pub async fn detail(
        &mut self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        symbol: &str,
    ) -> Result<CoinAssets, crate::ServiceError> {
        let tx = &mut self.repo;

        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;
        let address = if let Some(account_id) = account_id {
            let account = tx
                .detail_by_wallet_address_and_account_id_and_chain_code(
                    address, account_id, chain_code,
                )
                .await?
                .ok_or(crate::BusinessError::Account(crate::AccountError::NotFound))?;
            account.address
        } else {
            address.to_string()
        };
        let assets_id = AssetsId::new(&address, chain_code, symbol);
        let assets = tx
            .assets_by_id(&assets_id)
            .await?
            .ok_or(crate::BusinessError::Assets(crate::AssetsError::NotFound))?;

        let balance = token_currencies.calculate_assets_entity(&assets).await?;
        let data: CoinAssets = (balance, assets).into();

        Ok(data)
    }

    pub async fn get_all_account_assets(
        &mut self,
        account_id: u32,
        wallet_address: Option<&str>,
    ) -> Result<GetAccountAssetsRes, crate::ServiceError> {
        let tx = &mut self.repo;
        let accounts = tx
            .get_account_list_by_wallet_address_and_account_id(wallet_address, Some(account_id))
            .await?;
        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;

        let addresses = accounts.into_iter().map(|info| info.address).collect();

        let mut data = tx.get_coin_assets_in_address(addresses).await?;

        let account_total_assets = token_currencies
            .calculate_account_total_assets(&mut data)
            .await?;
        Ok(GetAccountAssetsRes {
            account_total_assets,
        })
    }

    #[deprecated]
    pub async fn get_chain_assets(
        &mut self,
        address: &str,
        get_chain: GetChain,
    ) -> Result<GetChainAssetsRes, crate::ServiceError> {
        let tx = &mut self.repo;
        let token_currencies = self.coin_domain.get_token_currencies_v2(tx).await?;

        let assets = match get_chain {
            GetChain::All => {
                // 查询该账户下的全部资产
                tx.get_coin_assets_in_address(vec![address.to_string()])
                    .await?
            }
            GetChain::One(ref chain_code, symbol) => {
                // 查这个链的特定币种
                tx.get_chain_assets_by_address_chain_code_symbol(
                    vec![address.to_string()],
                    Some(chain_code.to_string()),
                    Some(&symbol),
                    None,
                )
                .await?
            }
        };

        let mut chain_assets = Vec::<CoinAssets>::new();

        for asset in assets {
            let balance = token_currencies.calculate_assets_entity(&asset).await?;
            let data: CoinAssets = (balance, asset).into();
            chain_assets.push(data);
        }

        Ok(GetChainAssetsRes { chain_assets })
    }

    // 指定账户下的链的资产列表，需要去重
    // pub async fn get_account_chain_assets(
    //     &self,
    //     address: &str,
    //     account_id: Option<u32>,
    //     chain_code: Option<String>, // mut get_chain: wallet_entity::resources::assets::GetChain,
    //     is_multisig: Option<bool>,
    // ) -> Result<Vec<AccountChainAsset>, crate::ServiceError> {
    //     let service = Service::default();
    //     let account_addresses = self
    //         .account_domain
    //         .get_addresses(address, account_id, chain_code.clone(), is_multisig)
    //         .await?;
    //     let mut account_addresses = account_addresses
    //         .into_iter()
    //         .map(|address| address.address)
    //         .collect::<Vec<String>>();
    //     let mut res = Vec::<AccountChainAsset>::new();

    //     // 根据账户地址、网络查询币资产
    //     for address in account_addresses {
    //         let assets: Vec<AssetsEntity> = service
    //             .asset_service
    //             .get_chain_assets_by_account_address_chain_code_symbol(
    //                 vec![address],
    //                 chain_code.clone(),
    //                 None,
    //             )
    //             .await?;

    //         for asset in assets {
    //             let token_currency =
    //                 super::get_current_coin_unit_price(&asset.symbol, &asset.chain_code).await?;

    //             if let Some(existing_asset) = res.iter_mut().find(|a| a.symbol == asset.symbol) {
    //                 // 如果资产已存在，合并
    //                 let balance = wallet_utils::parse_func::decimal_from_str(&asset.balance)?;
    //                 let balacne_f = wallet_utils::parse_func::f64_from_str(&asset.balance)?;
    //                 let config = crate::config::CONFIG.read().unwrap();
    //                 let currency = config.currency();

    //                 let price = token_currency.get_price(currency);

    //                 let BalanceInfo {
    //                     amount,
    //                     currency,
    //                     unit_price,
    //                     fiat_value,
    //                 } = &mut existing_asset.balance;

    //                 let after_balance = *amount + balacne_f;
    //                 *amount = after_balance;
    //                 let fiat_balance = price * after_balance;
    //                 *fiat_value = Some(fiat_balance);

    //                 // existing_asset.usdt_balance = (after_balance * unit_price).to_string();
    //                 // FIXME: btc 的资产是 非multisig 的，需要特殊处理
    //                 existing_asset.is_multichain = true;
    //             } else {
    //                 let balance = (token_currency, &asset).try_into()?;
    //                 res.push(AccountChainAsset {
    //                     chain_code: asset.chain_code,
    //                     symbol: asset.symbol,
    //                     balance,
    //                     is_multichain: false,
    //                     is_multisig: asset.is_multisig, // chains: vec![chain_assets],
    //                 });
    //             }
    //         }
    //     }

    //     // 过滤掉multisig的资产
    //     if let Some(is_multisig) = is_multisig {
    //         res = res
    //             .into_iter()
    //             .filter(|asset| {
    //                 if is_multisig {
    //                     asset.is_multisig == 1
    //                 } else {
    //                     asset.is_multisig == 0 || asset.is_multisig == 2
    //                 }
    //             })
    //             .collect();
    //     }

    //     Ok(res)
    // }

    // 指定账户下的链的资产列表，需要去重
    pub async fn get_account_chain_assets_v2(
        mut self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> Result<AccountChainAssetList, crate::ServiceError> {
        let mut tx = self.repo;
        let account_addresses = self
            .account_domain
            .get_addresses(
                &mut tx,
                address,
                account_id,
                chain_code.clone(),
                is_multisig,
            )
            .await?;

        // tracing::info!("account_addresses: {:?}", account_addresses);

        let mut res = AccountChainAssetList::default();
        let token_currencies = self.coin_domain.get_token_currencies_v2(&mut tx).await?;
        // 根据账户地址、网络查询币资产
        for address in account_addresses {
            let assets: Vec<AssetsEntity> = tx
                .get_chain_assets_by_address_chain_code_symbol(
                    vec![address.address],
                    Some(address.chain_code),
                    None,
                    None,
                )
                .await?;
            for asset in assets {
                if let Some(existing_asset) = res.iter_mut().find(|a| a.symbol == asset.symbol) {
                    token_currencies
                        .calculate_assets(asset, existing_asset)
                        .await?;
                } else {
                    let balance = token_currencies.calculate_assets_entity(&asset).await?;
                    let chain_code = if chain_code.is_none()
                        && let Some(chain) = tx.detail_with_main_symbol(&asset.symbol).await?
                    {
                        chain.chain_code
                    } else {
                        asset.chain_code
                    };
                    // tracing::warn!("[获取资产列表] 链码: {:?}", chain_code);
                    res.push(AccountChainAsset {
                        chain_code,
                        symbol: asset.symbol,
                        name: asset.name,
                        balance,
                        is_multichain: false,
                        is_multisig: asset.is_multisig, // chains: vec![chain_assets],
                    });
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
        res.mark_multichain_assets();
        res.sort_account_chain_assets();
        Ok(res)
    }

    pub async fn add_coin(
        self,
        address: &str,
        account_id: Option<u32>,
        symbol: &str,
        chain_code: Option<String>,
        is_multisig: Option<bool>,
    ) -> Result<(), crate::ServiceError> {
        let mut tx = self.repo;
        let accounts = self
            .account_domain
            .get_addresses(
                &mut tx,
                address,
                account_id,
                chain_code.clone(),
                is_multisig,
            )
            .await?;

        let coins = tx.coin_list(Some(symbol), chain_code.clone()).await?;
        let mut req: TokenQueryPriceReq = TokenQueryPriceReq(Vec::new());

        for account in accounts {
            if let Some(coin) = coins
                .iter()
                .find(|coin| coin.chain_code == account.chain_code)
            {
                let chain_code = account.chain_code.as_str();
                // let code: ChainCode = chain_code.try_into()?;

                // 判断资产是否已添加
                let asset = tx
                    .assets_by_id(&AssetsId {
                        address: account.address.to_string(),
                        chain_code: chain_code.to_string(),
                        symbol: symbol.to_string(),
                    })
                    .await?;

                if asset.is_none() {
                    let assets_id = AssetsId::new(&account.address, chain_code, symbol);

                    let assets = CreateAssetsVo::new(
                        assets_id,
                        coin.decimals,
                        coin.token_address().clone(),
                        coin.protocol.clone(),
                        0,
                    )
                    .with_name(&coin.name)
                    .with_u256(alloy::primitives::U256::default(), coin.decimals)?;

                    if coin.price.is_empty() {
                        req.insert(
                            chain_code,
                            &assets.token_address.clone().unwrap_or_default(),
                        );
                    }
                    tx.upsert_assets(assets).await?;
                }
            }
        }
        let task =
            domain::task_queue::Task::Common(domain::task_queue::CommonTask::QueryCoinPrice(req));
        Tasks::new().push(task).send().await?;
        Ok(())
    }

    pub async fn remove_coin(
        &mut self,
        address: &str,
        account_id: Option<u32>,
        symbol: &str,
        is_multisig: Option<bool>,
    ) -> Result<(), crate::ServiceError> {
        let tx = &mut self.repo;
        let accounts = self
            .account_domain
            .get_addresses(tx, address, account_id, None, is_multisig)
            .await?;

        // tracing::info!("remove_coin: {:?}", accounts);
        let mut assets_ids = Vec::new();
        let mut coin_ids = Vec::new();
        for account in accounts {
            let assets_id = AssetsId::new(&account.address, &account.chain_code, symbol);
            assets_ids.push(assets_id);
            let coin_id = SymbolId::new(&account.chain_code, symbol);
            coin_ids.push(coin_id);
        }
        tx.delete_multi_assets(assets_ids).await?;
        tx.drop_multi_custom_coin(coin_ids).await?;

        Ok(())
    }

    pub async fn get_coin_list(
        self,
        address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<crate::response_vo::coin::CoinInfoList, crate::ServiceError> {
        let mut tx = self.repo;
        let account_addresses = self
            .account_domain
            .get_addresses(
                &mut tx,
                address,
                account_id,
                chain_code.clone(),
                is_multisig,
            )
            .await?;
        let account_addresses = account_addresses
            .into_iter()
            .map(|address| address.address)
            .collect::<Vec<String>>();
        let mut res = self
            .assets_domain
            .get_local_coin_list(&mut tx, account_addresses, chain_code, keyword, is_multisig)
            .await?;
        res.mark_multichain_assets();
        Ok(res)
    }

    #[deprecated]
    pub async fn get_account_assets_by_symbol_and_chain_code(
        mut self,
        account_address: &str,
        chain_code: &str,
        symbol: &str,
    ) -> Result<AccountChainAsset, crate::ServiceError> {
        let mut tx = self.repo;
        let token_currencies = self.coin_domain.get_token_currencies_v2(&mut tx).await?;

        let Some(account) = AccountRepoTrait::detail(&mut tx, account_address).await? else {
            return Err(crate::ServiceError::Business(
                crate::BusinessError::Account(crate::AccountError::NotFound),
            ));
        };

        let wallet_address = account.wallet_address;
        let account_id = account.account_id;

        // 获取钱包下的这个账户的所有地址
        let accounts = tx
            .get_account_list_by_wallet_address_and_account_id(
                Some(&wallet_address),
                Some(account_id),
            )
            .await?;

        let mut account_addresses = Vec::<String>::new();
        for account in accounts {
            if !account_addresses
                .iter()
                .any(|address| address == &account.address)
            {
                account_addresses.push(account.address);
            }
        }

        let assets = tx
            .get_chain_assets_by_address_chain_code_symbol(
                account_addresses,
                None,
                Some(symbol),
                None,
            )
            .await?;
        let is_multichain = assets.len() > 1;
        let asset = tx
            .assets_by_id(&AssetsId {
                address: account.address.to_string(),
                chain_code: chain_code.to_string(),
                symbol: symbol.to_string(),
            })
            .await?
            .ok_or(crate::ServiceError::Business(
                crate::AssetsError::NotFound.into(),
            ))?;

        let balance = token_currencies.calculate_assets_entity(&asset).await?;

        Ok(AccountChainAsset {
            chain_code: asset.chain_code,
            symbol: asset.symbol,
            name: asset.name,
            balance,
            is_multichain,
            is_multisig: asset.is_multisig,
        })
    }

    // 更具地址来同步余额
    pub async fn sync_assets_by_addr(
        self,
        addr: Vec<String>,
        chain_code: Option<String>,
        symbol: Vec<String>,
    ) -> Result<(), crate::ServiceError> {
        AssetsDomain::sync_assets_by_address(addr, chain_code, symbol).await
    }

    // 根据钱包地址来同步资产余额
    pub async fn sync_assets_by_wallet(
        self,
        wallet_address: String,
        account_id: Option<u32>,
        symbol: Vec<String>,
    ) -> Result<(), crate::ServiceError> {
        AssetsDomain::sync_assets_by_wallet(wallet_address, account_id, symbol).await
    }
}
