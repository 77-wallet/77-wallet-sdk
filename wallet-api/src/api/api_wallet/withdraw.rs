use crate::{
    api::ReturnType,
    manager::WalletManager,
    response_vo::api_wallet::withdraw::{ApiWithdrawOrderDetailVo, ApiWithdrawOrderVo},
    service::api_wallet::withdraw::WithdrawService,
};
use wallet_database::{
    entities::{api_withdraw::ApiWithdrawStatus, asset_token_key::AssetTokenKey},
    pagination::Pagination,
};

impl WalletManager {
    /// Pending review orders across active API withdrawal wallets.
    pub async fn get_pending_api_withdrawals(&self) -> ReturnType<Vec<ApiWithdrawOrderVo>> {
        WithdrawService::new(self.ctx).pending_withdrawals().await
    }

    pub async fn list_api_withdraw_order(&self, uid: &str) -> ReturnType<Vec<ApiWithdrawOrderVo>> {
        WithdrawService::new(self.ctx).list_withdraw_order(uid).await
    }

    pub async fn page_api_withdraw_order_with_init_status(
        &self,
        uid: &str,
        init_status: u8,
        status: Vec<u8>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<ApiWithdrawOrderVo>> {
        let s = status.iter().map(|it| ApiWithdrawStatus::try_from(it.clone()).unwrap()).collect();
        let init_status = ApiWithdrawStatus::try_from(init_status)?;
        WithdrawService::new(self.ctx)
            .page_withdraw_order_with_init_status(uid, init_status, s, page, page_size)
            .await
    }

    pub async fn page_api_withdraw_order(
        &self,
        uid: &str,
        status: Vec<u8>,
        page: i64,
        page_size: i64,
    ) -> ReturnType<Pagination<ApiWithdrawOrderVo>> {
        let s = status.iter().map(|it| ApiWithdrawStatus::try_from(it.clone()).unwrap()).collect();
        WithdrawService::new(self.ctx).page_withdraw_order(uid, s, page, page_size).await
    }

    pub async fn detail_api_withdraw_order(
        &self,
        trade_no: &str,
    ) -> ReturnType<ApiWithdrawOrderDetailVo> {
        WithdrawService::new(self.ctx).detail_withdraw_order(trade_no).await
    }

    // 测试
    pub async fn api_withdrawal_order(
        &self,
        from: &str,
        to: &str,
        value: &str,
        validate: &str,
        chain_code: &str,
        token_address: Option<String>,
        symbol: &str,
        trade_no: &str,
        trade_type: u8,
        uid: &str,
    ) -> ReturnType<()> {
        let token_key = AssetTokenKey::from_raw(token_address.as_deref());
        WithdrawService::new(self.ctx)
            .withdrawal_order(
                from, to, value, validate, chain_code, token_key, symbol, trade_no, trade_type,
                uid, 1,
            )
            .await
    }

    pub async fn sign_api_withdrawal_order(&self, trade_no: &str) -> ReturnType<()> {
        WithdrawService::new(self.ctx).sign_withdrawal_order(trade_no).await
    }

    pub async fn reject_api_withdrawal_order(&self, trade_no: &str) -> ReturnType<()> {
        WithdrawService::new(self.ctx).reject_withdrawal_order(trade_no).await
    }
}

#[cfg(test)]
mod unit_tests {
    use chrono::{Duration, Utc};
    use wallet_database::entities::{
        api_trade_type::ApiTradeType,
        api_withdraw::{ApiWithdrawEntity, ApiWithdrawStatus, WithdrawFailureStage},
        asset_token_key::AssetTokenKey,
    };

    use crate::response_vo::api_wallet::{
        withdraw::{ApiWithdrawOrderDetailVo, ApiWithdrawOrderVo},
        withdraw_display::FailureReasonDisplay,
    };

