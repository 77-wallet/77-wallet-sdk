use alloy::signers::Result;
use chrono::{DateTime, Utc};

use crate::{
    CoreDbPool,
    dao::{assets::AssetsDao, coin::CoinDao},
    entities::{
        asset_token_key::AssetTokenKey,
        coin::{BatchCoinSwappable, CoinData, CoinEntity, CoinId, CoinWithAssets, SymbolId},
    },
    pagination::Pagination,
};

pub struct CoinRepo;
#[cfg(test)]
mod catalog_metadata_tests;
#[path = "coin/precision.rs"]
mod precision;
#[cfg(test)]
#[path = "coin/precision_tests.rs"]
mod precision_tests;

impl CoinRepo {
    pub async fn upsert_multi_coin(
        pool: &CoreDbPool,
        coin: Vec<CoinData>,
    ) -> Result<(), crate::Error> {
        if coin.is_empty() {
            return Ok(());
        }
        let mut native_symbols = std::collections::HashMap::new();
        for native in coin.iter().filter(|coin| coin.token_address.is_native()) {
            if let Some(previous) = native_symbols.insert(&native.chain_code, &native.symbol)
                && previous != &native.symbol
            {
                return Err(crate::Error::Other(format!(
                    "conflicting native symbols in catalog batch for {}",
                    native.chain_code
                )));
            }
        }
        let keys: Vec<_> = coin
            .iter()
            .map(|coin| (coin.chain_code.clone(), coin.token_address.clone(), coin.decimals))
            .collect();
        let mut tx = pool.write_ref().begin().await.map_err(crate::DatabaseError::from)?;
        // Use the existing transaction so a uniqueness failure or cancellation cannot
        // leave catalog identity and cached asset labels on different symbols.
        for native in coin.iter().filter(|coin| coin.token_address.is_native()) {
            CoinDao::rename_native_symbol(tx.as_mut(), &native.chain_code, &native.symbol).await?;
            AssetsDao::sync_native_coin_metadata(
                tx.as_mut(),
                &native.chain_code,
                &native.symbol,
                native.name.as_deref(),
            )
            .await?;
        }
        CoinDao::upsert_multi_coin(tx.as_mut(), coin).await?;
        precision::sync_asset_precision_batch(tx.as_mut(), &keys).await?;
        tx.commit().await.map_err(crate::DatabaseError::from)?;
        Ok(())
    }

    pub async fn drop_coin_just_null_token_address(pool: &CoreDbPool) -> Result<(), crate::Error> {
        CoinDao::drop_coin_just_null_token_address(pool.write_ref()).await
    }

    pub async fn get_market_chain_list(pool: &CoreDbPool) -> Result<Vec<String>, crate::Error> {
        CoinDao::chain_code_list(pool.read_ref()).await
    }

