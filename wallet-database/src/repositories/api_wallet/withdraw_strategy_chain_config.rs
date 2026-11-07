use crate::{
    ApiWalletDbPool, dao::api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigDao,
    entities::api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigEntity,
};
pub struct ApiWithdrawStrategyChainConfigRepo;

impl ApiWithdrawStrategyChainConfigRepo {
    pub async fn upsert(
        pool: &ApiWalletDbPool,
        input: ApiWithdrawStrategyChainConfigEntity,
    ) -> Result<(), crate::Error> {
        ApiWithdrawStrategyChainConfigDao::upsert(pool.write_ref(), input).await
    }

    pub async fn get_by_strategy_id(
        pool: &ApiWalletDbPool,
        strategy_id: i64,
    ) -> Result<Vec<ApiWithdrawStrategyChainConfigEntity>, crate::Error> {
        ApiWithdrawStrategyChainConfigDao::get_chain_configs_by_strategy_id(
            pool.read_ref(),
            strategy_id,
        )
        .await
    }

    pub async fn delete_by_strategy_id(
        pool: &ApiWalletDbPool,
        strategy_id: i64,
    ) -> Result<(), crate::Error> {
        ApiWithdrawStrategyChainConfigDao::delete_chain_configs_by_strategy_id(
            pool.write_ref(),
            strategy_id,
        )
        .await
    }

    pub async fn delete_chain_config(
        pool: &ApiWalletDbPool,
        strategy_id: i64,
        chain_code: &str,
    ) -> Result<(), crate::Error> {
        ApiWithdrawStrategyChainConfigDao::delete_chain_config(
            pool.write_ref(),
            strategy_id,
            chain_code,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::ApiWithdrawStrategyChainConfigRepo;
    use crate::{
        dao::api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigDao,
        entities::{
            api_withdraw_strategy::ApiWithdrawStrategyEntity,
            api_withdraw_strategy_chain_config::ApiWithdrawStrategyChainConfigEntity,
        },
        repositories::{
            api_wallet::withdraw_strategy::ApiWithdrawStrategyRepo,
            test_helper::setup_api_wallet_pool,
        },
    };

    fn make_cfg(
        strategy_id: i64,
        chain_code: &str,
        normal: &str,
    ) -> ApiWithdrawStrategyChainConfigEntity {
        ApiWithdrawStrategyChainConfigEntity {
            id: 0,
            strategy_id,
            chain_code: chain_code.to_string(),
            chain_address_type: None,
            normal_idx: Some(3),
            normal_address: normal.to_string(),
            risk_idx: Some(4),
            risk_address: "risk_addr".to_string(),
            created_at: Default::default(),
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn withdraw_strategy_chain_config_repo_upsert_and_get_success() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_cfg_success").await;
        let strategy_uid = "withdraw_strategy_uid_for_cfg_s";
        ApiWithdrawStrategyRepo::upsert(
            &pool,
            ApiWithdrawStrategyEntity {
                id: 0,
                uid: strategy_uid.to_string(),
                threshold: 1,
                created_at: Default::default(),
                updated_at: None,
            },
        )
        .await
        .unwrap();
        let strategy_id =
            ApiWithdrawStrategyRepo::get_by_uid(&pool, strategy_uid).await.unwrap().unwrap().id;
        ApiWithdrawStrategyChainConfigRepo::upsert(
            &pool,
            make_cfg(strategy_id, wallet_types::constant::chain_code::ETHEREUM, "addr_normal"),
        )
        .await
        .unwrap();

        let got = ApiWithdrawStrategyChainConfigRepo::get_by_strategy_id(&pool, strategy_id)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].strategy_id, strategy_id);
        assert_eq!(got[0].chain_code, wallet_types::constant::chain_code::ETHEREUM);
        assert_eq!(got[0].normal_address, "addr_normal");
        assert_eq!(got[0].risk_address, "risk_addr");
    }

    #[tokio::test]
    async fn withdraw_strategy_chain_config_repo_missing_strategy_returns_empty() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_cfg_edge").await;
        let got =
            ApiWithdrawStrategyChainConfigRepo::get_by_strategy_id(&pool, 888_888).await.unwrap();
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn withdraw_strategy_chain_config_repo_tx_rollback_keeps_value_unchanged() {
        let pool = setup_api_wallet_pool("wallet_db_withdraw_cfg_rollback").await;
        let strategy_uid = "withdraw_strategy_uid_for_cfg_rb";
        let chain = wallet_types::constant::chain_code::ETHEREUM;
        ApiWithdrawStrategyRepo::upsert(
            &pool,
            ApiWithdrawStrategyEntity {
                id: 0,
                uid: strategy_uid.to_string(),
                threshold: 1,
                created_at: Default::default(),
                updated_at: None,
            },
        )
        .await
        .unwrap();
        let strategy_id =
            ApiWithdrawStrategyRepo::get_by_uid(&pool, strategy_uid).await.unwrap().unwrap().id;

        ApiWithdrawStrategyChainConfigRepo::upsert(&pool, make_cfg(strategy_id, chain, "old_addr"))
            .await
            .unwrap();

        let mut tx = pool.write_ref().begin().await.unwrap();
        ApiWithdrawStrategyChainConfigDao::upsert(
            tx.as_mut(),
            make_cfg(strategy_id, chain, "new_addr"),
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();

        let got = ApiWithdrawStrategyChainConfigRepo::get_by_strategy_id(&pool, strategy_id)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].strategy_id, strategy_id);
        assert_eq!(got[0].chain_code, chain);
        assert_eq!(got[0].normal_address, "old_addr");
    }
}
