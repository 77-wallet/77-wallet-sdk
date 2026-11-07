use crate::{
    CoreDbPool,
    dao::assets::{AssetsDao, CreateAssetsVo},
    entities::assets::{AssetsEntity, AssetsEntityWithAddressType, AssetsId},
};
use sqlx::{Sqlite, Transaction};

#[cfg(test)]
#[path = "assets/default_assets_tests.rs"]
mod ordinary_default_assets_tests;

pub struct AssetsRepo;

impl AssetsRepo {
    pub async fn pending_identity_refreshes(
        pool: &CoreDbPool,
        wallet: &str,
        account_id: Option<u32>,
        chain: Option<&str>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::pending_identity_refreshes(pool.read_ref(), wallet, account_id, chain).await
    }

    pub async fn mark_identity_refresh_dispatched(
        pool: &CoreDbPool,
        ids: &[AssetsId],
    ) -> Result<(), crate::Error> {
        AssetsDao::mark_identity_refresh_dispatched(pool.write_ref(), ids).await
    }
    pub async fn backend_balance_assets(
        pool: &CoreDbPool,
        address: Option<&str>,
        wallet: Option<&str>,
        chain: Option<&str>,
        account_id: Option<u32>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::backend_balance_assets(pool.read_ref(), address, wallet, chain, account_id).await
    }

    /// Add missing default assets without updating existing or hidden rows.
    pub async fn reconcile_ordinary_default_assets(pool: &CoreDbPool) -> Result<u64, crate::Error> {
        let upper = {
            let mut conn = pool.write_ref().acquire().await.map_err(crate::DatabaseError::from)?;
            AssetsDao::ordinary_default_asset_upper_bounds(&mut conn).await?
        };
        let mut cursor = (0, 0);
        let mut inserted = 0;
        loop {
            // Release the gate/connection after each bounded write. Cancellation
            // leaves completed pages durable and the next run starts idempotently.
            let _guard = pool.lock_write_with_metric("ordinary_default_assets").await;
            let mut conn = pool.write_ref().acquire().await.map_err(crate::DatabaseError::from)?;
            let candidates =
                AssetsDao::ordinary_default_asset_candidates(&mut conn, cursor, upper).await?;
            let Some(last) = candidates.last().copied() else {
                break;
            };
            inserted +=
                AssetsDao::insert_ordinary_default_asset_candidates(&mut conn, &candidates).await?;
            cursor = last;
        }
        Ok(inserted)
    }

    pub async fn get_coin_assets_in_address(
        pool: &CoreDbPool,
        address: Vec<String>,
        status: Option<u8>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::get_coin_assets_in_address(pool.read_ref(), address, status).await
    }

    pub async fn get_assets_by_address(
        pool: &CoreDbPool,
        address: Vec<String>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntityWithAddressType>, crate::Error> {
        AssetsDao::get_assets_by_address(pool.read_ref(), address, None, None, None, is_multisig)
            .await
    }

    pub async fn assets_by_id(
        pool: &CoreDbPool,
        assets_id: &AssetsId,
    ) -> Result<Option<AssetsEntity>, crate::Error> {
        AssetsDao::assets_by_id(pool.read_ref(), assets_id).await
    }

    pub async fn upsert_assets(
        pool: &CoreDbPool,
        assets: CreateAssetsVo,
    ) -> Result<(), crate::Error> {
        AssetsDao::upsert_assets(pool.write_ref(), assets).await
    }

    pub async fn all_assets(
        pool: &CoreDbPool,
        addr: Vec<String>,
        chain_code: Option<String>,
        keyword: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::all_assets(pool.read_ref(), addr, chain_code, keyword, is_multisig).await
    }

    pub async fn update_balance(
        pool: &CoreDbPool,
        id: &AssetsId,
        balance: &str,
    ) -> Result<(), crate::Error> {
        AssetsDao::update_balance(pool.write_ref(), id, balance).await
    }

    pub async fn delete_multi_assets(
        pool: &CoreDbPool,
        assets_ids: Vec<AssetsId>,
    ) -> Result<(), crate::Error> {
        AssetsDao::delete_multi_assets(pool.write_ref(), assets_ids).await
    }

    pub async fn update_balance_with_executor(
        tx: &mut Transaction<'_, Sqlite>,
        id: &AssetsId,
        balance: &str,
    ) -> Result<(), crate::Error> {
        AssetsDao::update_balance(tx.as_mut(), id, balance).await
    }

    pub async fn list_by_chain_token_map_batch(
        pool: &CoreDbPool,
        chain_list: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::list_by_chain_token_map_batch(pool.read_ref(), chain_list).await
    }

    pub async fn get_chain_assets_by_address_chain_code_symbol(
        pool: &CoreDbPool,
        address: Vec<String>,
        chain_code: Option<String>,
        symbol: Option<&str>,
        is_multisig: Option<bool>,
    ) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::get_chain_assets_by_address_chain_code_symbol(
            pool.read_ref(),
            address,
            chain_code,
            symbol,
            is_multisig,
        )
        .await
    }

