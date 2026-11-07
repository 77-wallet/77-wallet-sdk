use crate::{
    ApiWalletDbPool,
    dao::api_assets::{ApiAssertSummeryEntity, ApiAssetsDao, AssetBalanceEntity, SumResult},
    entities::{
        api_assets::{
            ApiAssetsEntity, ApiAssetsEntityWithAddressType, ApiCreateAssetsVo,
            AssetWithWalletAddress,
        },
        asset_token_key::AssetTokenKey,
        assets::AssetsId,
    },
};

pub struct ApiAssetsRepo;
const ASSETS_WRITE_TX_CHUNK_SIZE: usize = 200;

impl ApiAssetsRepo {
    pub async fn upsert_assets(
        pool: &ApiWalletDbPool,
        assets: ApiCreateAssetsVo,
    ) -> Result<(), crate::Error> {
        ApiAssetsDao::upsert_assets(pool.write_ref(), assets).await
    }

    /// Initialize defaults without changing persisted user choices or balances.
    pub async fn insert_missing_assets_multi(
        pool: &ApiWalletDbPool,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        if assets.is_empty() {
            return Ok(());
        }
        let _write_guard = pool.lock_write_with_metric("insert_missing_assets_multi").await;
        let mut remaining = assets.into_iter();
        loop {
            let chunk: Vec<_> = remaining.by_ref().take(ASSETS_WRITE_TX_CHUNK_SIZE).collect();
            if chunk.is_empty() {
                break;
            }
            let mut tx = pool
                .write_ref()
                .begin()
                .await
                .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
            ApiAssetsDao::insert_missing_assets_multi(tx.as_mut(), chunk).await?;
            tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        }
        Ok(())
    }

    /// 批量插入或更新资产
    pub async fn upsert_assets_multi(
        pool: &ApiWalletDbPool,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        if assets.is_empty() {
            return Ok(());
        }

        let _write_guard = pool.lock_write_with_metric("upsert_assets_multi").await;
        let tx_start = std::time::Instant::now();
        let total_rows = assets.len();
        // 分块事务提交，缩短单次写锁持有时间。
        let mut remaining = assets;
        let mut chunks = 0usize;
        while !remaining.is_empty() {
            chunks += 1;
            let chunk_len = remaining.len().min(ASSETS_WRITE_TX_CHUNK_SIZE);
            let chunk: Vec<_> = remaining.drain(..chunk_len).collect();
            let mut tx = pool
                .write_ref()
                .begin()
                .await
                .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

            ApiAssetsDao::upsert_assets_multi(tx.as_mut(), chunk).await?;
            tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        }
        let elapsed_ms = tx_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            metric = "write_tx_duration_ms",
            db = "api_wallet.db",
            op = "upsert_assets_multi",
            chunks = %chunks,
            rows = %total_rows,
            value_ms = %elapsed_ms,
            "assets write completed"
        );

