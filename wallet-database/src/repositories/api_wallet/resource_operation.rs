use crate::{
    ApiResourceDbPool,
    dao::api_resource_operation::ApiResourceOperationDao,
    entities::api_resource_operation::{ApiResourceOperationEntity, NewApiResourceOperation},
};

pub struct ApiResourceOperationRepo;

impl ApiResourceOperationRepo {
    pub async fn upsert(
        pool: &ApiResourceDbPool,
        input: NewApiResourceOperation,
    ) -> Result<(), crate::Error> {
        ApiResourceOperationDao::upsert(pool.write_ref(), input).await
    }

    pub async fn get_by_resource_trade_no(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<ApiResourceOperationEntity, crate::Error> {
        ApiResourceOperationDao::get_by_resource_trade_no(pool.read_ref(), resource_trade_no).await
    }

    pub async fn get_by_hash_and_owner(
        pool: &ApiResourceDbPool,
        tx_hash: &str,
        owner_address: &str,
    ) -> Result<Option<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::get_by_hash_and_owner(pool.read_ref(), tx_hash, owner_address)
            .await
    }

    pub async fn record_client_broadcast_success(
        pool: &ApiResourceDbPool,
        input: NewApiResourceOperation,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
    ) -> Result<(), crate::Error> {
        ApiResourceOperationDao::record_client_broadcast_success(
            pool.write_ref(),
            input,
            tx_hash,
            raw_tx,
            transaction_fee,
        )
        .await
    }

    pub async fn mark_task_ack_sent(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_task_ack_sent(pool.write_ref(), resource_trade_no).await
    }

    pub async fn scan_need_task_ack(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_need_task_ack(pool.read_ref(), limit).await
    }

    pub async fn scan_can_build(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_can_build(pool.read_ref(), limit).await
    }

    pub async fn scan_can_broadcast(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_can_broadcast(pool.read_ref(), limit).await
    }

    pub async fn scan_need_recover(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_need_recover(pool.read_ref(), limit).await
    }

    pub async fn scan_need_tx_exec_receipt_upload(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_need_tx_exec_receipt_upload(pool.read_ref(), limit).await
    }

    pub async fn scan_need_result_ack(
        pool: &ApiResourceDbPool,
        limit: usize,
    ) -> Result<Vec<ApiResourceOperationEntity>, crate::Error> {
        ApiResourceOperationDao::scan_need_result_ack(pool.read_ref(), limit).await
    }

    pub async fn claim_building_at(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::claim_building_at(pool.write_ref(), resource_trade_no).await
    }

    pub async fn update_after_build(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        tx_hash: &str,
        raw_tx: &str,
        transaction_fee: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::update_after_build(
            pool.write_ref(),
            resource_trade_no,
            tx_hash,
            raw_tx,
            transaction_fee,
        )
        .await
    }

    pub async fn clear_building_at(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::clear_building_at(pool.write_ref(), resource_trade_no).await
    }

    pub async fn mark_broadcast_executed(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_broadcast_executed(pool.write_ref(), resource_trade_no).await
    }

    pub async fn confirm_transaction_time_if_absent(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        transaction_time: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::confirm_transaction_time_if_absent(
            pool.write_ref(),
            resource_trade_no,
            transaction_time,
        )
        .await
    }

    pub async fn mark_tx_exec_receipt_uploaded(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_tx_exec_receipt_uploaded(pool.write_ref(), resource_trade_no)
            .await
    }

    pub async fn mark_result_received(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        result_status: &str,
        fail_type: Option<i64>,
        err_code: Option<&str>,
        err_msg: Option<&str>,
        result_payload: Option<&str>,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_result_received(
            pool.write_ref(),
            resource_trade_no,
            result_status,
            fail_type,
            err_code,
            err_msg,
            result_payload,
        )
        .await
    }

    pub async fn mark_result_ack_sent(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_result_ack_sent(pool.write_ref(), resource_trade_no).await
    }

