use crate::{
    context::Context,
    domain::{self, app::config::ConfigDomain, bill::BillDomain},
    response_vo::CoinCurrency,
};
use wallet_database::{
    entities::{asset_token_key::AssetTokenKey, bill::BillEntity},
    pagination::Pagination,
    repositories::{account::AccountRepo, bill::BillRepo, permission::PermissionRepo},
};

pub struct BillService {
    ctx: &'static crate::context::Context,
}

impl BillService {
    pub fn new(ctx: &'static Context) -> Result<Self, crate::error::service::ServiceError> {
        Ok(Self { ctx })
    }

    pub async fn bill_lists(
        &self,
        root_addr: Option<String>,
        account_id: Option<u32>,
        addr: Option<String>,
        chain_code: Option<&str>,
        symbol: Option<&str>,
        is_multisig: Option<i64>,
        filter_min_value: Option<bool>,
        start: Option<i64>,
        end: Option<i64>,
        transfer_type: Vec<i32>,
        page: i64,
        page_size: i64,
    ) -> Result<Pagination<BillEntity>, crate::error::service::ServiceError> {
        let sqlite_pool = {
            let pool = self.ctx.get_global_sqlite_pool()?;
            wallet_database::CoreDbPool::new(pool)
        };
        let adds = if let Some(addr) = addr {
            vec![addr]
        } else {
            let account = AccountRepo::get_account_list_by_wallet_address_and_account_id(
                sqlite_pool.clone(),
                root_addr.as_deref(),
                account_id,
            )
            .await?;

            let mut address =
                account.iter().map(|item| item.address.clone()).collect::<Vec<String>>();

            // 兼容权限里面的地址
            let users = PermissionRepo::permission_by_users(&sqlite_pool, &address).await?;

            for user in users {
                address.push(user.grantor_addr.clone());
            }
            address
        };

        // 过滤最小金额
        let min_value = match (symbol, filter_min_value) {
            (Some(symbol), Some(true)) => {
                ConfigDomain::get_config_min_value(self.ctx, symbol).await?
            }
            _ => None,
        };

        let mut lists = BillRepo::bill_lists(
            &adds,
            chain_code,
            symbol,
            is_multisig,
            min_value,
            start,
            end,
            transfer_type,
            page,
            page_size,
            &sqlite_pool,
        )
        .await?;

        lists.data.iter_mut().for_each(|item| item.truncate_to_8_decimals());

        Ok(lists)
    }

    pub async fn sync_bill_by_address(
        &self,
        chain_code: &str,
        address: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let backend_api = self.ctx.get_global_backend_api();
        let pool = self.ctx.core_pool()?;
        BillDomain::sync_bills(&pool, &backend_api, chain_code, address).await
    }

    pub async fn sync_bill_by_wallet_and_account(
        &self,
        wallet_address: String,
        account_id: u32,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = self.ctx.core_pool()?;
        // get all
        let accounts = AccountRepo::get_account_list_by_wallet_address_and_account_id(
            core_pool,
            Some(wallet_address.as_str()),
            Some(account_id),
        )
        .await?;

        for account in accounts.iter() {
            let backend_api = self.ctx.get_global_backend_api();
            let pool = self.ctx.core_pool()?;
            if let Err(e) =
                BillDomain::sync_bills(&pool, &backend_api, &account.chain_code, &account.address)
                    .await
            {
                tracing::warn!(
                    "[bill::sync_bill_by_wallet_and_account] chain_code:{},address {},fail {}",
                    account.chain_code,
                    account.address,
                    e
                );
            }
        }

        Ok(())
    }

    pub async fn coin_currency_price(
        &self,
        chain_code: String,
        symbol: String,
        token_key: AssetTokenKey,
    ) -> Result<CoinCurrency, crate::error::service::ServiceError> {
        let currency = crate::app_state::APP_STATE.read().await;
        let currency = currency.currency();

        let token = domain::coin::TokenCurrencyGetter::get_currency_by_token_key(
            self.ctx,
            currency,
            &chain_code,
            &symbol,
            token_key,
        )
        .await?;

        Ok(CoinCurrency { currency: currency.to_string(), unit_price: token.currency_price })
    }
}