    fn make_entity(
        created_at: chrono::DateTime<Utc>,
        audit_passed_at: Option<chrono::DateTime<Utc>>,
        audit_rejected_at: Option<chrono::DateTime<Utc>>,
    ) -> ApiWithdrawEntity {
        ApiWithdrawEntity {
            id: 0,
            name: "test".to_string(),
            uid: "test_uid".to_string(),
            from_addr: "from".to_string(),
            to_addr: "to".to_string(),
            value: "100".to_string(),
            validate: "validate".to_string(),
            chain_code: "TRX".to_string(),
            token_addr: AssetTokenKey::Native,
            symbol: "USDT".to_string(),
            trade_no: "T2024000000000000001".to_string(),
            trade_type: ApiTradeType::Withdraw,
            init_status: ApiWithdrawStatus::Init,
            status: ApiWithdrawStatus::Init,
            nonce: 0,
            tx_hash: None,
            raw_tx: None,
            resource_consume: "0".to_string(),
            transaction_fee: "0".to_string(),
            estimated_transaction_fee: None,
            estimated_resource_consume: None,
            fee_estimated_at: None,
            transaction_time: None,
            block_height: None,
            notes: None,
            post_tx_count: 0,
            post_confirm_tx_count: 0,
            err_code: None,
            err_msg: None,
            resource_check_at: None,
            resource_gate_released_at: None,
            resource_gate_result: None,
            resource_block_reason: None,
            resource_dependency_trade_no: None,
            resource_dependency_type: None,
            tx_ack_sent_at: None,
            building_at: None,
            last_broadcast_at: None,
            broadcast_uncertain_since_at: None,
            broadcast_uncertain_retry_count: 0,
            broadcast_uncertain_last_checked_at: None,
            broadcast_uncertain_reconciled_at: None,
            broadcast_uncertain_rebroadcast_count: 0,
            tx_res_ack_sent_at: None,
            tx_res_received_at: None,
            tx_exec_receipt_uploaded_at: None,
            finished_at: None,
            audit_passed_at,
            audit_rejected_at,
            audit_reason: None,
            chain_success_at: None,
            chain_failed_at: None,
            failure_stage: None,
            created_at,
            updated_at: None,
            out_order_id: None,
            client_id: None,
            create_time: None,
        }
    }

    fn assert_internal_display_fields_absent(json: &serde_json::Value) {
        for field in [
            "estimatedTransactionFee",
            "estimatedResourceConsume",
            "estimatedBandwidthConsume",
            "estimatedEnergyConsume",
            "feeEstimatedAt",
            "transactionFeeDisplay",
            "bandwidthConsumeDisplay",
            "energyConsumeDisplay",
        ] {
            assert!(json.get(field).is_none(), "{field} should not be exposed");
        }
    }

    #[test]
    fn test_apply_time_from_created_at() {
        let now = Utc::now();
        let entity = make_entity(now, None, None);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.apply_time, now);
    }

