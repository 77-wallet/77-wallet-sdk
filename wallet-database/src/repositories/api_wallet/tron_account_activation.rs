use crate::{
    ApiResourceDbPool,
    dao::api_tron_account_activation::ApiTronAccountActivationDao,
    entities::api_tron_account_activation::{
        ApiTronAccountActivationEntity, ApiTronAccountActivationRecoverStatus,
        NewApiTronAccountActivation,
    },
};

pub struct ApiTronAccountActivationRepo;

impl ApiTronAccountActivationRepo {
    pub async fn upsert(
        pool: &ApiResourceDbPool,
        input: NewApiTronAccountActivation,
    ) -> Result<(), crate::Error> {
        ApiTronAccountActivationDao::upsert(pool.write_ref(), input).await
    }

    pub async fn find_by_activation_trade_no(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
    ) -> Result<Option<ApiTronAccountActivationEntity>, crate::Error> {
        ApiTronAccountActivationDao::find_by_activation_trade_no(
            pool.read_ref(),
            activation_trade_no,
        )
        .await
    }

    pub async fn find_by_origin_target(
        pool: &ApiResourceDbPool,
        origin_trade_no: &str,
        origin_trade_type: i64,
        target_address: &str,
    ) -> Result<Option<ApiTronAccountActivationEntity>, crate::Error> {
        ApiTronAccountActivationDao::find_by_origin_target(
            pool.read_ref(),
            origin_trade_no,
            origin_trade_type,
            target_address,
        )
        .await
    }

    pub async fn mark_account_verified(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::mark_account_verified(pool.write_ref(), activation_trade_no)
            .await
    }