    pub async fn mark_failed_if_unfinished(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
        err_code: &str,
        err_msg: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_failed_if_unfinished(
            pool.write_ref(),
            resource_trade_no,
            err_code,
            err_msg,
        )
        .await
    }

    pub async fn mark_broadcast_uncertain_attempt(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::mark_broadcast_uncertain_attempt(
            pool.write_ref(),
            resource_trade_no,
        )
        .await
    }

    pub async fn invalidate_raw_tx(
        pool: &ApiResourceDbPool,
        resource_trade_no: &str,
    ) -> Result<u64, crate::Error> {
        ApiResourceOperationDao::invalidate_raw_tx(pool.write_ref(), resource_trade_no).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entities::{
            api_resource_operation::{
                ApiResourceOperationStatus, ApiResourceOperationTaskSource,
                ApiResourceOperationType,
            },
            api_resource_type::ApiResourceType,
        },
        repositories::test_helper::setup_api_resource_pool,
    };

    #[tokio::test]
    async fn resource_operation_upsert_supports_backend_stake_task() {
        let pool = setup_api_resource_pool("resource_operation_upsert").await;
        let input = NewApiResourceOperation::backend_stake("uid_1", "op_trade_1", "owner", "1000");

        ApiResourceOperationRepo::upsert(&pool, input.clone()).await.unwrap();
        ApiResourceOperationRepo::upsert(&pool, input).await.unwrap();

        let got =
            ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_trade_1").await.unwrap();
        assert_eq!(got.task_source, ApiResourceOperationTaskSource::Backend);
        assert_eq!(got.operation_type, ApiResourceOperationType::Stake);
        assert_eq!(got.resource_type, ApiResourceType::Energy);
        assert_eq!(got.status, ApiResourceOperationStatus::Pending);
        assert_eq!(got.amount, "1000");
    }

    #[tokio::test]
    async fn resource_operation_records_client_vote_for_hash_detail_lookup() {
        let pool = setup_api_resource_pool("resource_operation_client_vote_lookup").await;
        let input = NewApiResourceOperation::client_vote("uid_1", "vote_trade_1", "owner", "1010");

        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            "vote_hash",
            "raw_vote_tx",
            "0",
        )
        .await
        .unwrap();

