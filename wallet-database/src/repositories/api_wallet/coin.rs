use chrono::{DateTime, Utc};

use crate::{
    ApiWalletDbPool,
    dao::api_coin::ApiCoinDao,
    entities::{
        api_coin::{ApiCoinData, ApiCoinEntity},
        asset_token_key::AssetTokenKey,
        coin::{BatchCoinSwappable, CoinId, CoinWithAssets},
    },
    pagination::Pagination,
};

pub struct ApiCoinRepo;

impl ApiCoinRepo {
    /// Recheck API-chain enable/master/binding atomically with native default insertion.
    pub async fn insert_missing_native_coin_for_bound_chain(
        pool: &ApiWalletDbPool,
        coin: ApiCoinData,
        node_id: &str,
    ) -> Result<bool, crate::Error> {
        ApiCoinDao::insert_missing_native_coin_for_bound_chain(pool.write_ref(), coin, node_id)
            .await
    }

    pub async fn upsert_multi_coin(
        pool: &ApiWalletDbPool,
        coin: Vec<ApiCoinData>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error> {
        if coin.is_empty() {
            return Ok(Vec::new());
        }
        let _guard = pool.lock_write_with_metric("api_coin_metadata").await;
        let mut tx = pool.write_ref().begin().await.map_err(crate::DatabaseError::from)?;
        let rows = ApiCoinDao::upsert_multi_coin(tx.as_mut(), coin).await?;
        tx.commit().await.map_err(crate::DatabaseError::from)?;
        Ok(rows)
    }

    pub async fn coin_list(pool: &ApiWalletDbPool) -> Result<Vec<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::list(pool.read_ref(), None, None, None).await
    }

    pub async fn coin_list_v2(
        pool: &ApiWalletDbPool,
        symbol: Option<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::list_v2(pool.read_ref(), symbol, chain_code, None).await
    }

    pub async fn coin_list_by_chain_token_map_batch(
        pool: &ApiWalletDbPool,
        chain_list: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::list_by_chain_token_map_batch(pool.read_ref(), chain_list).await
    }

    pub async fn coin_list_by_chain_token_pairs_batch(
        pool: &ApiWalletDbPool,
        pairs: &[(String, String)],
    ) -> Result<Vec<ApiCoinEntity>, crate::Error> {
        if pairs.is_empty() {
            return Ok(vec![]);
        }

        // SQLite 默认参数上限通常为 999，每个 pair 需要 2 个 bind 参数
        const MAX_PAIRS_PER_QUERY: usize = 400;
        let mut all = Vec::new();
        for chunk in pairs.chunks(MAX_PAIRS_PER_QUERY) {
            let mut res =
                ApiCoinDao::list_by_chain_token_pairs_batch(pool.read_ref(), chunk).await?;
            all.append(&mut res);
        }
        Ok(all)
    }

    pub async fn main_coin(
        chain_code: &str,
        pool: &ApiWalletDbPool,
    ) -> Result<ApiCoinEntity, crate::Error> {
        ApiCoinDao::main_coin(chain_code, pool.read_ref()).await?.ok_or(crate::Error::NotFound(
            format!("main coin not found: chain_code: {}", chain_code),
        ))
    }

    pub async fn update_price_unit(
        coin_id: &CoinId,
        price: &str,
        unit: Option<u8>,
        status: Option<i32>,
        time: Option<DateTime<Utc>>,
        symbol: Option<String>,
        name: Option<String>,
        pool: &ApiWalletDbPool,
    ) -> Result<(), crate::Error> {
        let _guard = pool.lock_write_with_metric("api_coin_metadata").await;
        let mut tx = pool.write_ref().begin().await.map_err(crate::DatabaseError::from)?;
        ApiCoinDao::update_price_unit(
            tx.as_mut(),
            coin_id,
            price,
            unit,
            status,
            time,
            symbol,
            name,
        )
        .await?;
        tx.commit().await.map_err(crate::DatabaseError::from)?;
        Ok(())
    }

