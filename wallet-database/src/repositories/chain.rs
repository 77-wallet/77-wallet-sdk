use crate::{
    CoreDbPool,
    dao::chain::ChainDao,
    entities::chain::{ChainCreateVo, ChainEntity, ChainWithNode},
};

pub struct ChainRepo;

impl ChainRepo {
    pub async fn detail(
        pool: &CoreDbPool,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::Error> {
        Ok(ChainDao::detail(pool.read_ref(), chain_code).await?)
    }

    pub async fn delete(
        pool: &CoreDbPool,
        chain_code: &str,
    ) -> Result<Option<ChainEntity>, crate::Error> {
        Ok(ChainDao::delete(pool.write_ref(), chain_code).await?)
    }

    pub async fn insert_missing(
        pool: &CoreDbPool,
        input: ChainCreateVo,
    ) -> Result<bool, crate::Error> {
        ChainDao::insert_missing(pool.write_ref(), input).await
    }

    pub async fn add(pool: &CoreDbPool, input: ChainCreateVo) -> Result<ChainEntity, crate::Error> {
        Ok(ChainDao::upsert(pool.write_ref(), input).await?)
    }

    pub async fn get_chain_list(pool: &CoreDbPool) -> Result<Vec<ChainEntity>, crate::Error> {
        Ok(ChainDao::list(pool.read_ref(), Some(1)).await?)
    }

    pub async fn get_chain_list_v2(pool: &CoreDbPool) -> Result<Vec<ChainEntity>, crate::Error> {
        Ok(ChainDao::list_v2(pool.read_ref(), Some(1)).await?)
    }

    pub async fn get_chain_node_list(
        pool: &CoreDbPool,
    ) -> Result<Vec<ChainWithNode>, crate::Error> {
        Ok(ChainDao::list_with_node_info(pool.read_ref()).await?)
    }

    pub async fn detail_with_node(
        pool: &CoreDbPool,
        chain_code: &str,
    ) -> Result<Option<ChainWithNode>, crate::Error> {
        Ok(ChainDao::chain_node_info(pool.read_ref(), chain_code).await?)
    }

    pub async fn detail_with_main_symbol(
        pool: &CoreDbPool,
        main_symbol: &str,
    ) -> Result<Option<ChainEntity>, crate::Error> {
        Ok(ChainDao::detail_with_main_symbol(pool.read_ref(), main_symbol).await?)
    }

    pub async fn toggle_chains_status(
        pool: &CoreDbPool,
        chain_codes: &[String],
    ) -> Result<Vec<ChainEntity>, crate::Error> {
        Ok(ChainDao::toggle_chains_status(pool.write_ref(), chain_codes).await?)
    }

    pub async fn upsert_multi_chain(
        pool: &CoreDbPool,
        input: Vec<ChainCreateVo>,
    ) -> Result<(), crate::Error> {
        ChainDao::upsert_multi_chain(pool.write_ref(), input).await
    }

    pub async fn user_select(
        pool: &CoreDbPool,
        chain_code: &str,
        node_id: &str,
    ) -> Result<(), crate::Error> {
        Ok(ChainDao::user_select(pool.write_ref(), chain_code, node_id).await?)
    }

    pub async fn set_chain_node_with_type(
        pool: &crate::CoreDbPool,
        chain_code: &str,
        node_id: &str,
        bind_type: crate::entities::api_chain::NodeBindType,
    ) -> Result<(), crate::Error> {
        Ok(ChainDao::set_chain_node_with_type(pool.write_ref(), chain_code, node_id, bind_type)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::ChainRepo;
    use crate::{
        dao::chain::ChainDao,
        entities::{api_chain::NodeBindType, chain::ChainCreateVo},
        repositories::test_helper::setup_core_pool,
    };

    fn build_chain(chain_code: &str) -> ChainCreateVo {
        ChainCreateVo::new(
            "Tron",
            chain_code,
            &[String::from("tron")],
            NodeBindType::AutoBackend,
            "TRX",
        )
    }

    #[tokio::test]
    async fn default_insert_preserves_manual_disabled_row_and_backend_write_races() {
        let pool = setup_core_pool("five_chain_default_insert_guard").await;
        ChainRepo::add(
            &pool,
            ChainCreateVo::new("Backend Polygon", "polygon", &[], NodeBindType::AutoBackend, "POL")
                .with_status(0),
        )
        .await
        .unwrap();
        ChainRepo::user_select(&pool, "polygon", "manual-selected").await.unwrap();
        sqlx::query(
            "UPDATE chain SET updated_at='2000-01-01T00:00:00Z' WHERE chain_code='polygon'",
        )
        .execute(pool.write_ref())
        .await
        .unwrap();
        let before = sqlx::query_as::<_, crate::entities::chain::ChainEntity>(
            "SELECT * FROM chain WHERE chain_code='polygon'",
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        assert!(
            !ChainRepo::insert_missing(
                &pool,
                ChainCreateVo::new("Local Polygon", "polygon", &[], NodeBindType::AutoLocal, "POL")
            )
            .await
            .unwrap()
        );
        let after = sqlx::query_as::<_, crate::entities::chain::ChainEntity>(
            "SELECT * FROM chain WHERE chain_code='polygon'",
        )
        .fetch_one(pool.read_ref())
        .await
        .unwrap();
        assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(after).unwrap());
        let (backend, local) = tokio::join!(
            ChainRepo::upsert_multi_chain(
                &pool,
                vec![
                    ChainCreateVo::new(
                        "Backend Base",
                        "base",
                        &[],
                        NodeBindType::AutoBackend,
                        "ETH"
                    )
                    .with_status(0)
                ]
            ),
            ChainRepo::insert_missing(
                &pool,
                ChainCreateVo::new("Local Base", "base", &[], NodeBindType::AutoLocal, "ETH")
            )
        );
        backend.unwrap();
        local.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT status FROM chain WHERE chain_code='base'")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            0
        );
        let (a, b) = tokio::join!(
            ChainRepo::insert_missing(&pool, build_chain("scroll")),
            ChainRepo::insert_missing(&pool, build_chain("scroll"))
        );
        assert_ne!(a.unwrap(), b.unwrap());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chain WHERE chain_code='scroll'")
                .fetch_one(pool.read_ref())
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn chain_repo_add_and_detail_success() {
        let pool = setup_core_pool("wallet_db_chain_repo_success").await;
        ChainRepo::add(&pool, build_chain("tron_success")).await.unwrap();

        let found = ChainRepo::detail(&pool, "tron_success").await.unwrap().unwrap();
        assert_eq!(found.chain_code, "tron_success");
        assert_eq!(found.main_symbol, "TRX");
    }

    #[tokio::test]
    async fn chain_repo_missing_chain_returns_none() {
        let pool = setup_core_pool("wallet_db_chain_repo_edge").await;
        let found = ChainRepo::detail(&pool, "chain_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn chain_repo_tx_rollback_keeps_chain_absent() {
        let pool = setup_core_pool("wallet_db_chain_repo_rollback").await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        ChainDao::upsert(tx.as_mut(), build_chain("tron_rb")).await.unwrap();
        tx.rollback().await.unwrap();

        let found = ChainRepo::detail(&pool, "tron_rb").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn chain_collection_queries_bind_chain_codes() {
        let pool = setup_core_pool("wallet_db_chain_collection_bind").await;
        ChainRepo::add(&pool, build_chain("chain_bind_a")).await.unwrap();
        ChainRepo::add(&pool, build_chain("chain_bind_b")).await.unwrap();
        let sql_shaped = "missing') OR 1=1 --";

        ChainRepo::toggle_chains_status(&pool, &[sql_shaped.to_string()]).await.unwrap();
        let active = ChainDao::get_chain_list_in_chain_code(pool.read_pool(), vec![sql_shaped])
            .await
            .unwrap();

        assert!(active.is_empty());
        for chain_code in ["chain_bind_a", "chain_bind_b"] {
            let status: i64 = sqlx::query_scalar("SELECT status FROM chain WHERE chain_code = ?")
                .bind(chain_code)
                .fetch_one(pool.read_ref())
                .await
                .unwrap();
            assert_eq!(status, 0);
        }
    }

    #[tokio::test]
    async fn active_chain_collection_lookup_returns_selected_chain() {
        let pool = setup_core_pool("wallet_db_chain_collection_success").await;
        ChainRepo::add(&pool, build_chain("chain_selected")).await.unwrap();

        let active =
            ChainDao::get_chain_list_in_chain_code(pool.read_pool(), vec!["chain_selected"])
                .await
                .unwrap();

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].chain_code, "chain_selected");
    }
}