        let got = ApiResourceOperationRepo::get_by_hash_and_owner(&pool, "vote_hash", "owner")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.task_source, ApiResourceOperationTaskSource::Client);
        assert_eq!(got.operation_type, ApiResourceOperationType::Vote);
        assert_eq!(got.tx_hash.as_deref(), Some("vote_hash"));
        assert_eq!(got.raw_tx.as_deref(), Some("raw_vote_tx"));
        assert!(got.last_broadcast_at.is_some());
        assert!(got.tx_status.is_none());
        assert!(got.result_status.is_none());
        assert_eq!(got.amount, "1010");

        let missing =
            ApiResourceOperationRepo::get_by_hash_and_owner(&pool, "vote_hash", "other_owner")
                .await
                .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn resource_operation_records_client_batch_delegate_for_hash_detail_lookup() {
        let pool = setup_api_resource_pool("resource_operation_client_batch_delegate_lookup").await;
        let mut input = NewApiResourceOperation::client(
            "uid_1",
            "delegate_trade_1",
            "owner",
            ApiResourceType::Energy,
            "2",
            ApiResourceOperationType::BatchDelegate,
        );
        input.receiver_address = Some("receiver".to_string());

        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            "delegate_hash",
            "raw_delegate_tx",
            "123",
        )
        .await
        .unwrap();

        let got = ApiResourceOperationRepo::get_by_hash_and_owner(&pool, "delegate_hash", "owner")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.operation_type, ApiResourceOperationType::BatchDelegate);
        assert_eq!(got.receiver_address.as_deref(), Some("receiver"));
        assert_eq!(got.amount, "2");
        assert_eq!(got.raw_tx.as_deref(), Some("raw_delegate_tx"));
        assert_eq!(got.transaction_fee.as_deref(), Some("123"));
    }

    #[tokio::test]
    async fn resource_operation_records_client_withdraw_reward_for_hash_detail_lookup() {
        let pool =
            setup_api_resource_pool("resource_operation_client_withdraw_reward_lookup").await;
        let input = NewApiResourceOperation::client_withdraw_reward(
            "uid_1",
            "reward_trade_1",
            "owner",
            "12.5",
        );

        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            "reward_hash",
            "raw_reward_tx",
            "0",
        )
        .await
        .unwrap();

        let got = ApiResourceOperationRepo::get_by_hash_and_owner(&pool, "reward_hash", "owner")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.task_source, ApiResourceOperationTaskSource::Client);
        assert_eq!(got.operation_type, ApiResourceOperationType::WithdrawReward);
        assert_eq!(got.tx_hash.as_deref(), Some("reward_hash"));
        assert!(got.last_broadcast_at.is_some());
        assert!(got.tx_status.is_none());
        assert!(got.result_status.is_none());
        assert_eq!(got.amount, "12.5");
    }

    #[tokio::test]
    async fn resource_operation_task_ack_is_idempotent() {
        let pool = setup_api_resource_pool("resource_operation_ack").await;
        let input = NewApiResourceOperation::backend_stake("uid_1", "op_trade_ack", "owner", "1");
        ApiResourceOperationRepo::upsert(&pool, input).await.unwrap();

        assert_eq!(
            ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_trade_ack").await.unwrap(),
            1
        );
        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_trade_ack")
            .await
            .unwrap();
        assert!(got.task_ack_sent_at.is_some());
    }

    #[tokio::test]
    async fn resource_operation_task_ack_scan_selects_unsent_backend_tasks() {
        let pool = setup_api_resource_pool("resource_operation_ack_scan").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_need_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_sent_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_sent_ack").await.unwrap();

        let rows = ApiResourceOperationRepo::scan_need_task_ack(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.into_iter().map(|row| row.resource_trade_no).collect();
        assert!(trade_nos.contains(&"op_need_ack".to_string()));
        assert!(!trade_nos.contains(&"op_sent_ack".to_string()));
    }

    #[tokio::test]
    async fn resource_operation_can_build_requires_task_ack_and_claims_once() {
        let pool = setup_api_resource_pool("resource_operation_can_build").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_no_ack", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_can_build", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_can_build").await.unwrap();

        let rows = ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_can_build"));
        assert!(!trade_nos.contains(&"op_no_ack"));

        assert_eq!(
            ApiResourceOperationRepo::claim_building_at(&pool, "op_can_build").await.unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::claim_building_at(&pool, "op_can_build").await.unwrap(),
            0
        );

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_can_build")
            .await
            .unwrap();
        assert!(got.building_at.is_some());
        let rows = ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_update_after_build_persists_raw_tx_once() {
        let pool = setup_api_resource_pool("resource_operation_after_build").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_after_build", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_after_build").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_after_build").await.unwrap();

        assert_eq!(
            ApiResourceOperationRepo::update_after_build(
                &pool,
                "op_after_build",
                "0xhash_1",
                "{\"raw\":\"tx_1\"}",
                "10",
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::update_after_build(
                &pool,
                "op_after_build",
                "0xhash_2",
                "{\"raw\":\"tx_2\"}",
                "20",
            )
            .await
            .unwrap(),
            0
        );

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_after_build")
            .await
            .unwrap();
        assert_eq!(got.raw_tx.as_deref(), Some("{\"raw\":\"tx_1\"}"));
        assert_eq!(got.tx_hash.as_deref(), Some("0xhash_1"));
        assert_eq!(got.transaction_fee.as_deref(), Some("10"));
        assert!(got.result_status.is_none());
        assert!(got.tx_exec_receipt_uploaded_at.is_none());

        let rows = ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_can_broadcast_after_build_and_marks_once() {
        let pool = setup_api_resource_pool("resource_operation_can_broadcast").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_can_broadcast", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_can_broadcast").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_can_broadcast").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_can_broadcast",
            "0xhash_1",
            "{\"raw\":\"tx_1\"}",
            "10",
        )
        .await
        .unwrap();

        let rows = ApiResourceOperationRepo::scan_can_broadcast(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_can_broadcast"));

        assert_eq!(
            ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_can_broadcast")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_can_broadcast")
                .await
                .unwrap(),
            0
        );

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_can_broadcast")
            .await
            .unwrap();
        assert!(got.last_broadcast_at.is_some());
        assert_eq!(got.raw_tx.as_deref(), Some("{\"raw\":\"tx_1\"}"));
        assert_eq!(got.tx_hash.as_deref(), Some("0xhash_1"));
        assert_eq!(got.transaction_fee.as_deref(), Some("10"));
        assert!(got.result_status.is_none());
        assert!(got.result_received_at.is_none());

        let rows = ApiResourceOperationRepo::scan_can_broadcast(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_need_recover_after_broadcast_and_confirms_once() {
        let pool = setup_api_resource_pool("resource_operation_recover").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_recover", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_recover").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_recover").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_recover",
            "0xhash_1",
            "{\"raw\":\"tx_1\"}",
            "10",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_recover").await.unwrap();

        let rows = ApiResourceOperationRepo::scan_need_recover(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_recover"));

        assert_eq!(
            ApiResourceOperationRepo::confirm_transaction_time_if_absent(
                &pool,
                "op_recover",
                "2026-05-04T00:00:00Z",
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::confirm_transaction_time_if_absent(
                &pool,
                "op_recover",
                "2026-05-05T00:00:00Z",
            )
            .await
            .unwrap(),
            0
        );

        let got =
            ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_recover").await.unwrap();
        assert_eq!(got.tx_status.as_deref(), Some("success"));
        assert!(got.transaction_time.is_some());
        assert!(got.result_status.is_none());
        assert!(got.tx_exec_receipt_uploaded_at.is_none());

        let rows = ApiResourceOperationRepo::scan_need_recover(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_receipt_upload_scan_and_mark_are_idempotent() {
        let pool = setup_api_resource_pool("resource_operation_receipt_upload").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_receipt", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_receipt").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_receipt").await.unwrap();
        ApiResourceOperationRepo::update_after_build(
            &pool,
            "op_receipt",
            "0xhash_1",
            "{\"raw\":\"tx_1\"}",
            "10",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_receipt").await.unwrap();
        ApiResourceOperationRepo::confirm_transaction_time_if_absent(
            &pool,
            "op_receipt",
            "2026-05-04T00:00:00Z",
        )
        .await
        .unwrap();

        let rows =
            ApiResourceOperationRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_receipt"));

        assert_eq!(
            ApiResourceOperationRepo::mark_tx_exec_receipt_uploaded(&pool, "op_receipt")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::mark_tx_exec_receipt_uploaded(&pool, "op_receipt")
                .await
                .unwrap(),
            0
        );

        let got =
            ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_receipt").await.unwrap();
        assert!(got.tx_exec_receipt_uploaded_at.is_some());

        let rows =
            ApiResourceOperationRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_failure_fact_is_idempotent_and_receiptable() {
        let pool = setup_api_resource_pool("resource_operation_failure_fact").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_failed", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_failed").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_failed").await.unwrap();

        assert_eq!(
            ApiResourceOperationRepo::mark_failed_if_unfinished(
                &pool,
                "op_failed",
                "ERR_6008",
                "invalid amount",
            )
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::mark_failed_if_unfinished(
                &pool,
                "op_failed",
                "500",
                "second error",
            )
            .await
            .unwrap(),
            0
        );

        let got =
            ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_failed").await.unwrap();
        assert_eq!(got.err_code.as_deref(), Some("ERR_6008"));
        assert_eq!(got.err_msg.as_deref(), Some("invalid amount"));
        assert_eq!(got.tx_status.as_deref(), Some("fail"));
        assert!(got.building_at.is_none());
        assert!(got.transaction_time.is_none());

        let rows =
            ApiResourceOperationRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_failed"));
    }

    #[tokio::test]
    async fn resource_operation_result_ack_scan_and_mark_are_idempotent() {
        let pool = setup_api_resource_pool("resource_operation_result_ack").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_result_ack", "owner", "1"),
        )
        .await
        .unwrap();

        assert_eq!(
            ApiResourceOperationRepo::mark_result_received(
                &pool,
                "op_result_ack",
                "success",
                Some(0),
                None,
                None,
                Some("{\"status\":true}"),
            )
            .await
            .unwrap(),
            1
        );

        let rows = ApiResourceOperationRepo::scan_need_result_ack(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_result_ack"));

        assert_eq!(
            ApiResourceOperationRepo::mark_result_ack_sent(&pool, "op_result_ack").await.unwrap(),
            1
        );
        assert_eq!(
            ApiResourceOperationRepo::mark_result_ack_sent(&pool, "op_result_ack").await.unwrap(),
            0
        );

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_result_ack")
            .await
            .unwrap();
        assert_eq!(got.result_status.as_deref(), Some("success"));
        assert!(got.result_received_at.is_some());
        assert!(got.result_ack_sent_at.is_some());
        assert_eq!(got.result_payload.as_deref(), Some("{\"status\":true}"));

        let rows = ApiResourceOperationRepo::scan_need_result_ack(&pool, 100).await.unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn resource_operation_shadow_scans_ignore_client_source_tasks() {
        let pool = setup_api_resource_pool("resource_operation_client_source_guard").await;
        let client_input = NewApiResourceOperation {
            uid: "uid_1".to_string(),
            task_source: ApiResourceOperationTaskSource::Client,
            operation_type: ApiResourceOperationType::Stake,
            resource_trade_no: "op_client_source".to_string(),
            chain_code: "tron".to_string(),
            owner_address: "owner".to_string(),
            receiver_address: None,
            resource_type: ApiResourceType::Energy,
            amount: "1".to_string(),
        };
        ApiResourceOperationRepo::upsert(&pool, client_input).await.unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_client_source").await.unwrap();

        let can_build = ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap();
        assert!(!can_build.iter().any(|row| row.resource_trade_no == "op_client_source"));
        assert_eq!(
            ApiResourceOperationRepo::claim_building_at(&pool, "op_client_source").await.unwrap(),
            0
        );

        sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET raw_tx = '{"raw":"tx_1"}',
                tx_hash = '0xhash_1',
                transaction_fee = '10'
            WHERE resource_trade_no = ?
            "#,
        )
        .bind("op_client_source")
        .execute(pool.write_ref())
        .await
        .unwrap();

        let can_broadcast = ApiResourceOperationRepo::scan_can_broadcast(&pool, 100).await.unwrap();
        assert!(!can_broadcast.iter().any(|row| row.resource_trade_no == "op_client_source"));
        assert_eq!(
            ApiResourceOperationRepo::mark_broadcast_executed(&pool, "op_client_source")
                .await
                .unwrap(),
            0
        );
        sqlx::query(
            r#"
            UPDATE api_resource_operation
            SET transaction_time = '2026-05-04T00:00:00Z'
            WHERE resource_trade_no = ?
            "#,
        )
        .bind("op_client_source")
        .execute(pool.write_ref())
        .await
        .unwrap();
        let need_receipt =
            ApiResourceOperationRepo::scan_need_tx_exec_receipt_upload(&pool, 100).await.unwrap();
        assert!(!need_receipt.iter().any(|row| row.resource_trade_no == "op_client_source"));
        assert_eq!(
            ApiResourceOperationRepo::mark_tx_exec_receipt_uploaded(&pool, "op_client_source")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            ApiResourceOperationRepo::mark_result_received(
                &pool,
                "op_client_source",
                "success",
                Some(0),
                None,
                None,
                Some("{\"status\":true}"),
            )
            .await
            .unwrap(),
            0
        );
        let need_result_ack =
            ApiResourceOperationRepo::scan_need_result_ack(&pool, 100).await.unwrap();
        assert!(!need_result_ack.iter().any(|row| row.resource_trade_no == "op_client_source"));
        assert_eq!(
            ApiResourceOperationRepo::mark_result_ack_sent(&pool, "op_client_source")
                .await
                .unwrap(),
            0
        );

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_client_source")
            .await
            .unwrap();
        assert!(got.building_at.is_none());
        assert!(got.last_broadcast_at.is_none());
        assert!(got.tx_exec_receipt_uploaded_at.is_none());
        assert!(got.result_received_at.is_none());
        assert!(got.result_ack_sent_at.is_none());
    }

    #[tokio::test]
    async fn client_resource_operation_broadcast_persists_fact() {
        let pool = setup_api_resource_pool("client_resource_operation_broadcast").await;
        let input = NewApiResourceOperation::client(
            "uid_1",
            "client_resource_1",
            "owner",
            ApiResourceType::Energy,
            "1000",
            ApiResourceOperationType::Stake,
        );

        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input.clone(),
            "0xhash_1",
            "{\"raw\":\"tx_1\"}",
            "10",
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::record_client_broadcast_success(
            &pool,
            input,
            "0xhash_1",
            "{\"raw\":\"tx_1\"}",
            "10",
        )
        .await
        .unwrap();

        let got = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "client_resource_1")
            .await
            .unwrap();
        assert_eq!(got.task_source, ApiResourceOperationTaskSource::Client);
        assert_eq!(got.operation_type, ApiResourceOperationType::Stake);
        assert_eq!(got.resource_type, ApiResourceType::Energy);
        assert_eq!(got.amount, "1000");
        assert_eq!(got.tx_hash.as_deref(), Some("0xhash_1"));
        assert_eq!(got.raw_tx.as_deref(), Some("{\"raw\":\"tx_1\"}"));
        assert_eq!(got.transaction_fee.as_deref(), Some("10"));
        assert!(got.last_broadcast_at.is_some());
        assert!(got.tx_status.is_none());
        assert!(got.result_status.is_none());

        assert!(ApiResourceOperationRepo::scan_need_task_ack(&pool, 100).await.unwrap().is_empty());
        assert!(ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap().is_empty());
        assert!(ApiResourceOperationRepo::scan_can_broadcast(&pool, 100).await.unwrap().is_empty());
        assert!(
            ApiResourceOperationRepo::scan_need_result_ack(&pool, 100).await.unwrap().is_empty()
        );
    }

    #[tokio::test]
    async fn resource_operation_clear_building_at_releases_pre_raw_tx_slot() {
        let pool = setup_api_resource_pool("resource_operation_clear_building").await;
        ApiResourceOperationRepo::upsert(
            &pool,
            NewApiResourceOperation::backend_stake("uid_1", "op_clear_building", "owner", "1"),
        )
        .await
        .unwrap();
        ApiResourceOperationRepo::mark_task_ack_sent(&pool, "op_clear_building").await.unwrap();
        ApiResourceOperationRepo::claim_building_at(&pool, "op_clear_building").await.unwrap();

        let before = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_clear_building")
            .await
            .unwrap();
        assert!(before.building_at.is_some());

        assert_eq!(
            ApiResourceOperationRepo::clear_building_at(&pool, "op_clear_building").await.unwrap(),
            1
        );

        let after = ApiResourceOperationRepo::get_by_resource_trade_no(&pool, "op_clear_building")
            .await
            .unwrap();
        assert!(after.building_at.is_none());
        assert!(after.raw_tx.is_none());
        assert!(after.tx_hash.is_none());
        assert!(after.err_code.is_none());

        let rows = ApiResourceOperationRepo::scan_can_build(&pool, 100).await.unwrap();
        let trade_nos: Vec<_> = rows.iter().map(|row| row.resource_trade_no.as_str()).collect();
        assert!(trade_nos.contains(&"op_clear_building"));
    }
}