        Ok(())
    }

    /// 批量插入或更新资产（仅用于“余额同步”场景）
    ///
    /// ON CONFLICT 时只更新 `balance`，避免被默认资产初始化覆盖。
    pub async fn upsert_assets_multi_update_balance(
        pool: &ApiWalletDbPool,
        assets: Vec<ApiCreateAssetsVo>,
    ) -> Result<(), crate::Error> {
        if assets.is_empty() {
            return Ok(());
        }

        let _write_guard = pool.lock_write_with_metric("upsert_assets_multi_update_balance").await;
        let tx_start = std::time::Instant::now();
        let total_rows = assets.len();
        // 分块事务提交，缩短单次写锁持有时间。
        let mut remaining = assets;
        let mut chunks = 0usize;
        while !remaining.is_empty() {
            chunks += 1;
            let chunk_len = remaining.len().min(ASSETS_WRITE_TX_CHUNK_SIZE);
            let chunk: Vec<_> = remaining.drain(..chunk_len).collect();
            let mut tx = pool
                .write_ref()
                .begin()
                .await
                .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

            ApiAssetsDao::upsert_assets_multi_update_balance(tx.as_mut(), chunk).await?;
            tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        }
        let elapsed_ms = tx_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            metric = "write_tx_duration_ms",
            db = "api_wallet.db",
            op = "upsert_assets_multi_update_balance",
            chunks = %chunks,
            rows = %total_rows,
            value_ms = %elapsed_ms,
            "assets write completed"
        );
        Ok(())
    }

    pub async fn update_balance(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        token_key: AssetTokenKey,
        balance: &str,
    ) -> Result<(), crate::Error> {
        ApiAssetsDao::update_balance(pool.write_ref(), address, chain_code, token_key, balance)
            .await
    }

    /// 批量更新余额（使用事务批量执行，提升性能）
    pub async fn batch_update_balance(
        pool: &ApiWalletDbPool,
        updates: Vec<(String, String, AssetTokenKey, String)>, // (address, chain_code, token_key, balance)
    ) -> Result<(), crate::Error> {
        if updates.is_empty() {
            return Ok(());
        }

        let _write_guard = pool.lock_write_with_metric("batch_update_balance").await;
        let tx_start = std::time::Instant::now();
        let total_rows = updates.len();
        let mut remaining = updates;
        let mut chunks = 0usize;
        while !remaining.is_empty() {
            chunks += 1;
            let chunk_len = remaining.len().min(ASSETS_WRITE_TX_CHUNK_SIZE);
            let chunk: Vec<_> = remaining.drain(..chunk_len).collect();
            let mut tx = pool
                .write_ref()
                .begin()
                .await
                .map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;

            ApiAssetsDao::batch_update_balance_in_tx(&mut tx, &chunk).await?;
            tx.commit().await.map_err(|e| crate::Error::Database(crate::DatabaseError::Sqlx(e)))?;
        }
        let elapsed_ms = tx_start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            metric = "write_tx_duration_ms",
            db = "api_wallet.db",
            op = "batch_update_balance",
            chunks = %chunks,
            rows = %total_rows,
            value_ms = %elapsed_ms,
            "assets write completed"
        );

        Ok(())
    }

    pub async fn update_status(
        pool: &ApiWalletDbPool,
        chain_code: &str,
        symbol: &str,
        token_key: AssetTokenKey,
        status: u8,
    ) -> Result<(), crate::Error> {
        ApiAssetsDao::update_status(pool.write_ref(), chain_code, symbol, token_key, status).await
    }

    pub async fn find_by_id(
        pool: &ApiWalletDbPool,
        id: &AssetsId,
    ) -> Result<Option<ApiAssetsEntity>, crate::Error> {
        Ok(ApiAssetsDao::assets_by_id(pool.read_ref(), id).await?)
    }

    pub async fn list(
        pool: &ApiWalletDbPool,
        addr: Vec<String>,
        chain_code: Option<String>,
    ) -> Result<Vec<ApiAssetsEntity>, crate::Error> {
        Ok(ApiAssetsDao::list(pool.read_ref(), addr, chain_code).await?)
    }

    pub async fn get_chain_assets_by_address_chain_code_symbol(
        pool: &ApiWalletDbPool,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ApiAssetsEntity>, crate::Error> {
        ApiAssetsDao::get_chain_assets_by_address_chain_code_symbol(
            pool.read_ref(),
            address,
            chain_code,
            symbol,
            is_multisig,
        )
        .await
    }

    pub async fn delete_assets(
        pool: &ApiWalletDbPool,
        address: &str,
        chain_code: &str,
        token_key: AssetTokenKey,
    ) -> Result<(), crate::Error> {
        ApiAssetsDao::delete_assets(pool.write_ref(), address, chain_code, token_key.as_db_str())
            .await
    }

    pub async fn get_api_assets_by_address(
        pool: &ApiWalletDbPool,
        address: Vec<String>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<ApiAssetsEntityWithAddressType>, crate::Error> {
        ApiAssetsDao::get_api_assets_by_address(
            pool.read_ref(),
            address,
            None,
            None,
            None,
            is_multisig,
        )
        .await
    }

    pub async fn assets_with_wallet_address_by_address(
        pool: &ApiWalletDbPool,
        address: &[String],
    ) -> Result<Vec<AssetWithWalletAddress>, crate::Error> {
        ApiAssetsDao::assets_with_wallet_address_by_address(pool.read_ref(), address).await
    }

    pub async fn assets_with_wallet_address_by_token(
        pool: &ApiWalletDbPool,
        token: &[String],
    ) -> Result<Vec<AssetWithWalletAddress>, crate::Error> {
        ApiAssetsDao::assets_with_wallet_address_by_token(pool.read_ref(), token).await
    }

    pub async fn get_api_wallet_total_assets_v2(
        pool: &ApiWalletDbPool,
        wallet_address: Option<&str>,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<SumResult, crate::Error> {
        ApiAssetsDao::get_api_wallet_total_assets_v2(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await
    }

    pub async fn get_api_wallet_assets_v2(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
        hide_zero_balance: bool,
    ) -> Result<Vec<ApiAssertSummeryEntity>, crate::Error> {
        ApiAssetsDao::get_api_wallet_assets_v2(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
            hide_zero_balance,
        )
        .await
    }

    pub async fn get_api_wallet_total_assets_v3(
        pool: &ApiWalletDbPool,
        wallet_address: &str,
        account_id: Option<u32>,
        chain_code: Option<&str>,
    ) -> Result<Vec<AssetBalanceEntity>, crate::Error> {
        ApiAssetsDao::get_api_wallet_total_assets_v3(
            pool.read_ref(),
            wallet_address,
            account_id,
            chain_code,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiAssetsRepo;
    use crate::{
        SqlitePoolConfig,
        dao::api_assets::ApiAssetsDao,
        entities::{
            api_assets::ApiCreateAssetsVo,
            api_chain::{ApiChainCreateVo, NodeBindType},
            api_coin::ApiCoinData,
            asset_token_key::AssetTokenKey,
            assets::AssetsId,
        },
        repositories::{
            api_wallet::{chain::ApiChainRepo, coin::ApiCoinRepo},
            test_helper::{setup_api_wallet_pool, setup_api_wallet_pool_with_config},
        },
    };
    use chrono::Utc;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Barrier;

    pub(super) async fn seed_active_chain_and_coin(
        pool: &crate::ApiWalletDbPool,
        chain_code: &str,
        symbol: &str,
        token: Option<String>,
    ) {
        let protocols = vec!["evm".to_string()];
        let chain = ApiChainCreateVo::new(
            "Ethereum",
            chain_code,
            &protocols,
            NodeBindType::AutoLocal,
            "ETH",
        );
        ApiChainRepo::add(pool, chain).await.unwrap();

        let coin = ApiCoinData::new(
            Some(symbol.to_string()),
            symbol,
            chain_code,
            token.into(),
            Some("1".to_string()),
            None,
            6,
            1,
            1,
            1,
            Utc::now(),
            None,
        );
        ApiCoinRepo::upsert_multi_coin(pool, vec![coin]).await.unwrap();
    }

    fn make_asset(address: &str, token: Option<String>, balance: &str) -> ApiCreateAssetsVo {
        let id = AssetsId::new(address, wallet_types::constant::chain_code::ETHEREUM, token.into());
        ApiCreateAssetsVo::new(id, "USDT", 6, None, 0).with_name("usdt").with_balance(balance)
    }

    fn is_sqlite_locked(err: &crate::Error) -> bool {
        match err {
            crate::Error::Database(crate::DatabaseError::Sqlx(sqlx::Error::Database(db_err))) => {
                db_err.code().as_deref() == Some("5")
            }
            _ => false,
        }
    }

    #[tokio::test]
    async fn assets_repo_upsert_and_find_success() {
        let pool = setup_api_wallet_pool("wallet_db_api_assets_success").await;
        let address = "0xapi_assets_s_1";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;
        let token = Some("0xapi_assets_token_1".to_string());
        seed_active_chain_and_coin(&pool, chain_code, "USDT", token.clone()).await;

        ApiAssetsRepo::upsert_assets(&pool, make_asset(address, token.clone(), "12.5"))
            .await
            .unwrap();

        let id = AssetsId::new(address, chain_code, token.clone().into());
        let got = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap();
        assert_eq!(got.address, address);
        assert_eq!(got.chain_code, chain_code);
        assert_eq!(got.symbol, "USDT");
        assert_eq!(got.balance, "12.5");

        let rows =
            ApiAssetsRepo::list(&pool, vec![address.to_string()], Some(chain_code.to_string()))
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].address, address);
        assert_eq!(rows[0].balance, "12.5");
    }

    #[tokio::test]
    async fn assets_repo_missing_id_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_api_assets_edge").await;
        let id = AssetsId::new(
            "0xapi_assets_missing",
            wallet_types::constant::chain_code::ETHEREUM,
            Some("0xapi_assets_missing_token".to_string()).into(),
        );
        let got = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn assets_repo_tx_rollback_keeps_balance_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_api_assets_rollback").await;
        let address = "0xapi_assets_rb_1";
        let token = Some("0xapi_assets_rb_token_1".to_string());
        seed_active_chain_and_coin(
            &pool,
            wallet_types::constant::chain_code::ETHEREUM,
            "USDT",
            token.clone(),
        )
        .await;

        ApiAssetsRepo::upsert_assets(&pool, make_asset(address, token.clone(), "1")).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ApiAssetsDao::update_balance(
            tx.as_mut(),
            address,
            wallet_types::constant::chain_code::ETHEREUM,
            token.clone().into(),
            "99",
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let id = AssetsId::new(
            address,
            wallet_types::constant::chain_code::ETHEREUM,
            token.clone().into(),
        );
        let got = ApiAssetsRepo::find_by_id(&pool, &id).await.unwrap().unwrap();
        assert_eq!(got.balance, "1");

        let rows = ApiAssetsRepo::list(
            &pool,
            vec![address.to_string()],
            Some(wallet_types::constant::chain_code::ETHEREUM.to_string()),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].balance, "1");
    }

    #[tokio::test]
    async fn assets_repo_delete_assets_matches_by_token_key() {
        let pool = setup_api_wallet_pool("wallet_db_api_assets_delete_by_token_key").await;
        let address = "0xapi_assets_del_1";
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;
        let target_token = Some("0xapi_assets_del_token_1".to_string());
        let other_token = Some("0xapi_assets_del_token_2".to_string());

        seed_active_chain_and_coin(&pool, chain_code, "USDT", target_token.clone()).await;
        seed_active_chain_and_coin(&pool, chain_code, "USDT", other_token.clone()).await;

        ApiAssetsRepo::upsert_assets(&pool, make_asset(address, target_token.clone(), "12"))
            .await
            .unwrap();
        ApiAssetsRepo::upsert_assets(&pool, make_asset(address, other_token.clone(), "34"))
            .await
            .unwrap();

        ApiAssetsRepo::delete_assets(
            &pool,
            address,
            chain_code,
            AssetTokenKey::from_raw(target_token.as_deref()),
        )
        .await
        .unwrap();

        let target_status: i64 = sqlx::query_scalar(
            "SELECT status FROM api_assets WHERE address = ? AND chain_code = ? AND token_address = ?",
        )
        .bind(address)
        .bind(chain_code)
        .bind(target_token.as_deref().unwrap())
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        let other = ApiAssetsRepo::find_by_id(
            &pool,
            &AssetsId::new(address, chain_code, other_token.into()),
        )
        .await
        .unwrap()
        .unwrap();
        let listed =
            ApiAssetsRepo::list(&pool, vec![address.to_string()], Some(chain_code.to_string()))
                .await
                .unwrap();

        assert_eq!(target_status, 0);
        assert_eq!(other.status, 1);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].token_address.as_db_str(), "0xapi_assets_del_token_2");
    }

    #[tokio::test]
    async fn concurrent_balance_upserts_assets() {
        let chain_code = wallet_types::constant::chain_code::ETHEREUM;
        let address = "0xapi_assets_lock_1";
        let token = Some("0xapi_assets_lock_token_1".to_string());

        let cfg_multi = SqlitePoolConfig { reader_max_connections: 4, writer_max_connections: 4 };
        let pool_multi =
            setup_api_wallet_pool_with_config("wallet_db_api_assets_concurrent_multi", cfg_multi)
                .await;
        seed_active_chain_and_coin(&pool_multi, chain_code, "USDT", token.clone()).await;
        ApiAssetsRepo::upsert_assets(&pool_multi, make_asset(address, token.clone(), "10"))
            .await
            .unwrap();

        let gate = Arc::new(Barrier::new(2));
        let pool_hold = pool_multi.clone();
        let gate_hold = gate.clone();
        let token_hold = token.clone();
        let holder = tokio::spawn(async move {
            let mut tx = pool_hold.write_ref().begin().await.unwrap();
            ApiAssetsDao::update_balance(tx.as_mut(), address, chain_code, token_hold.into(), "20")
                .await
                .unwrap();
            gate_hold.wait().await;
            tokio::time::sleep(Duration::from_secs(6)).await;
            tx.commit().await.unwrap();
        });

        let pool_race = pool_multi.clone();
        let token_race = token.clone();
        let racer = tokio::spawn(async move {
            gate.wait().await;
            ApiAssetsRepo::upsert_assets_multi_update_balance(
                &pool_race,
                vec![make_asset(address, token_race, "30")],
            )
            .await
        });

        holder.await.unwrap();
        let race_res = racer.await.unwrap();
        assert!(race_res.as_ref().is_err_and(is_sqlite_locked));

        let pool_default = setup_api_wallet_pool("wallet_db_api_assets_concurrent_default").await;
        seed_active_chain_and_coin(&pool_default, chain_code, "USDT", token.clone()).await;
        ApiAssetsRepo::upsert_assets(&pool_default, make_asset(address, token.clone(), "10"))
            .await
            .unwrap();

        let gate_default = Arc::new(Barrier::new(2));
        let pool_hold_default = pool_default.clone();
        let gate_hold_default = gate_default.clone();
        let token_hold_default = token.clone();
        let holder_default = tokio::spawn(async move {
            let mut tx = pool_hold_default.write_ref().begin().await.unwrap();
            ApiAssetsDao::update_balance(
                tx.as_mut(),
                address,
                chain_code,
                token_hold_default.into(),
                "40",
            )
            .await
            .unwrap();
            gate_hold_default.wait().await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            tx.commit().await.unwrap();
        });

        let pool_race_default = pool_default.clone();
        let token_race_default = token.clone();
        let racer_default = tokio::spawn(async move {
            gate_default.wait().await;
            ApiAssetsRepo::upsert_assets_multi_update_balance(
                &pool_race_default,
                vec![make_asset(address, token_race_default, "50")],
            )
            .await
        });

        holder_default.await.unwrap();
        let ok_res = racer_default.await.unwrap();
        assert!(ok_res.is_ok());
        let id = AssetsId::new(address, chain_code, token.clone().into());
        let got = ApiAssetsRepo::find_by_id(&pool_default, &id).await.unwrap().unwrap();
        assert_eq!(got.balance, "50");
    }
}

#[cfg(test)]
#[path = "assets/insert_missing_tests.rs"]
mod insert_missing_tests;

#[cfg(test)]
#[path = "assets/token_case_tests.rs"]
mod token_case_tests;