    pub async fn coin_by_chain_token_key_opt(
        chain_code: &str,
        token_key: AssetTokenKey,
        pool: &ApiWalletDbPool,
    ) -> Result<Option<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::get_coin_by_chain_code_token_address(pool.read_ref(), chain_code, token_key)
            .await
    }

    pub async fn coin_list_symbol_not_in(
        pool: &ApiWalletDbPool,
        exclude: &[CoinId],
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<crate::pagination::Pagination<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::coin_list_symbol_not_in(
            pool.read_ref(),
            exclude,
            chain_code,
            keyword,
            page,
            page_size,
        )
        .await
    }

    pub async fn update_price_unit1(
        chain_code: &str,
        token_key: AssetTokenKey,
        price: &str,
        pool: &ApiWalletDbPool,
    ) -> Result<(), crate::Error> {
        ApiCoinDao::update_price_unit1(pool.write_ref(), chain_code, token_key.as_db_str(), price)
            .await
    }

    pub async fn multi_update_swappable(
        coins: Vec<BatchCoinSwappable>,
        pool: &ApiWalletDbPool,
    ) -> Result<(), crate::Error> {
        ApiCoinDao::multi_update_swappable(coins, pool.write_ref()).await
    }

    pub async fn coin_by_chain_token_key(
        chain_code: &str,
        token_key: AssetTokenKey,
        pool: &ApiWalletDbPool,
    ) -> Result<ApiCoinEntity, crate::Error> {
        let token_for_log = token_key.as_db_str().to_string();
        ApiCoinDao::get_coin_by_chain_code_token_address(pool.read_ref(), chain_code, token_key)
            .await?
            .ok_or(crate::Error::NotFound(format!(
                "coin not found: chain_code: {}, token: {}",
                chain_code, token_for_log
            )))
    }

    pub async fn has_coin(
        chain_code: &str,
        token_key: AssetTokenKey,
        pool: &ApiWalletDbPool,
    ) -> Result<bool, crate::Error> {
        ApiCoinRepo::coin_by_chain_token_key_opt(chain_code, token_key, pool)
            .await?
            .map_or_else(|| Ok(false), |_| Ok(true))
    }

    pub async fn last_coin(
        pool: &ApiWalletDbPool,
        is_create: bool,
    ) -> Result<Option<ApiCoinEntity>, crate::Error> {
        ApiCoinDao::get_last_coin(pool.read_ref(), is_create).await
    }

    pub async fn coin_count(pool: &ApiWalletDbPool) -> Result<i64, crate::Error> {
        ApiCoinDao::coin_count(pool.read_ref()).await
    }

    pub async fn same_coin_num(
        pool: &ApiWalletDbPool,
        symbol: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error> {
        ApiCoinDao::same_coin_num(pool.read_ref(), symbol, chain_code).await
    }

    pub async fn coin_list_with_assets(
        search: &str,
        exclude_token: Vec<String>,
        chain_code: String,
        address: Vec<String>,
        page: i64,
        page_size: i64,
        pool: &ApiWalletDbPool,
    ) -> Result<Pagination<CoinWithAssets>, crate::Error> {
        ApiCoinDao::coin_list_with_assets(
            search,
            exclude_token,
            chain_code,
            address,
            page,
            page_size,
            pool.read_pool(),
        )
        .await
    }

    pub async fn drop_coin_just_null_token_address(
        pool: &ApiWalletDbPool,
    ) -> Result<(), crate::Error> {
        ApiCoinDao::drop_coin_just_null_token_address(pool.write_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiCoinRepo;
    use crate::{
        dao::api_coin::ApiCoinDao,
        entities::{
            api_assets::ApiCreateAssetsVo, api_coin::ApiCoinData, asset_token_key::AssetTokenKey,
            assets::AssetsId,
        },
        repositories::{api_wallet::assets::ApiAssetsRepo, test_helper::setup_api_wallet_pool},
    };
    use chrono::Utc;

    fn make_coin(chain_code: &str, token_key: AssetTokenKey, price: &str) -> ApiCoinData {
        ApiCoinData::new(
            Some("USDT".to_string()),
            "USDT",
            chain_code,
            token_key,
            Some(price.to_string()),
            None,
            6,
            1,
            1,
            1,
            Utc::now(),
            None,
        )
    }

    #[tokio::test]
    async fn default_native_insert_rechecks_chain_status_symbol_and_binding() {
        use crate::{
            entities::api_chain::{ApiChainCreateVo, NodeBindType},
            repositories::api_wallet::chain::ApiChainRepo,
        };
        let pool = setup_api_wallet_pool("default_native_chain_guard").await;
        let chain = || ApiChainCreateVo::new("Base", "base", &[], NodeBindType::AutoBackend, "ETH");
        let coin = || {
            let mut c = make_coin("base", AssetTokenKey::default(), "0");
            c.symbol = "ETH".into();
            c
        };
        ApiChainRepo::add(&pool, chain()).await.unwrap();
        ApiChainRepo::set_chain_node_with_type(&pool, "base", "main", NodeBindType::AutoBackend)
            .await
            .unwrap();
        // Disable arrives after the default loader observed an enabled binding.
        ApiChainRepo::upsert_multi_chain(&pool, vec![chain().with_status(0)]).await.unwrap();
        assert!(
            !ApiCoinRepo::insert_missing_native_coin_for_bound_chain(&pool, coin(), "main")
                .await
                .unwrap()
        );
        ApiChainRepo::upsert_multi_chain(
            &pool,
            vec![ApiChainCreateVo::new("Base", "base", &[], NodeBindType::AutoBackend, "OTHER")],
        )
        .await
        .unwrap();
        assert!(
            !ApiCoinRepo::insert_missing_native_coin_for_bound_chain(&pool, coin(), "main")
                .await
                .unwrap()
        );
        ApiChainRepo::upsert_multi_chain(&pool, vec![chain()]).await.unwrap();
        ApiChainRepo::user_select(&pool, "base", "manual").await.unwrap();
        assert!(
            !ApiCoinRepo::insert_missing_native_coin_for_bound_chain(&pool, coin(), "main")
                .await
                .unwrap()
        );
        assert_eq!(ApiCoinRepo::coin_count(&pool).await.unwrap(), 0);
        assert!(
            ApiCoinRepo::insert_missing_native_coin_for_bound_chain(&pool, coin(), "manual")
                .await
                .unwrap()
        );
        assert!(
            !ApiCoinRepo::insert_missing_native_coin_for_bound_chain(&pool, coin(), "manual")
                .await
                .unwrap()
        );
        assert_eq!(ApiCoinRepo::coin_count(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn coin_repo_upsert_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_success").await;
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        let token = "0xapi_coin_token_s";
        let price = "1.23";

        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin(chain, AssetTokenKey::from_raw(Some(token)), price)],
        )
        .await
        .unwrap();

        let got = ApiCoinRepo::coin_by_chain_token_key_opt(
            chain,
            AssetTokenKey::from_raw(Some(token)),
            &pool,
        )
        .await
        .unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.chain_code, chain);
        assert_eq!(got.token_address.as_db_str(), token);
        assert_eq!(got.price, price);

        let count = ApiCoinRepo::coin_count(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn coin_repo_missing_token_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_edge").await;
        let got = ApiCoinRepo::coin_by_chain_token_key_opt(
            wallet_types::constant::chain_code::ETHEREUM,
            AssetTokenKey::from_raw(Some("0xapi_coin_missing_token")),
            &pool,
        )
        .await
        .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn api_coin_lists_filter_exact_values() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_list_exact").await;
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin("eth", "0xeth".into(), "1"), make_coin("sol", "sol-token".into(), "2")],
        )
        .await
        .unwrap();

        let coins = ApiCoinDao::list_v2(
            pool.read_ref(),
            Some("USDT".to_string()),
            Some("sol".to_string()),
            Some(1),
        )
        .await
        .unwrap();

        assert_eq!(coins.len(), 1);
        assert_eq!(coins[0].chain_code, "sol");
    }

    #[tokio::test]
    async fn api_coin_lists_treat_sql_shaped_values_as_data() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_list_bind").await;
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin("eth", "0xeth".into(), "1"), make_coin("sol", "sol-token".into(), "2")],
        )
        .await
        .unwrap();

        let by_chain =
            ApiCoinDao::list_v2(pool.read_ref(), None, Some("sol' OR 1=1 --".to_string()), Some(1))
                .await
                .unwrap();
        assert!(by_chain.is_empty());

        let by_symbol =
            ApiCoinDao::list(pool.read_ref(), Some("USDT' OR 1=1 --".to_string()), None, Some(1))
                .await
                .unwrap();
        assert!(by_symbol.is_empty());
    }

    #[tokio::test]
    async fn api_hot_coin_list_excludes_exact_coin_and_counts_rows() {
        let pool = setup_api_wallet_pool("wallet_db_api_hot_coin_exclude").await;
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin("eth", "0xeth".into(), "1"), make_coin("sol", "sol-token".into(), "2")],
        )
        .await
        .unwrap();
        let exclude = vec![crate::entities::coin::CoinId::new("eth", "USDT", "0xeth".into())];

        let page =
            ApiCoinRepo::coin_list_symbol_not_in(&pool, &exclude, None, None, 0, 10).await.unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].chain_code, "sol");
    }

    #[tokio::test]
    async fn api_hot_coin_list_treats_sql_shaped_chain_as_data() {
        let pool = setup_api_wallet_pool("wallet_db_api_hot_coin_bind").await;
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin("eth", "0xeth".into(), "1"), make_coin("sol", "sol-token".into(), "2")],
        )
        .await
        .unwrap();

        let page = ApiCoinRepo::coin_list_symbol_not_in(
            &pool,
            &[],
            Some("sol' OR 1=1 --".to_string()),
            None,
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 0);
        assert!(page.data.is_empty());
    }

    #[tokio::test]
    async fn api_coin_with_assets_filters_chain_and_joins_requested_balance() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_assets_filter").await;
        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin("eth", "0xeth".into(), "1"), make_coin("bnb", "0xbnb".into(), "2")],
        )
        .await
        .unwrap();
        let asset = ApiCreateAssetsVo::new(
            AssetsId::new("owner", "eth", "0xeth".into()),
            "USDT",
            6,
            None,
            0,
        )
        .with_balance("12.5");
        ApiAssetsRepo::upsert_assets(&pool, asset).await.unwrap();

        let page = ApiCoinRepo::coin_list_with_assets(
            "USD",
            Vec::new(),
            "eth".to_string(),
            vec!["owner".to_string()],
            0,
            10,
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].chain_code, "eth");
        assert_eq!(page.data[0].balance, "12.5");
    }

    #[tokio::test]
    async fn api_coin_with_assets_treats_sql_shaped_address_as_data() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_assets_bind").await;
        ApiCoinRepo::upsert_multi_coin(&pool, vec![make_coin("eth", "0xeth".into(), "1")])
            .await
            .unwrap();
        let asset = ApiCreateAssetsVo::new(
            AssetsId::new("victim", "eth", "0xeth".into()),
            "USDT",
            6,
            None,
            0,
        )
        .with_balance("99");
        ApiAssetsRepo::upsert_assets(&pool, asset).await.unwrap();

        let page = ApiCoinRepo::coin_list_with_assets(
            "",
            Vec::new(),
            "eth".to_string(),
            vec!["victim') OR 1=1 --".to_string()],
            0,
            10,
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].balance, "0");
    }

    #[tokio::test]
    async fn coin_repo_tx_rollback_keeps_price_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_api_coin_rollback").await;
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        let token = "0xapi_coin_token_rb";

        ApiCoinRepo::upsert_multi_coin(
            &pool,
            vec![make_coin(chain, AssetTokenKey::from_raw(Some(token)), "2.00")],
        )
        .await
        .unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ApiCoinDao::update_price_unit1(
            tx.as_mut(),
            chain,
            AssetTokenKey::from_raw(Some(token)).as_db_str(),
            "9.99",
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let got = ApiCoinRepo::coin_by_chain_token_key_opt(
            chain,
            AssetTokenKey::from_raw(Some(token)),
            &pool,
        )
        .await
        .unwrap();
        assert_eq!(got.unwrap().price, "2.00");

        let count = ApiCoinRepo::coin_count(&pool).await.unwrap();
        assert_eq!(count, 1);
    }
}

#[cfg(test)]
#[path = "coin/metadata_rename_tests.rs"]
mod metadata_rename_tests;

#[cfg(test)]
#[path = "coin/metadata_scale_tests.rs"]
mod metadata_scale_tests;
