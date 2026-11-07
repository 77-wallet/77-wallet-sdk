use crate::{
    CoreDbPool,
    dao::node::NodeDao,
    entities::node::{NodeCreateVo, NodeEntity},
};

pub struct NodeRepo;

impl NodeRepo {
    pub async fn get_local_node_by_chain(
        pool: &CoreDbPool,
        chain_code: &str,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        Ok(NodeDao::list(pool.read_ref(), &[chain_code.to_string()], Some(1), Some(1)).await?)
    }

    pub async fn list(
        pool: &CoreDbPool,
        is_local: Option<u8>,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        Ok(NodeDao::list(pool.read_ref(), &[], is_local, None).await?)
    }

    pub async fn list_with_network(
        pool: &CoreDbPool,
        is_local: Option<u8>,
        network: Option<&str>,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        Ok(NodeDao::list_with_network(pool.read_ref(), &[], is_local, None, network).await?)
    }

    pub async fn list_by_chain_with_network(
        pool: &CoreDbPool,
        chain_code: &str,
        network: Option<&str>,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        Ok(NodeDao::list_with_network(
            pool.read_ref(),
            &[chain_code.to_string()],
            None,
            None,
            network,
        )
        .await?)
    }

    /// Local fallback creation must not revive or overwrite an existing node.
    pub async fn insert_missing(
        pool: &CoreDbPool,
        req: NodeCreateVo,
    ) -> Result<bool, crate::Error> {
        NodeDao::insert_missing(pool.write_ref(), req).await
    }

    pub async fn upsert(pool: &CoreDbPool, req: NodeCreateVo) -> Result<NodeEntity, crate::Error> {
        Ok(NodeDao::upsert(pool.write_ref(), req).await?)
    }

    pub async fn detail(
        pool: &CoreDbPool,
        node_id: &str,
    ) -> Result<Option<NodeEntity>, crate::Error> {
        let executor = pool.read_ref();
        Ok(NodeDao::detail_by_node_id(executor, node_id).await?)
    }

    pub async fn disable_backend_not_in(
        pool: &CoreDbPool,
        chain_code: &str,
        backend_ids: &[String],
    ) -> Result<u64, crate::Error> {
        let executor = pool.write_ref();
        NodeDao::disable_backend_not_in(executor, chain_code, backend_ids).await
    }

    pub async fn refresh_visibility_for_networks(
        pool: &CoreDbPool,
        visible_networks: &[&str],
    ) -> Result<u64, crate::Error> {
        let executor = pool.write_ref();
        NodeDao::refresh_visibility_for_networks(executor, visible_networks).await
    }

