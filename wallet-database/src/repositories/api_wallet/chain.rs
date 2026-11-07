use crate::{
    ApiWalletDbPool, CoreDbPool,
    dao::api_chain::ApiChainDao,
    entities::api_chain::{ApiChainCreateVo, ApiChainEntity, ApiChainWithNode, NodeBindType},
    repositories::node::NodeRepo,
};

pub struct ApiChainRepo;

impl ApiChainRepo {
    pub async fn get_chain_list(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::list(pool.read_ref(), Some(1)).await?)
    }

    pub async fn detail_with_node(
        core_pool: &CoreDbPool,
        api_pool: &ApiWalletDbPool,
        chain_code: &str,
    ) -> Result<Option<ApiChainWithNode>, crate::Error> {
        let Some(chain) = ApiChainDao::detail(api_pool.read_ref(), chain_code).await? else {
            return Ok(None);
        };

        let Some(node_id) = chain.node_id.as_deref() else {
            return Ok(None);
        };

        let Some(node) = NodeRepo::detail(core_pool, node_id).await? else {
            return Ok(None);
        };

        Ok(Some(crate::entities::chain::ChainWithNode {
            name: chain.name,
            chain_code: chain.chain_code,
            main_symbol: chain.main_symbol,
            node_id: node.node_id,
            node_name: node.name,
            rpc_url: node.rpc_url,
            ws_url: node.ws_url,
            http_url: node.http_url,
            network: node.network,
            status: chain.status,
            created_at: chain.created_at,
            updated_at: chain.updated_at,
        }))
    }

    pub async fn detail(
        pool: &ApiWalletDbPool,
        chain_code: &str,
    ) -> Result<Option<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::detail(pool.read_ref(), chain_code).await?)
    }

    pub async fn add(pool: &ApiWalletDbPool, input: ApiChainCreateVo) -> Result<(), crate::Error> {
        Ok(ApiChainDao::upsert(pool.write_ref(), input).await?)
    }

    pub async fn insert_missing(
        pool: &ApiWalletDbPool,
        input: ApiChainCreateVo,
    ) -> Result<(), crate::Error> {
        ApiChainDao::insert_missing(pool.write_ref(), input).await
    }

    pub async fn detail_with_main_symbol(
        pool: &ApiWalletDbPool,
        main_symbol: &str,
    ) -> Result<Option<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::detail_with_main_symbol(pool.read_ref(), main_symbol).await?)
    }

    pub async fn toggle_chains_status(
        pool: &ApiWalletDbPool,
        chain_codes: &[String],
    ) -> Result<Vec<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::toggle_chains_status(pool.write_ref(), chain_codes).await?)
    }

    pub async fn toggle_chains_status_except(
        pool: &ApiWalletDbPool,
        chain_codes: &[String],
        preserved: &[String],
    ) -> Result<Vec<ApiChainEntity>, crate::Error> {
        ApiChainDao::toggle_chains_status_except(pool.write_ref(), chain_codes, preserved).await
    }

    pub async fn upsert_multi_chain(
        pool: &ApiWalletDbPool,
        input: Vec<ApiChainCreateVo>,
    ) -> Result<(), crate::Error> {
        ApiChainDao::upsert_multi_chain(pool.write_ref(), input).await
    }

    // pub async fn set_chain_node_id_empty(
    //     pool: &ApiWalletDbPool,
    //     node_id: &str,
    // ) -> Result<Vec<ApiChainEntity>, crate::Error> {
    //     ApiChainDao::set_chain_node_id_empty(pool.read_ref(), node_id).await
    // }

    pub async fn user_select(
        pool: &ApiWalletDbPool,
        chain_code: &str,
        node_id: &str,
    ) -> Result<(), crate::Error> {
        Ok(ApiChainDao::user_select(pool.write_ref(), chain_code, node_id).await?)
    }

    pub async fn set_api_chain_node(
        pool: &ApiWalletDbPool,
        chain_code: &str,
        node_id: &str,
    ) -> Result<Vec<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::set_api_chain_node(pool.write_ref(), chain_code, node_id).await?)
    }

    pub async fn set_chain_node_with_type(
        pool: &ApiWalletDbPool,
        chain_code: &str,
        node_id: &str,
        bind_type: NodeBindType,
    ) -> Result<(), crate::Error> {
        Ok(ApiChainDao::set_chain_node_with_type(pool.write_ref(), chain_code, node_id, bind_type)
            .await?)
    }

    pub async fn get_chain_node_list(
        core_pool: &CoreDbPool,
        api_pool: &ApiWalletDbPool,
    ) -> Result<Vec<ApiChainWithNode>, crate::Error> {
        let chains = ApiChainDao::list(api_pool.read_ref(), Some(1)).await?;
        if chains.is_empty() {
            return Ok(Vec::new());
        }

        let nodes = NodeRepo::list(core_pool, None)
            .await?
            .into_iter()
            .filter(|n| n.status == 1)
            .map(|n| (n.node_id.clone(), n))
            .collect::<std::collections::HashMap<_, _>>();

        let mut out = Vec::with_capacity(chains.len());
        for chain in chains {
            let Some(node_id) = chain.node_id.as_deref() else {
                continue;
            };
            let Some(node) = nodes.get(node_id) else {
                continue;
            };

            out.push(crate::entities::chain::ChainWithNode {
                name: chain.name,
                chain_code: chain.chain_code,
                main_symbol: chain.main_symbol,
                node_id: node.node_id.clone(),
                node_name: node.name.clone(),
                rpc_url: node.rpc_url.clone(),
                ws_url: node.ws_url.clone(),
                http_url: node.http_url.clone(),
                network: node.network.clone(),
                status: chain.status,
                created_at: chain.created_at,
                updated_at: chain.updated_at,
            });
        }

        Ok(out)
    }

    pub async fn get_chain_list_all_status(
        pool: &ApiWalletDbPool,
    ) -> Result<Vec<ApiChainEntity>, crate::Error> {
        Ok(ApiChainDao::list(pool.read_ref(), None).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiChainRepo;
    use crate::{
        dao::api_chain::ApiChainDao,
        entities::api_chain::{ApiChainCreateVo, NodeBindType},
        repositories::test_helper::setup_api_wallet_pool,
    };

    #[tokio::test]
    async fn default_status_refresh_preserves_a_backend_disable_after_snapshot() {
        let pool = setup_api_wallet_pool("api_chain_status_race").await;
        ApiChainRepo::add(&pool, make_chain("base", "ETH")).await.unwrap();
        ApiChainRepo::add(&pool, make_chain("legacy", "OLD")).await.unwrap();
        let earlier_enabled = vec!["base".to_owned()];
        ApiChainRepo::upsert_multi_chain(&pool, vec![make_chain("base", "ETH").with_status(0)])
            .await
            .unwrap();
        ApiChainRepo::toggle_chains_status_except(&pool, &earlier_enabled, &["base".into()])
            .await
            .unwrap();
        let rows = ApiChainRepo::get_chain_list_all_status(&pool).await.unwrap();
        assert!(rows.iter().all(|row| row.status == 0));
    }

    #[tokio::test]
    async fn default_chain_insert_does_not_overwrite_backend_disable() {
        let pool = setup_api_wallet_pool("api_chain_default_race").await;
        // The default loader may have observed absence before the backend write.
        ApiChainRepo::upsert_multi_chain(&pool, vec![make_chain("polygon", "POL").with_status(0)])
            .await
            .unwrap();
        ApiChainRepo::insert_missing(&pool, make_chain("polygon", "POL")).await.unwrap();
        assert_eq!(ApiChainRepo::get_chain_list_all_status(&pool).await.unwrap()[0].status, 0);
        let (backend, local) = tokio::join!(
            ApiChainRepo::upsert_multi_chain(&pool, vec![make_chain("base", "ETH").with_status(0)]),
            ApiChainRepo::insert_missing(&pool, make_chain("base", "ETH")),
        );
        backend.unwrap();
        local.unwrap();
        let rows = ApiChainRepo::get_chain_list_all_status(&pool).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.status == 0));
    }

    fn make_chain(chain_code: &str, symbol: &str) -> ApiChainCreateVo {
        let protocols = vec!["evm".to_string()];
        ApiChainCreateVo::new("chain_name", chain_code, &protocols, NodeBindType::AutoLocal, symbol)
    }

    #[tokio::test]
    async fn chain_repo_add_and_detail_success() {
        let pool = setup_api_wallet_pool("wallet_db_api_chain_success").await;
        let chain_code = "CHAIN_TEST_SUCCESS";
        let symbol = "TST";

        ApiChainRepo::add(&pool, make_chain(chain_code, symbol)).await.unwrap();
        let got = ApiChainRepo::detail(&pool, chain_code).await.unwrap();
        assert!(got.is_some());
        let got = got.unwrap();
        assert_eq!(got.chain_code, chain_code);
        assert_eq!(got.main_symbol, symbol);
        assert_eq!(got.status, 1);

        let all = ApiChainRepo::get_chain_list_all_status(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].chain_code, chain_code);
    }

    #[tokio::test]
    async fn chain_repo_missing_chain_returns_none() {
        let pool = setup_api_wallet_pool("wallet_db_api_chain_edge").await;
        let got = ApiChainRepo::detail(&pool, "CHAIN_TEST_MISSING").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn chain_repo_tx_rollback_keeps_status_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_api_chain_rollback").await;
        let chain_a = "CHAIN_TEST_RB_A";
        let chain_b = "CHAIN_TEST_RB_B";

        ApiChainRepo::add(&pool, make_chain(chain_a, "RBA")).await.unwrap();
        ApiChainRepo::add(&pool, make_chain(chain_b, "RBB")).await.unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        let touched =
            ApiChainDao::toggle_chains_status(tx.as_mut(), &[chain_a.to_string()]).await.unwrap();
        assert!(!touched.is_empty());
        tx.rollback().await.unwrap();

        let after_a = ApiChainRepo::detail(&pool, chain_a).await.unwrap().unwrap();
        assert_eq!(after_a.status, 1);

        let after_b = ApiChainRepo::detail(&pool, chain_b).await.unwrap();
        assert!(after_b.is_some());
        assert_eq!(after_b.unwrap().status, 1);
    }

    #[tokio::test]
    async fn api_chain_toggle_binds_chain_codes() {
        let pool = setup_api_wallet_pool("wallet_db_api_chain_toggle_bind").await;
        ApiChainRepo::add(&pool, make_chain("api_chain_bind_a", "BDA")).await.unwrap();
        ApiChainRepo::add(&pool, make_chain("api_chain_bind_b", "BDB")).await.unwrap();

        ApiChainRepo::toggle_chains_status(&pool, &["missing') OR 1=1 --".to_string()])
            .await
            .unwrap();

        for chain_code in ["api_chain_bind_a", "api_chain_bind_b"] {
            let status: i64 =
                sqlx::query_scalar("SELECT status FROM api_chain WHERE chain_code = ?")
                    .bind(chain_code)
                    .fetch_one(pool.read_ref())
                    .await
                    .unwrap();
            assert_eq!(status, 0);
        }
    }
}