    pub async fn scan_can_execute(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiTronAccountActivationEntity>, crate::Error> {
        ApiTronAccountActivationDao::scan_can_execute(pool.read_ref(), limit).await
    }

    pub async fn scan_can_verify(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiTronAccountActivationEntity>, crate::Error> {
        ApiTronAccountActivationDao::scan_can_verify(pool.read_ref(), limit).await
    }

    pub async fn claim_build_slot(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::claim_build_slot(pool.write_ref(), activation_trade_no).await
    }

    pub async fn mark_broadcast_success(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::mark_broadcast_success(
            pool.write_ref(),
            activation_trade_no,
            tx_hash,
            raw_tx,
        )
        .await
    }

    pub async fn mark_retry_wait(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        recover_status: ApiTronAccountActivationRecoverStatus,
        next_retry_at: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::mark_retry_wait(
            pool.write_ref(),
            activation_trade_no,
            err_code,
            err_msg,
            recover_status,
            next_retry_at,
        )
        .await
    }

    pub async fn mark_chain_success(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::mark_chain_success(pool.write_ref(), activation_trade_no).await
    }

    pub async fn mark_chain_failed_retry(
        pool: &ApiResourceDbPool,
        activation_trade_no: &str,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        recover_status: ApiTronAccountActivationRecoverStatus,
        next_retry_at: &str,
    ) -> Result<u64, crate::Error> {
        ApiTronAccountActivationDao::mark_chain_failed_retry(
            pool.write_ref(),
            activation_trade_no,
            err_code,
            err_msg,
            recover_status,
            next_retry_at,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            api_trade_type::ApiTradeType,
            api_tron_account_activation::{
                ApiTronAccountActivationRecoverStatus, ApiTronAccountActivationStatus,
                NewApiTronAccountActivation,
            },
        },
        repositories::test_helper::setup_api_resource_pool,
    };

    #[tokio::test]
    async fn tron_account_activation_upsert_is_idempotent_by_origin_target() {
        let pool = setup_api_resource_pool("tron_account_activation_upsert").await;

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_1",
                "act_collect_1",
                "T_payer_1",
                "T_target_1",
            ),
        )
        .await
        .unwrap();
        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_1",
                "act_collect_1",
                "T_payer_2",
                "T_target_1",
            ),
        )
        .await
        .unwrap();

        let got = ApiTronAccountActivationRepo::find_by_origin_target(
            &pool,
            "collect_1",
            ApiTradeType::Collect as i64,
            "T_target_1",
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(got.activation_trade_no, "act_collect_1");
        assert_eq!(got.payer_address, "T_payer_2");
        assert_eq!(got.amount, "0.00001");
        assert_eq!(got.status, ApiTronAccountActivationStatus::Pending);
        assert!(got.account_verified_at.is_none());
    }

    #[tokio::test]
    async fn tron_account_activation_account_verified_is_idempotent() {
        let pool = setup_api_resource_pool("tron_account_activation_verified").await;

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_verified",
                "act_collect_verified",
                "T_payer",
                "T_target",
            ),
        )
        .await
        .unwrap();

        assert_eq!(
            ApiTronAccountActivationRepo::mark_account_verified(&pool, "act_collect_verified")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            ApiTronAccountActivationRepo::mark_account_verified(&pool, "act_collect_verified")
                .await
                .unwrap(),
            0
        );

        let got = ApiTronAccountActivationRepo::find_by_activation_trade_no(
            &pool,
            "act_collect_verified",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(got.status, ApiTronAccountActivationStatus::Success);
        assert!(got.account_verified_at.is_some());
    }

    #[tokio::test]
    async fn tron_account_activation_execute_and_verify_scans_are_fact_based() {
        let pool = setup_api_resource_pool("tron_account_activation_scans").await;

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_scan",
                "act_collect_scan",
                "T_payer",
                "T_target",
            ),
        )
        .await
        .unwrap();

        let executable = ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap();
        assert_eq!(executable.len(), 1);
        assert_eq!(executable[0].activation_trade_no, "act_collect_scan");

        assert_eq!(
            ApiTronAccountActivationRepo::claim_build_slot(&pool, "act_collect_scan")
                .await
                .unwrap(),
            1
        );
        assert!(
            ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap().is_empty()
        );

        assert_eq!(
            ApiTronAccountActivationRepo::mark_broadcast_success(
                &pool,
                "act_collect_scan",
                "tx_hash_scan",
                r#"{"raw":"tx"}"#,
            )
            .await
            .unwrap(),
            1
        );
        assert!(
            ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap().is_empty()
        );

        let verifiable = ApiTronAccountActivationRepo::scan_can_verify(&pool, 10).await.unwrap();
        assert_eq!(verifiable.len(), 1);
        assert_eq!(verifiable[0].tx_hash.as_deref(), Some("tx_hash_scan"));

        assert_eq!(
            ApiTronAccountActivationRepo::mark_chain_failed_retry(
                &pool,
                "act_collect_scan",
                Some("chain_failed"),
                Some("failed"),
                ApiTronAccountActivationRecoverStatus::RetryBuild,
                "1970-01-01T00:00:00Z",
            )
            .await
            .unwrap(),
            1
        );
        let retryable = ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap();
        assert_eq!(retryable.len(), 1);
        assert!(retryable[0].tx_hash.is_none());
        assert_eq!(retryable[0].tx_status.as_deref(), Some("fail"));
        assert_eq!(
            retryable[0].recover_status,
            Some(ApiTronAccountActivationRecoverStatus::RetryBuild)
        );
    }

    #[tokio::test]
    async fn tron_account_activation_build_claim_matches_resource_delegation_guard() {
        let pool = setup_api_resource_pool("tron_account_activation_build_claim_guard").await;

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_guard",
                "act_collect_guard",
                "T_payer",
                "T_target",
            ),
        )
        .await
        .unwrap();

        assert_eq!(
            ApiTronAccountActivationRepo::mark_broadcast_success(
                &pool,
                "act_collect_guard",
                "tx_hash_without_claim",
                r#"{"raw":"tx"}"#,
            )
            .await
            .unwrap(),
            0
        );

        assert_eq!(
            ApiTronAccountActivationRepo::claim_build_slot(&pool, "act_collect_guard")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            ApiTronAccountActivationRepo::claim_build_slot(&pool, "act_collect_guard")
                .await
                .unwrap(),
            0
        );

        sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET building_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-6 minutes')
            WHERE activation_trade_no = ?
            "#,
        )
        .bind("act_collect_guard")
        .execute(pool.as_ref())
        .await
        .unwrap();

        let executable = ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap();
        assert_eq!(executable.len(), 1);
        assert_eq!(executable[0].activation_trade_no, "act_collect_guard");

        assert_eq!(
            ApiTronAccountActivationRepo::claim_build_slot(&pool, "act_collect_guard")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn tron_account_activation_retry_wait_blocks_execute_and_verify_scans() {
        let pool = setup_api_resource_pool("tron_account_activation_retry_wait").await;

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_retry_execute",
                "act_collect_retry_execute",
                "T_payer",
                "T_target_execute",
            ),
        )
        .await
        .unwrap();
        ApiTronAccountActivationRepo::mark_retry_wait(
            &pool,
            "act_collect_retry_execute",
            Some("retry"),
            Some("wait"),
            ApiTronAccountActivationRecoverStatus::RetryBuild,
            "2999-01-01T00:00:00Z",
        )
        .await
        .unwrap();

        ApiTronAccountActivationRepo::upsert(
            &pool,
            NewApiTronAccountActivation::collect(
                "uid_1",
                "collect_retry_verify",
                "act_collect_retry_verify",
                "T_payer",
                "T_target_verify",
            ),
        )
        .await
        .unwrap();
        ApiTronAccountActivationRepo::claim_build_slot(&pool, "act_collect_retry_verify")
            .await
            .unwrap();
        ApiTronAccountActivationRepo::mark_broadcast_success(
            &pool,
            "act_collect_retry_verify",
            "tx_hash_retry_verify",
            r#"{"raw":"tx"}"#,
        )
        .await
        .unwrap();
        ApiTronAccountActivationRepo::mark_retry_wait(
            &pool,
            "act_collect_retry_verify",
            Some("retry"),
            Some("wait"),
            ApiTronAccountActivationRecoverStatus::RetryRecover,
            "2999-01-01T00:00:00Z",
        )
        .await
        .unwrap();

        assert!(
            ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap().is_empty()
        );
        assert!(ApiTronAccountActivationRepo::scan_can_verify(&pool, 10).await.unwrap().is_empty());

        sqlx::query(
            r#"
            UPDATE api_tron_account_activation
            SET next_retry_at = '1970-01-01T00:00:00Z'
            WHERE activation_trade_no IN (?, ?)
            "#,
        )
        .bind("act_collect_retry_execute")
        .bind("act_collect_retry_verify")
        .execute(pool.as_ref())
        .await
        .unwrap();

        assert_eq!(
            ApiTronAccountActivationRepo::scan_can_execute(&pool, 10).await.unwrap().len(),
            1
        );
        assert_eq!(
            ApiTronAccountActivationRepo::scan_can_verify(&pool, 10).await.unwrap().len(),
            1
        );
    }
}