    pub async fn get_node_list_in_chain_codes(
        pool: &CoreDbPool,
        chain_codes: &[String],
        status: Option<u8>,
    ) -> Result<Vec<NodeEntity>, crate::Error> {
        Ok(NodeDao::list(pool.read_ref(), chain_codes, None, status).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::NodeRepo;
    use crate::{
        dao::node::NodeDao, entities::node::NodeCreateVo,
        repositories::test_helper::setup_core_pool,
    };

    fn build_node(node_id: &str, chain_code: &str) -> NodeCreateVo {
        NodeCreateVo::new(node_id, "node_name", chain_code, "https://rpc.test", None)
            .with_network("mainnet")
            .with_is_local(1)
    }

    #[tokio::test]
    async fn default_node_insert_does_not_revive_disabled_or_replace_existing_node() {
        let pool = setup_core_pool("default_node_insert_race").await;
        NodeRepo::upsert(&pool, build_node("fallback", "base").with_status(0)).await.unwrap();
        assert!(!NodeRepo::insert_missing(&pool, build_node("fallback", "base")).await.unwrap());
        let row = NodeRepo::detail(&pool, "fallback").await.unwrap().unwrap();
        assert_eq!(row.status, 0);
        assert_eq!(row.rpc_url, "https://rpc.test");
        let (a, b) = tokio::join!(
            NodeRepo::insert_missing(&pool, build_node("second", "scroll")),
            NodeRepo::insert_missing(&pool, build_node("second", "scroll"))
        );
        assert_ne!(a.unwrap(), b.unwrap());
        assert_eq!(NodeRepo::list(&pool, None).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn node_repo_upsert_and_detail_success() {
        let pool = setup_core_pool("wallet_db_node_repo_success").await;
        NodeRepo::upsert(&pool, build_node("node_success", "tron")).await.unwrap();

        let found = NodeRepo::detail(&pool, "node_success").await.unwrap().unwrap();
        assert_eq!(found.node_id, "node_success");
        assert_eq!(found.chain_code, "tron");
    }

    #[tokio::test]
    async fn node_repo_missing_node_returns_none() {
        let pool = setup_core_pool("wallet_db_node_repo_edge").await;
        let found = NodeRepo::detail(&pool, "node_missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn node_list_filters_chain_local_status_and_network() {
        let pool = setup_core_pool("wallet_db_node_list_filters").await;
        NodeRepo::upsert(&pool, build_node("tron_main", "tron")).await.unwrap();
        NodeRepo::upsert(&pool, build_node("eth_main", "eth")).await.unwrap();

        let nodes = NodeDao::list_with_network(
            pool.read_ref(),
            &["tron".to_string()],
            Some(1),
            Some(1),
            Some("mainnet"),
        )
        .await
        .unwrap();

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "tron_main");
    }

    #[tokio::test]
    async fn node_list_treats_sql_shaped_chain_as_data() {
        let pool = setup_core_pool("wallet_db_node_list_bind").await;
        NodeRepo::upsert(&pool, build_node("tron_bind", "tron")).await.unwrap();
        NodeRepo::upsert(&pool, build_node("eth_bind", "eth")).await.unwrap();

        let nodes = NodeRepo::get_node_list_in_chain_codes(
            &pool,
            &["tron') OR 1=1 --".to_string()],
            Some(1),
        )
        .await
        .unwrap();

        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn node_repo_tx_rollback_keeps_node_absent() {
        let pool = setup_core_pool("wallet_db_node_repo_rollback").await;

        let mut tx = pool.write_ref().begin().await.unwrap();
        NodeDao::upsert(tx.as_mut(), build_node("node_rb", "tron")).await.unwrap();
        tx.rollback().await.unwrap();

        let found = NodeRepo::detail(&pool, "node_rb").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn node_repo_refresh_visibility_respects_allowed_networks() {
        let pool = setup_core_pool("wallet_db_node_repo_visibility").await;
        NodeRepo::upsert(
            &pool,
            build_node("node_mainnet", "tron").with_network("mainnet").with_status(0),
        )
        .await
        .unwrap();
        NodeRepo::upsert(
            &pool,
            build_node("node_testnet", "tron").with_network("testnet").with_status(0),
        )
        .await
        .unwrap();

        NodeRepo::refresh_visibility_for_networks(&pool, &["mainnet", "testnet"]).await.unwrap();

        let nodes = NodeRepo::list(&pool, None).await.unwrap();
        let mainnet = nodes.iter().find(|node| node.node_id == "node_mainnet").unwrap();
        let testnet = nodes.iter().find(|node| node.node_id == "node_testnet").unwrap();
        assert_eq!(mainnet.status, 1);
        assert_eq!(testnet.status, 1);

        NodeRepo::refresh_visibility_for_networks(&pool, &["mainnet"]).await.unwrap();

        let nodes = NodeRepo::list(&pool, None).await.unwrap();
        let mainnet = nodes.iter().find(|node| node.node_id == "node_mainnet").unwrap();
        let testnet = nodes.iter().find(|node| node.node_id == "node_testnet").unwrap();
        assert_eq!(mainnet.status, 1);
        assert_eq!(testnet.status, 0);
    }

    #[tokio::test]
    async fn node_repo_refresh_visibility_does_not_touch_backend_nodes() {
        let pool = setup_core_pool("wallet_db_node_repo_visibility_backend").await;
        NodeRepo::upsert(
            &pool,
            build_node("node_local", "tron").with_network("mainnet").with_status(0),
        )
        .await
        .unwrap();
        NodeRepo::upsert(
            &pool,
            NodeCreateVo::new("node_backend", "node_name", "tron", "https://rpc.test", None)
                .with_network("testnet")
                .with_status(0)
                .with_is_local(0),
        )
        .await
        .unwrap();

        NodeRepo::refresh_visibility_for_networks(&pool, &["mainnet", "testnet"]).await.unwrap();

        let nodes = NodeRepo::list(&pool, None).await.unwrap();
        let local = nodes.iter().find(|node| node.node_id == "node_local").unwrap();
        let backend = nodes.iter().find(|node| node.node_id == "node_backend").unwrap();
        assert_eq!(local.status, 1);
        assert_eq!(backend.status, 0);

        NodeRepo::refresh_visibility_for_networks(&pool, &["mainnet"]).await.unwrap();

        let nodes = NodeRepo::list(&pool, None).await.unwrap();
        let local = nodes.iter().find(|node| node.node_id == "node_local").unwrap();
        let backend = nodes.iter().find(|node| node.node_id == "node_backend").unwrap();
        assert_eq!(local.status, 1);
        assert_eq!(backend.status, 0);
    }
}