    pub async fn hot_coin_list_symbol_not_in(
        pool: &CoreDbPool,
        exclude: &[CoinId],
        chain_code: Option<String>,
        keyword: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<crate::pagination::Pagination<CoinEntity>, crate::Error> {
        CoinDao::coin_list_symbol_not_in(
            pool.read_ref(),
            exclude,
            chain_code,
            keyword,
            page,
            page_size,
        )
        .await
    }

    pub async fn coin_list_by_chain_token_map_batch(
        pool: &CoreDbPool,
        chain_list: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        CoinDao::list_by_chain_token_map_batch(pool.read_ref(), chain_list).await
    }

    pub async fn default_coin_list(pool: &CoreDbPool) -> Result<Vec<CoinEntity>, crate::Error> {
        CoinDao::list_v2(pool.read_ref(), None, None, Some(1)).await
    }

    pub async fn coin_by_chain_token_key(
        chain_code: &str,
        token_key: AssetTokenKey,
        pool: &CoreDbPool,
    ) -> Result<CoinEntity, crate::Error> {
        tracing::info!(
            chain_code = %chain_code,
            token_key = %token_key,
            "coin_by_chain_token_key lookup start"
        );
        CoinDao::get_coin_by_chain_code_token_address(
            pool.read_ref(),
            chain_code,
            token_key.clone(),
        )
        .await?
        .ok_or(crate::Error::NotFound(format!(
            "coin not found: chain_code: {}, token: {}",
            chain_code,
            token_key.as_db_str()
        )))
    }

    pub async fn coin_by_chain_token_key_opt(
        chain_code: &str,
        token_key: AssetTokenKey,
        pool: &CoreDbPool,
    ) -> Result<Option<CoinEntity>, crate::Error> {
        CoinDao::get_coin_by_chain_code_token_address(pool.read_ref(), chain_code, token_key).await
    }

    pub async fn main_coin(
        chain_code: &str,
        pool: &CoreDbPool,
    ) -> Result<CoinEntity, crate::Error> {
        CoinDao::main_coin(chain_code, pool.read_ref()).await?.ok_or(crate::Error::NotFound(
            format!("main coin not found: chain_code: {}", chain_code),
        ))
    }

    // 修复数据用
    pub async fn delete_wsol_error(pool: &CoreDbPool) -> Result<(), crate::Error> {
        CoinDao::delete_wsol_error(pool.write_ref()).await
    }

    pub async fn update_price_unit1(
        chain_code: &str,
        token_key: AssetTokenKey,
        price: &str,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        CoinDao::update_price_unit1(pool.write_ref(), chain_code, token_key.as_db_str(), price)
            .await
    }

    pub async fn multi_update_swappable(
        coins: Vec<BatchCoinSwappable>,
        pool: &CoreDbPool,
    ) -> Result<(), crate::Error> {
        CoinDao::multi_update_swappable(coins, pool.write_ref()).await
    }

    pub async fn drop_multi_custom_coin(
        pool: &CoreDbPool,
        coin_ids: std::collections::HashSet<SymbolId>,
    ) -> Result<(), crate::Error> {
        CoinDao::drop_multi_custom_coin(pool.write_ref(), coin_ids).await
    }

    pub async fn last_coin(
        pool: &CoreDbPool,
        is_create: bool,
    ) -> Result<Option<CoinEntity>, crate::Error> {
        CoinDao::get_last_coin(pool.read_ref(), is_create).await
    }

    pub async fn coin_count(pool: &CoreDbPool) -> Result<i64, crate::Error> {
        CoinDao::coin_count(pool.read_ref()).await
    }

    pub async fn same_coin_num(
        pool: &CoreDbPool,
        symbol: &str,
        chain_code: &str,
    ) -> Result<i64, crate::Error> {
        CoinDao::same_coin_num(pool.read_ref(), symbol, chain_code).await
    }

    pub async fn coin_list_with_assets(
        search: &str,
        exclude_token: Vec<String>,
        chain_code: String,
        address: Vec<String>,
        page: i64,
        page_size: i64,
        pool: CoreDbPool,
    ) -> Result<Pagination<CoinWithAssets>, crate::Error> {
        CoinDao::coin_list_with_assets(
            search,
            exclude_token,
            chain_code,
            address,
            page,
            page_size,
            pool.into_inner(),
        )
        .await
    }

    pub async fn coin_list_v2(
        pool: CoreDbPool,
        symbol: Option<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        CoinDao::list_v2(pool.read_ref(), symbol, chain_code, None).await
    }

    pub async fn list_v2(
        pool: &CoreDbPool,
        symbol: Option<String>,
        chain_code: Option<String>,
        status: Option<u8>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        CoinDao::list_v2(pool.read_ref(), symbol, chain_code, status).await
    }

    pub async fn update_price_unit(
        pool: CoreDbPool,
        coin_id: &CoinId,
        price: &str,
        unit: Option<u8>,
        status: Option<i32>,
        swappable: Option<bool>,
        time: Option<DateTime<Utc>>,
        symbols: Option<String>,
    ) -> Result<(), crate::Error> {
        let mut tx = pool.write_ref().begin().await.map_err(crate::DatabaseError::from)?;
        CoinDao::update_price_unit(
            tx.as_mut(),
            coin_id,
            price,
            unit,
            status,
            swappable,
            time,
            symbols,
        )
        .await?;
        if let Some(decimals) = unit {
            precision::sync_asset_precision(
                tx.as_mut(),
                &coin_id.chain_code,
                &coin_id.token_address,
                decimals,
            )
            .await?;
        }
        tx.commit().await.map_err(crate::DatabaseError::from)?;
        Ok(())
    }

    pub async fn coin_list_with_symbols(
        pool: CoreDbPool,
        symbols: &[String],
        chain_code: Option<String>,
    ) -> Result<Vec<CoinEntity>, crate::Error> {
        CoinDao::list(pool.read_ref(), symbols, chain_code, None).await
    }

    pub async fn batch_update_default_coin_status(
        pool: CoreDbPool,
        coin_ids: &[CoinId],
        status: u8,
    ) -> Result<(), crate::Error> {
        CoinDao::batch_update_default_coin_status(pool.write_ref(), coin_ids, status).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn make_temp_dir(prefix: &str) -> String {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    fn make_coin(symbol: &str, chain_code: &str, token_address: Option<&str>) -> CoinData {
        let now = Utc::now();
        CoinData::new(
            Some(symbol.to_string()),
            symbol,
            chain_code,
            token_address.map(|s| s.to_string()).into(),
            Some("0".to_string()),
            None,
            if symbol.eq_ignore_ascii_case("SOL") { 9 } else { 6 },
            1,
            1,
            1,
            true,
            now,
            now,
        )
    }

    async fn prepare_sol_coin_pool() -> CoreDbPool {
        let dir = make_temp_dir("wallet_db_coin_repo_token_key");
        let ctx = crate::SqliteContext::new(&dir, Some("data.db")).await.unwrap();
        let pool = ctx.get_pool().unwrap();
        CoinDao::upsert_multi_coin(
            pool.as_ref(),
            vec![
                make_coin("SOL", "sol", None),
                make_coin("USDT", "sol", Some("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB")),
            ],
        )
        .await
        .unwrap();
        CoreDbPool::new(pool)
    }

    #[tokio::test]
    async fn coin_by_chain_token_key_hits_contract_token() {
        let pool = prepare_sol_coin_pool().await;

        let coin = CoinRepo::coin_by_chain_token_key(
            "sol",
            AssetTokenKey::Contract("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string()),
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(coin.symbol, "USDT");
        assert_eq!(coin.chain_code, "sol");
        assert_eq!(
            coin.token_address,
            AssetTokenKey::Contract("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string())
        );
    }

    #[tokio::test]
    async fn coin_by_chain_token_key_hits_native_for_empty_token() {
        let pool = prepare_sol_coin_pool().await;

        let coin =
            CoinRepo::coin_by_chain_token_key("sol", AssetTokenKey::Native, &pool).await.unwrap();

        assert_eq!(coin.symbol, "SOL");
        assert_eq!(coin.token_address, AssetTokenKey::Native);
    }

    #[tokio::test]
    async fn coin_by_chain_token_key_returns_not_found_for_unknown_token() {
        let pool = prepare_sol_coin_pool().await;

        let err = CoinRepo::coin_by_chain_token_key(
            "sol",
            AssetTokenKey::Contract("wrong-token".to_string()),
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, crate::Error::NotFound(_)));
    }

    #[tokio::test]
    async fn coin_repo_list_v2_returns_inserted_default_coins() {
        let pool = prepare_sol_coin_pool().await;

        let coins = CoinRepo::list_v2(&pool, None, Some("sol".to_string()), Some(1)).await.unwrap();

        assert!(!coins.is_empty());
        assert!(coins.iter().any(|c| c.symbol == "SOL"));
        assert!(coins.iter().all(|c| c.chain_code == "sol"));
    }

    #[tokio::test]
    async fn coin_repo_list_v2_returns_empty_for_unknown_chain() {
        let pool = prepare_sol_coin_pool().await;

        let coins =
            CoinRepo::list_v2(&pool, None, Some("tron".to_string()), Some(1)).await.unwrap();

        assert!(coins.is_empty());
    }

    #[tokio::test]
    async fn coin_repo_list_v2_treats_sql_shaped_chain_as_data() {
        let pool = prepare_sol_coin_pool().await;

        let coins = CoinRepo::list_v2(&pool, None, Some("sol' OR 1=1 --".to_string()), Some(1))
            .await
            .unwrap();

        assert!(coins.is_empty());
    }

    #[tokio::test]
    async fn coin_repo_symbol_list_treats_sql_shaped_symbol_as_data() {
        let pool = prepare_sol_coin_pool().await;
        let symbols = vec!["SOL') OR 1=1 --".to_string()];

        let coins = CoinDao::list(pool.read_ref(), &symbols, None, None).await.unwrap();

        assert!(coins.is_empty());
    }

    #[tokio::test]
    async fn coin_list_page_filters_and_counts_matching_rows() {
        let pool = prepare_sol_coin_pool().await;

        let page = CoinDao::coin_list_page(
            std::sync::Arc::new(pool.read_ref().clone()),
            Some("sol"),
            Some("USD"),
            vec!["USDT".to_string()],
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].symbol, "USDT");
    }

    #[tokio::test]
    async fn coin_list_page_treats_sql_shaped_filters_as_data() {
        let pool = prepare_sol_coin_pool().await;
        let pool = std::sync::Arc::new(pool.read_ref().clone());

        let cases = [
            (Some("sol' OR 1=1 --"), None, Vec::new()),
            (None, None, vec!["SOL') OR 1=1 --".to_string()]),
            (None, Some("%' OR 1=1 --"), Vec::new()),
        ];
        for (chain_code, keyword, symbols) in cases {
            let page = CoinDao::coin_list_page(pool.clone(), chain_code, keyword, symbols, 0, 10)
                .await
                .unwrap();
            assert_eq!(page.total_count, 0);
            assert!(page.data.is_empty());
        }
    }

    #[tokio::test]
    async fn hot_coin_list_excludes_exact_coin_and_updates_total_count() {
        let pool = prepare_sol_coin_pool().await;
        let exclude = vec![CoinId::new("sol", "SOL", AssetTokenKey::Native)];

        let page = CoinRepo::hot_coin_list_symbol_not_in(
            &pool,
            &exclude,
            Some("sol".to_string()),
            None,
            0,
            10,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].symbol, "USDT");
    }

    #[tokio::test]
    async fn hot_coin_list_treats_sql_shaped_chain_as_data() {
        let pool = prepare_sol_coin_pool().await;

        let page = CoinRepo::hot_coin_list_symbol_not_in(
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
    async fn coin_list_with_assets_joins_requested_address_balance() {
        let pool = prepare_sol_coin_pool().await;
        let token = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
        crate::repositories::test_helper::seed_assets(
            &pool,
            crate::entities::assets::AssetsId::new("owner", "sol", token.into()),
            "USDT",
            "USDT",
            6,
            "12.5",
        )
        .await;

        let page = CoinRepo::coin_list_with_assets(
            "",
            Vec::new(),
            "sol".to_string(),
            vec!["owner".to_string()],
            0,
            10,
            pool,
        )
        .await
        .unwrap();

        assert_eq!(page.total_count, 2);
        assert_eq!(page.data[0].symbol, "USDT");
        assert_eq!(page.data[0].balance, "12.5");
    }

    #[tokio::test]
    async fn coin_list_with_assets_treats_sql_shaped_selectors_as_data() {
        let pool = prepare_sol_coin_pool().await;
        let token = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
        crate::repositories::test_helper::seed_assets(
            &pool,
            crate::entities::assets::AssetsId::new("victim", "sol", token.into()),
            "USDT",
            "USDT",
            6,
            "99",
        )
        .await;

        let address_page = CoinRepo::coin_list_with_assets(
            "",
            Vec::new(),
            "sol".to_string(),
            vec!["victim') OR 1=1 --".to_string()],
            0,
            10,
            pool.clone(),
        )
        .await
        .unwrap();
        assert!(address_page.data.iter().all(|coin| coin.balance == "0"));

        let chain_page = CoinRepo::coin_list_with_assets(
            "",
            Vec::new(),
            "sol' OR 1=1 --".to_string(),
            Vec::new(),
            0,
            10,
            pool,
        )
        .await
        .unwrap();
        assert_eq!(chain_page.total_count, 0);
        assert!(chain_page.data.is_empty());
    }

    #[tokio::test]
    async fn coin_repo_main_coin_missing_chain_returns_not_found() {
        let pool = prepare_sol_coin_pool().await;
        let err = CoinRepo::main_coin("tron", &pool).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound(_)));
    }

    #[tokio::test]
    async fn coin_repo_tx_rollback_keeps_price_unchanged() {
        let pool = prepare_sol_coin_pool().await;
        let before =
            CoinRepo::coin_by_chain_token_key("sol", AssetTokenKey::Native, &pool).await.unwrap();
        let coin_id = CoinId::new("sol", "SOL", None::<String>.into());

        let mut tx = pool.write_ref().begin().await.unwrap();
        CoinDao::update_price_unit(tx.as_mut(), &coin_id, "9.99", None, None, None, None, None)
            .await
            .unwrap();
        tx.rollback().await.unwrap();

        let after =
            CoinRepo::coin_by_chain_token_key("sol", AssetTokenKey::Native, &pool).await.unwrap();
        assert_eq!(after.price, before.price);
    }
}
