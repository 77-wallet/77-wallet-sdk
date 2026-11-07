use crate::{
    context::Context,
    domain::{
        api_wallet::{
            account::ApiAccountDomain, adapter_factory::ApiChainAdapterFactory, coin::ApiCoinDomain,
        },
        chain::ChainDomain,
    },
    error::service::ServiceError,
    infrastructure::task_queue::{
        CommonTask,
        backend::{BackendApiTask, BackendApiTaskData},
        task::Tasks,
    },
    response_vo::standard_wallet::coin::CoinInfo,
};
use wallet_database::{
    entities::{
        api_assets::ApiCreateAssetsVo, api_coin::ApiCoinData, asset_token_key::AssetTokenKey,
        assets::AssetsId, coin::CoinId,
    },
    pagination::Pagination,
    repositories::api_wallet::{account::ApiAccountRepo, assets::ApiAssetsRepo, coin::ApiCoinRepo},
};
use wallet_transport_backend::request::TokenQueryPriceReq;

pub(crate) struct ApiCoinApplication {
    ctx: &'static Context,
}

impl ApiCoinApplication {
    pub(crate) fn new(ctx: &'static Context) -> Self {
        Self { ctx }
    }

    pub(crate) async fn get_hot_coin_list(
        &self,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<CoinInfo>, ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;

        let accounts =
            ApiAccountRepo::list_by_wallet_address(&pool, wallet_address, account_id, None).await?;
        let addresses =
            accounts.into_iter().map(|address| address.address).collect::<Vec<String>>();

        let assets = ApiAssetsRepo::get_chain_assets_by_address_chain_code_symbol(
            &pool,
            addresses,
            chain_code.clone(),
            None,
            is_multisig,
        )
        .await?;

        let exclude = assets
            .iter()
            .map(|asset| CoinId {
                symbol: asset.symbol.clone(),
                chain_code: asset.chain_code.clone(),
                token_address: asset.token_key(),
            })
            .collect::<Vec<CoinId>>();

        let list = ApiCoinRepo::coin_list_symbol_not_in(
            &pool, &exclude, chain_code, keyword, page, page_size,
        )
        .await?;

        let data = ApiCoinDomain::merge_coin_to_list(list.data, keyword.is_some())?;
        let res = Pagination { page, page_size, total_count: list.total_count, data: data.0 };
        Ok(res)
    }

    pub(crate) async fn customize_coin(
        &self,
        address: &str,
        account_id: Option<u32>,
        chain_code: &str,
        mut token_address: String,
        protocol: Option<String>,
        is_multisig: bool,
        status: u8,
    ) -> Result<(), ServiceError> {
        let pool = self.ctx.api_wallet_pool()?;
        let net = crate::domain::api_wallet::chain::ApiChainDomain::network_kind_by_chain_code(
            chain_code, self.ctx,
        )
        .await?;

        ChainDomain::check_token_address(&mut token_address, chain_code, net)?;

        let _ = ChainDomain::get_node_with_ctx(self.ctx, chain_code).await?;

        let chain_instance =
            ApiChainAdapterFactory::get_transaction_adapter_with_ctx(self.ctx, chain_code).await?;

        let coin = ApiCoinRepo::coin_by_chain_token_key_opt(
            chain_code,
            AssetTokenKey::from_raw(Some(token_address.as_str())),
            &pool,
        )
        .await?;
        let (decimals, symbol, name) = if let Some(coin) = coin {
            (coin.decimals, coin.symbol, coin.name)
        } else {
            let decimals = chain_instance.decimals(&token_address).await.map_err(|e| match e {
                wallet_chain_interact::Error::UtilsError(wallet_utils::Error::Parse(_))
                | wallet_chain_interact::Error::RpcError(_) => {
                    ServiceError::Business(crate::error::business::BusinessError::Coin(
                        crate::error::business::coin::CoinError::InvalidContractAddress(
                            token_address.to_string(),
                        ),
                    ))
                }
                _ => ServiceError::ChainInteract(e),
            })?;
            if decimals == 0 {
                return Err(ServiceError::Business(crate::error::business::BusinessError::Coin(
                    crate::error::business::coin::CoinError::InvalidContractAddress(
                        token_address.to_string(),
                    ),
                )));
            }
            let symbol = chain_instance.token_symbol(&token_address).await?;
            let name = chain_instance.token_name(&token_address).await?;

            let time = wallet_utils::time::now();
            let cus_coin = ApiCoinData::new(
                Some(name.clone()),
                &symbol,
                chain_code,
                AssetTokenKey::from_raw(Some(token_address.as_str())),
                None,
                protocol,
                decimals,
                0,
                0,
                status,
                time,
                Some(time),
            )
            .with_custom(1);
            let coin = vec![cus_coin];
            tracing::warn!("[customize_coin] coin: {:?} ", coin);
            ApiCoinRepo::upsert_multi_coin(&pool, coin).await?;

            (decimals, symbol, name)
        };

        let mut account_addresses = ApiAccountDomain::get_addresses_with_ctx(
            self.ctx,
            address,
            account_id,
            vec![chain_code.to_string()],
        )
        .await?;

        tracing::debug!("[customize_coin] account_addresses: {:?}", account_addresses);
        let account_addresses = account_addresses.pop().ok_or(ServiceError::Business(
            crate::error::business::BusinessError::Account(
                crate::error::business::account::AccountError::NotFound(address.to_string()),
            ),
        ))?;

        tracing::debug!("[customize_coin] account_addresses pop: {:?}", account_addresses);
        let is_multisig = if is_multisig { 1 } else { 0 };

        let balance = chain_instance
            .balance_token_key(
                &account_addresses.address,
                AssetTokenKey::from_raw(Some(token_address.as_str())),
            )
            .await?;
        let balance = wallet_utils::unit::format_to_string(balance, decimals)
            .unwrap_or_else(|_| "0".to_string());

        let assets_id = AssetsId::new(
            &account_addresses.address,
            chain_code,
            Some(token_address.clone()).into(),
        );
        let assets = ApiCreateAssetsVo::new(assets_id, &symbol, decimals, None, is_multisig)
            .with_name(&name)
            .with_balance(&balance)
            .with_u256(alloy::primitives::U256::default(), decimals)?;

        ApiAssetsRepo::upsert_assets_multi(&pool, vec![assets]).await?;
        let req = wallet_transport_backend::request::CustomTokenInitReq {
            address: account_addresses.address,
            chain_code: chain_code.to_string(),
            symbol: symbol.to_string(),
            token_name: name,
            contract_address: Some(token_address.to_string()),
            master: false,
            unit: decimals,
        };
        let token_custom_init_task_data = BackendApiTaskData::new(
            wallet_transport_backend::consts::endpoint::TOKEN_CUSTOM_TOKEN_INIT,
            &req,
        )?;

        let mut req = TokenQueryPriceReq(Vec::new());
        req.insert(chain_code, &token_address);
        let task = CommonTask::QueryCoinPrice(req);
        Tasks::new()
            .push(BackendApiTask::BackendApi(token_custom_init_task_data))
            .push(task)
            .send_with_ctx(self.ctx)
            .await?;
        Ok(())
    }
}