    pub async fn get_by_addr_token(
        pool: &CoreDbPool,
        chain_code: &str,
        token_key: crate::entities::asset_token_key::AssetTokenKey,
        address: &str,
    ) -> Result<AssetsEntity, crate::Error> {
        let token_for_log = token_key.as_db_str().to_string();
        AssetsDao::get_by_addr_token(pool.read_ref(), chain_code, token_key.as_db_str(), address)
            .await?
            .ok_or(crate::Error::NotFound(format!(
                "asset not found chain_code {}, token_address {}, address {}",
                chain_code, token_for_log, address
            )))
    }

    // option 类型
    pub async fn get_by_addr_token_opt(
        pool: &CoreDbPool,
        chain_code: &str,
        token_key: crate::entities::asset_token_key::AssetTokenKey,
        address: &str,
    ) -> Result<Option<AssetsEntity>, crate::Error> {
        AssetsDao::get_by_addr_token(pool.read_ref(), chain_code, token_key.as_db_str(), address)
            .await
    }

    // repair
    pub async fn all_error_wsol(pool: &CoreDbPool) -> Result<Vec<AssetsEntity>, crate::Error> {
        AssetsDao::error_wsol_assets(pool.read_ref()).await
    }

    pub async fn repair_wsol_error(pool: &CoreDbPool) -> Result<(), crate::Error> {
        AssetsDao::delete_error_wsol_assets(pool.write_ref()).await
    }