    #[test]
    fn test_sign_time_from_audit_passed_at() {
        let now = Utc::now();
        let audit_time = now + Duration::hours(1);
        let entity = make_entity(now, Some(audit_time), None);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.sign_time, Some(audit_time));
    }

    #[test]
    fn test_sign_time_from_audit_rejected_at() {
        let now = Utc::now();
        let reject_time = now + Duration::hours(2);
        let entity = make_entity(now, None, Some(reject_time));
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.sign_time, Some(reject_time));
    }

    #[test]
    fn test_sign_time_audit_passed_preferred_over_rejected() {
        let now = Utc::now();
        let pass_time = now + Duration::hours(1);
        let reject_time = now + Duration::hours(2);
        let entity = make_entity(now, Some(pass_time), Some(reject_time));
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.sign_time, Some(pass_time));
    }

    #[test]
    fn test_sign_time_none_when_no_audit() {
        let now = Utc::now();
        let entity = make_entity(now, None, None);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.sign_time, None);
    }

    #[test]
    fn test_serde_camel_case_fields() {
        let now = Utc::now();
        let pass_time = now + Duration::hours(1);
        let entity = make_entity(now, Some(pass_time), None);
        let vo = ApiWithdrawOrderVo::from(entity);

        let json = serde_json::to_value(&vo).unwrap();
        assert!(json.get("applyTime").is_some(), "should have camelCase applyTime");
        assert!(json.get("signTime").is_some(), "should have camelCase signTime");
        assert!(json.get("apply_time").is_none(), "should NOT have snake_case apply_time");
        assert!(json.get("sign_time").is_none(), "should NOT have snake_case sign_time");
    }

    #[test]
    fn api_withdraw_order_list_vo_excludes_detail_only_fields() {
        let now = Utc::now();
        let mut entity = make_entity(now, None, None);
        entity.validate = "secret-validate".to_string();
        entity.audit_reason = Some("reject reason".to_string());
        entity.err_code = Some(wallet_database::entities::api_withdraw::ErrCode::UnknownError);
        entity.notes = Some("internal note".to_string());
        entity.transaction_fee = "1.23".to_string();
        entity.resource_consume = r#"{"net_used":10,"energy_used":20}"#.to_string();
        entity.block_height = Some("12345".to_string());
        entity.estimated_transaction_fee = Some("0.42".to_string());
        entity.estimated_resource_consume = Some(r#"{"bandwidth":1,"energy":2}"#.to_string());
        entity.fee_estimated_at = Some(now + Duration::minutes(3));
        entity.out_order_id = Some("merchant-order-1".to_string());
        entity.client_id = Some("client-1".to_string());
        entity.create_time = Some("2026-05-19 10:11:12".to_string());
        entity.chain_code = "tron".to_string();
        entity.token_addr =
            AssetTokenKey::Contract("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string());

        let json = serde_json::to_value(ApiWithdrawOrderVo::from(entity)).unwrap();

        assert_eq!(json.get("tradeNo").and_then(|v| v.as_str()), Some("T2024000000000000001"));
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("test"));
        assert_eq!(json.get("outOrderId").and_then(|v| v.as_str()), Some("merchant-order-1"));
        assert_eq!(json.get("clientId").and_then(|v| v.as_str()), Some("client-1"));
        assert_eq!(json.get("createTime").and_then(|v| v.as_str()), Some("2026-05-19 10:11:12"));
        assert_eq!(json.get("chainCode").and_then(|v| v.as_str()), Some("tron"));
        assert_eq!(
            json.get("tokenAddr").and_then(|v| v.as_str()),
            Some("TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf")
        );
        assert!(json.get("network").is_none());
        assert_eq!(json.get("blockHeight").and_then(|v| v.as_str()), Some("12345"));
        assert_eq!(json.get("transactionFee").and_then(|v| v.as_str()), Some("1.23"));
        assert_eq!(json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(10));
        assert_eq!(json.get("energyConsume").and_then(|v| v.as_u64()), Some(20));
        assert_internal_display_fields_absent(&json);
        assert!(json.get("validate").is_none());
        assert!(json.get("auditReason").is_none());
        assert!(json.get("errCode").is_none());
        assert!(json.get("notes").is_none());
    }

    #[test]
    fn api_withdraw_order_detail_vo_includes_audit_detail_fields() {
        let now = Utc::now();
        let mut entity = make_entity(now, None, Some(now + Duration::minutes(5)));
        entity.validate = "validate-payload".to_string();
        entity.audit_reason = Some("risk rejected".to_string());
        entity.err_code = Some(wallet_database::entities::api_withdraw::ErrCode::UnknownError);
        entity.err_msg = Some("backend failed".to_string());
        entity.notes = Some("operator note".to_string());
        entity.transaction_fee = "2.46".to_string();
        entity.resource_consume = r#"{"bandwidth":30,"energy":40}"#.to_string();
        entity.block_height = Some("67890".to_string());
        entity.estimated_transaction_fee = Some("0.84".to_string());
        entity.estimated_resource_consume = Some(r#"{"bandwidth":3,"energy":4}"#.to_string());
        entity.fee_estimated_at = Some(now + Duration::minutes(8));
        entity.out_order_id = Some("merchant-order-2".to_string());
        entity.client_id = Some("client-2".to_string());
        entity.create_time = Some("2026-05-19 13:14:15".to_string());
        entity.chain_code = "bnb".to_string();
        entity.token_addr =
            AssetTokenKey::Contract("0x55d398326f99059fF775485246999027B3197955".to_string());

        let json = serde_json::to_value(ApiWithdrawOrderDetailVo::from(entity)).unwrap();

        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("test"));
        assert_eq!(json.get("outOrderId").and_then(|v| v.as_str()), Some("merchant-order-2"));
        assert_eq!(json.get("clientId").and_then(|v| v.as_str()), Some("client-2"));
        assert_eq!(json.get("createTime").and_then(|v| v.as_str()), Some("2026-05-19 13:14:15"));
        assert_eq!(json.get("chainCode").and_then(|v| v.as_str()), Some("bnb"));
        assert_eq!(
            json.get("tokenAddr").and_then(|v| v.as_str()),
            Some("0x55d398326f99059fF775485246999027B3197955")
        );
        assert!(json.get("network").is_none());
        assert_eq!(json.get("validate").and_then(|v| v.as_str()), Some("validate-payload"));
        assert_eq!(json.get("auditReason").and_then(|v| v.as_str()), Some("risk rejected"));
        assert_eq!(json.get("errMsg").and_then(|v| v.as_str()), Some("backend failed"));
        assert_eq!(json.get("notes").and_then(|v| v.as_str()), Some("operator note"));
        assert_eq!(json.get("blockHeight").and_then(|v| v.as_str()), Some("67890"));
        assert_eq!(json.get("transactionFee").and_then(|v| v.as_str()), Some("2.46"));
        assert_eq!(json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(30));
        assert_eq!(json.get("energyConsume").and_then(|v| v.as_u64()), Some(40));
        assert_internal_display_fields_absent(&json);
        assert!(json.get("audit_reason").is_none());
    }

    #[test]
    fn api_withdraw_order_vo_exposes_tron_fee_display_fields() {
        let now = Utc::now();
        let mut entity = make_entity(now, None, None);
        entity.transaction_fee = "5.67".to_string();
        entity.resource_consume = r#"{"net_used":123,"energy_used":456}"#.to_string();
        entity.estimated_transaction_fee = Some("1.23".to_string());
        entity.estimated_resource_consume = Some(r#"{"bandwidth":12,"energy":34}"#.to_string());

        let list_json = serde_json::to_value(ApiWithdrawOrderVo::from(entity.clone())).unwrap();
        assert_eq!(list_json.get("transactionFee").and_then(|v| v.as_str()), Some("5.67"));
        assert_eq!(list_json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(123));
        assert_eq!(list_json.get("energyConsume").and_then(|v| v.as_u64()), Some(456));
        assert_internal_display_fields_absent(&list_json);

        let detail_json = serde_json::to_value(ApiWithdrawOrderDetailVo::from(entity)).unwrap();
        assert_eq!(detail_json.get("transactionFee").and_then(|v| v.as_str()), Some("5.67"));
        assert_eq!(detail_json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(123));
        assert_eq!(detail_json.get("energyConsume").and_then(|v| v.as_u64()), Some(456));
        assert_internal_display_fields_absent(&detail_json);
    }

    #[test]
    fn api_withdraw_order_vo_fee_display_reads_legacy_energy_only_actual_resource() {
        let now = Utc::now();
        let mut entity = make_entity(now, None, None);
        entity.transaction_fee = "0.344".to_string();
        entity.resource_consume = "70070".to_string();
        entity.estimated_transaction_fee = Some("1.10".to_string());
        entity.estimated_resource_consume =
            Some(r#"{"bandwidth":4891,"energy":70070}"#.to_string());
        entity.tx_hash = Some("legacy-resource-tx-hash".to_string());

        let list_json = serde_json::to_value(ApiWithdrawOrderVo::from(entity.clone())).unwrap();
        assert_eq!(list_json.get("transactionFee").and_then(|v| v.as_str()), Some("0.344"));
        assert!(list_json.get("bandwidthConsume").is_some_and(|v| v.is_null()));
        assert_eq!(list_json.get("energyConsume").and_then(|v| v.as_u64()), Some(70070));
        assert_internal_display_fields_absent(&list_json);

        let detail_json = serde_json::to_value(ApiWithdrawOrderDetailVo::from(entity)).unwrap();
        assert_eq!(detail_json.get("transactionFee").and_then(|v| v.as_str()), Some("0.344"));
        assert!(detail_json.get("bandwidthConsume").is_some_and(|v| v.is_null()));
        assert_eq!(detail_json.get("energyConsume").and_then(|v| v.as_u64()), Some(70070));
        assert_internal_display_fields_absent(&detail_json);
    }

    #[test]
    fn api_withdraw_order_vo_fee_display_falls_back_to_estimated_values() {
        let now = Utc::now();
        let mut entity = make_entity(now, None, None);
        entity.transaction_fee = "0".to_string();
        entity.resource_consume = "0".to_string();
        entity.estimated_transaction_fee = Some("1.10".to_string());
        entity.estimated_resource_consume =
            Some(r#"{"bandwidth":4891,"energy":70070}"#.to_string());

        let list_json = serde_json::to_value(ApiWithdrawOrderVo::from(entity.clone())).unwrap();
        assert_eq!(list_json.get("transactionFee").and_then(|v| v.as_str()), Some("1.10"));
        assert_eq!(list_json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(4891));
        assert_eq!(list_json.get("energyConsume").and_then(|v| v.as_u64()), Some(70070));
        assert_internal_display_fields_absent(&list_json);

        let detail_json = serde_json::to_value(ApiWithdrawOrderDetailVo::from(entity)).unwrap();
        assert_eq!(detail_json.get("transactionFee").and_then(|v| v.as_str()), Some("1.10"));
        assert_eq!(detail_json.get("bandwidthConsume").and_then(|v| v.as_u64()), Some(4891));
        assert_eq!(detail_json.get("energyConsume").and_then(|v| v.as_u64()), Some(70070));
        assert_internal_display_fields_absent(&detail_json);

        let mut entity = make_entity(now, None, None);
        entity.transaction_fee = "0".to_string();
        entity.block_height = Some("12345".to_string());
        entity.estimated_transaction_fee = Some("1.10".to_string());

        let actual_zero_json = serde_json::to_value(ApiWithdrawOrderVo::from(entity)).unwrap();
        assert_eq!(actual_zero_json.get("transactionFee").and_then(|v| v.as_str()), Some("0"));
    }

    #[test]
    fn test_failure_reason_display() {
        let now = Utc::now();

        let entity = make_entity(now, None, None);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, None);

        let mut entity = make_entity(now, None, None);
        entity.status = ApiWithdrawStatus::AuditReject;
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, Some(FailureReasonDisplay::AuditRejected));

        let mut entity = make_entity(now, None, None);
        entity.status = ApiWithdrawStatus::SendingTxFailedReport;
        entity.failure_stage = Some(WithdrawFailureStage::Broadcast);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, Some(FailureReasonDisplay::BroadcastFailed));

        let mut entity = make_entity(now, None, None);
        entity.status = ApiWithdrawStatus::ConfirmFailureReport;
        entity.err_code = Some(wallet_database::entities::api_withdraw::ErrCode::FeeInsufficient);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, Some(FailureReasonDisplay::ResourceFailed));

        let mut entity = make_entity(now, None, None);
        entity.status = ApiWithdrawStatus::SendingTxFailedReport;
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, None);

        let mut entity = make_entity(now, None, None);
        entity.chain_failed_at = Some(now);
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, Some(FailureReasonDisplay::ChainFailed));

        let mut entity = make_entity(now, None, None);
        entity.status = ApiWithdrawStatus::Success;
        let vo = ApiWithdrawOrderVo::from(entity);
        assert_eq!(vo.failure_reason_display, None);
    }
}