    pub async fn update_tron_multisig_assets(
        pool: &CoreDbPool,
        address: &str,
        chain_code: &str,
        is_multisig: i8,
    ) -> Result<(), crate::Error> {
        AssetsDao::update_tron_multisig_assets(address, chain_code, is_multisig, pool.write_ref())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::AssetsRepo;
    use crate::{
        entities::{api_chain::NodeBindType, assets::AssetsId, chain::ChainCreateVo},
        repositories::{
            chain::ChainRepo,
            test_helper::{seed_account, seed_assets, seed_wallet, setup_core_pool},
        },
    };

    async fn seed_active_tron(pool: &crate::CoreDbPool) {
        ChainRepo::add(
            pool,
            ChainCreateVo::new(
                "Tron",
                wallet_types::constant::chain_code::TRON,
                &["tron".to_string()],
                NodeBindType::AutoBackend,
                "TRX",
            ),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn assets_upsert_update_and_query_consistent() {
        let pool = setup_core_pool("wallet_db_assets_success").await;
        let assets_id = AssetsId::new(
            "T_assets_owner_1",
            wallet_types::constant::chain_code::TRON,
            Some("token_assets_1".to_string()).into(),
        );
        seed_assets(&pool, assets_id.clone(), "TRX", "Tron", 6, "1.00").await;

        let mut chain_map = std::collections::HashMap::new();
        chain_map
            .insert(assets_id.chain_code.clone(), assets_id.token_address.as_db_str().to_string());

        let before = AssetsRepo::list_by_chain_token_map_batch(&pool, &chain_map)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(before.balance, "1.00");

        AssetsRepo::update_balance(&pool, &assets_id, "2.50").await.unwrap();
        let after = AssetsRepo::list_by_chain_token_map_batch(&pool, &chain_map)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(after.balance, "2.50");
    }

    #[tokio::test]
    async fn assets_query_missing_returns_none() {
        let pool = setup_core_pool("wallet_db_assets_edge").await;
        let missing = AssetsRepo::assets_by_id(
            &pool,
            &AssetsId::new(
                "T_assets_missing",
                wallet_types::constant::chain_code::TRON,
                Some("token_assets_missing".to_string()).into(),
            ),
        )
        .await
        .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn assets_update_with_tx_rollback_restores_balance() {
        let pool = setup_core_pool("wallet_db_assets_rollback").await;
        let assets_id = AssetsId::new(
            "T_assets_owner_rb",
            wallet_types::constant::chain_code::TRON,
            Some("token_assets_rb".to_string()).into(),
        );
        seed_assets(&pool, assets_id.clone(), "TRX", "Tron", 6, "7.77").await;

        let mut chain_map = std::collections::HashMap::new();
        chain_map
            .insert(assets_id.chain_code.clone(), assets_id.token_address.as_db_str().to_string());

        let mut tx = pool.write_ref().begin().await.unwrap();
        AssetsRepo::update_balance_with_executor(&mut tx, &assets_id, "9.99").await.unwrap();
        tx.rollback().await.unwrap();

        let after = AssetsRepo::list_by_chain_token_map_batch(&pool, &chain_map)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(after.balance, "7.77");
    }

    #[tokio::test]
    async fn asset_collection_reads_bind_address_selectors() {
        let pool = setup_core_pool("wallet_db_assets_address_bind").await;
        seed_active_tron(&pool).await;
        let quoted_address = "T_assets_'_quoted";
        seed_assets(
            &pool,
            AssetsId::new(
                quoted_address,
                wallet_types::constant::chain_code::TRON,
                Some("token_quote".to_string()).into(),
            ),
            "QUOTE",
            "Quoted",
            6,
            "1.00",
        )
        .await;

        let exact = AssetsRepo::get_coin_assets_in_address(
            &pool,
            vec![quoted_address.to_string()],
            Some(1),
        )
        .await
        .unwrap();
        let sql_shaped = vec!["missing') OR 1=1 --".to_string()];
        let coin = AssetsRepo::get_coin_assets_in_address(&pool, sql_shaped.clone(), Some(1))
            .await
            .unwrap();
        let chain = AssetsRepo::get_chain_assets_by_address_chain_code_symbol(
            &pool,
            sql_shaped.clone(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let all = AssetsRepo::all_assets(&pool, sql_shaped, None, None, None).await.unwrap();

        assert_eq!(exact.len(), 1);
        assert!(coin.is_empty());
        assert!(chain.is_empty());
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn all_assets_binds_chain_and_keyword_filters() {
        let pool = setup_core_pool("wallet_db_assets_filter_bind").await;
        seed_active_tron(&pool).await;
        seed_assets(
            &pool,
            AssetsId::new(
                "T_assets_filter",
                wallet_types::constant::chain_code::TRON,
                Some("token_filter".to_string()).into(),
            ),
            "FILTER",
            "Filter",
            6,
            "1.00",
        )
        .await;

        let by_chain = AssetsRepo::all_assets(
            &pool,
            vec![],
            Some("missing' OR 1=1 --".to_string()),
            None,
            None,
        )
        .await
        .unwrap();
        let by_keyword =
            AssetsRepo::all_assets(&pool, vec![], None, Some("%' OR 1=1 --"), None).await.unwrap();

        assert!(by_chain.is_empty());
        assert!(by_keyword.is_empty());
    }

    #[tokio::test]
    async fn joined_asset_read_binds_address_selectors() {
        let pool = setup_core_pool("wallet_db_assets_join_bind").await;
        seed_active_tron(&pool).await;
        seed_wallet(&pool, "wallet_join", "uid_join", "wallet").await;
        seed_account(
            &pool,
            1,
            "T_assets_join",
            "wallet_join",
            wallet_types::constant::chain_code::TRON,
        )
        .await;
        seed_assets(
            &pool,
            AssetsId::new(
                "T_assets_join",
                wallet_types::constant::chain_code::TRON,
                Some("token_join".to_string()).into(),
            ),
            "JOIN",
            "Join",
            6,
            "1.00",
        )
        .await;

        let result = AssetsRepo::get_assets_by_address(
            &pool,
            vec!["missing') OR 1=1 --".to_string()],
            Some(false),
        )
        .await
        .unwrap();

        assert!(result.is_empty());
    }
}
