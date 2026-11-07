use wallet_api::testkit::{
    collect::{
        eval_collect_resource_gate_via_worker, send_resource_result_ack_via_worker,
        upload_resource_tx_exec_receipt_via_worker,
    },
    mqtt::{api_resource_pool, api_transaction_pool},
};
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool, ApiWalletDbPool,
    entities::{
        api_collect::{ApiCollectEntity, ApiCollectStatus, ErrCode},
        api_resource_gate::{
            ApiResourceBlockReason, ApiResourceDependencyType, ApiResourceGateResult,
        },
        api_trade_type::ApiTradeType,
    },
    repositories::api_wallet::{
        collect::ApiCollectRepo, resource_delegation::ApiResourceDelegationRepo,
    },
};

use crate::harness::{
    AssertRole, GivenRole, LoadRole, SeedRole, ThenRole, WhenRole,
    decrypt_captured_api_backend_body, ensure_worker_env, open_api_wallet_pool,
    pop_request_with_retry, worker::WorkerTestEnv,
};

use super::{
    db::{
        mark_collect_chain_success, seed_blocked_collect, seed_failed_resource_receipt_row,
        seed_resource_result, seed_same_address_collect_resource_window,
        seed_tron_collect_resource_gate_basics, seed_tron_native_collect_order,
        seed_tron_token_collect_with_fee_funding_wallet,
    },
    fixtures::CollectResourceGateFixture,
};

pub(crate) struct CollectResourceGateScenario {
    env: &'static WorkerTestEnv,
    collect_pool: ApiTransactionDbPool,
    resource_pool: ApiResourceDbPool,
    wallet_pool: ApiWalletDbPool,
}

impl CollectResourceGateScenario {
    pub(crate) async fn new() -> Self {
        let env = ensure_worker_env().await;
        env.recorder.reset();

        let collect_pool = api_transaction_pool(env.ctx()).expect("api transaction pool");
        let resource_pool = api_resource_pool(env.ctx()).expect("api resource pool");
        let wallet_pool = open_api_wallet_pool(&env.db_dir).await;

        Self { env, collect_pool, resource_pool, wallet_pool }
    }

    fn seed(&self) -> SeedRole<'_, Self> {
        SeedRole::new(self)
    }

    fn load(&self) -> LoadRole<'_, Self> {
        LoadRole::new(self)
    }

    fn assert(&self) -> AssertRole<'_, Self> {
        AssertRole::new(self)
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectResourceGateGiven {
    async fn blocked_collect(&self, fixture: &CollectResourceGateFixture);

    async fn successful_collect_resource_result(&self, fixture: &CollectResourceGateFixture);

    async fn failed_collect_resource_result(&self, fixture: &CollectResourceGateFixture);

    async fn successful_withdraw_origin_resource_result(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn collect_chain_success(&self, fixture: &CollectResourceGateFixture);

    async fn failed_collect_resource_receipt(&self, fixture: &CollectResourceGateFixture);

    async fn failed_resource_receipt_without_origin_trade(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn same_address_collect_with_unconsumed_platform_resource(
        &self,
        first: &CollectResourceGateFixture,
        second: &CollectResourceGateFixture,
    );

    async fn tron_native_collect_with_insufficient_amount(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn tron_token_collect_with_insufficient_fee_funding(
        &self,
        fixture: &CollectResourceGateFixture,
        funding_addr: &str,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectResourceGateGiven for GivenRole<'_, CollectResourceGateScenario> {
    async fn blocked_collect(&self, fixture: &CollectResourceGateFixture) {
        self.scenario().seed().blocked_collect(fixture).await;
    }

    async fn successful_collect_resource_result(&self, fixture: &CollectResourceGateFixture) {
        self.scenario().seed().collect_resource_result(fixture, true).await;
    }

    async fn failed_collect_resource_result(&self, fixture: &CollectResourceGateFixture) {
        self.scenario().seed().collect_resource_result(fixture, false).await;
    }

    async fn successful_withdraw_origin_resource_result(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        self.scenario().seed().withdraw_origin_resource_result(fixture).await;
    }

    async fn collect_chain_success(&self, fixture: &CollectResourceGateFixture) {
        self.scenario().seed().collect_chain_success(fixture).await;
    }

    async fn failed_collect_resource_receipt(&self, fixture: &CollectResourceGateFixture) {
        self.scenario().seed().failed_collect_resource_receipt(fixture).await;
    }

    async fn failed_resource_receipt_without_origin_trade(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        self.scenario().seed().failed_resource_receipt_without_origin_trade(fixture).await;
    }

    async fn same_address_collect_with_unconsumed_platform_resource(
        &self,
        first: &CollectResourceGateFixture,
        second: &CollectResourceGateFixture,
    ) {
        self.scenario()
            .seed()
            .same_address_collect_with_unconsumed_platform_resource(first, second)
            .await;
    }

    async fn tron_native_collect_with_insufficient_amount(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        self.scenario().seed().tron_native_collect_with_insufficient_amount(fixture).await;
    }

    async fn tron_token_collect_with_insufficient_fee_funding(
        &self,
        fixture: &CollectResourceGateFixture,
        funding_addr: &str,
    ) {
        self.scenario()
            .seed()
            .tron_token_collect_with_insufficient_fee_funding(fixture, funding_addr)
            .await;
    }
}

#[async_trait::async_trait(?Send)]
trait CollectResourceGateSeed {
    async fn blocked_collect(&self, fixture: &CollectResourceGateFixture);

    async fn collect_resource_result(&self, fixture: &CollectResourceGateFixture, success: bool);

    async fn withdraw_origin_resource_result(&self, fixture: &CollectResourceGateFixture);

    async fn collect_chain_success(&self, fixture: &CollectResourceGateFixture);

    async fn failed_collect_resource_receipt(&self, fixture: &CollectResourceGateFixture);

    async fn failed_resource_receipt_without_origin_trade(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn same_address_collect_with_unconsumed_platform_resource(
        &self,
        first: &CollectResourceGateFixture,
        second: &CollectResourceGateFixture,
    );

    async fn tron_native_collect_with_insufficient_amount(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn tron_token_collect_with_insufficient_fee_funding(
        &self,
        fixture: &CollectResourceGateFixture,
        funding_addr: &str,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectResourceGateSeed for SeedRole<'_, CollectResourceGateScenario> {
    async fn blocked_collect(&self, fixture: &CollectResourceGateFixture) {
        seed_blocked_collect(&self.scenario().collect_pool, &fixture.trade_no).await;
    }

    async fn collect_resource_result(&self, fixture: &CollectResourceGateFixture, success: bool) {
        seed_resource_result(
            &self.scenario().collect_pool,
            &self.scenario().resource_pool,
            &fixture.trade_no,
            &fixture.resource_trade_no,
            ApiTradeType::Collect,
            success,
        )
        .await;
    }

    async fn withdraw_origin_resource_result(&self, fixture: &CollectResourceGateFixture) {
        seed_resource_result(
            &self.scenario().collect_pool,
            &self.scenario().resource_pool,
            "W_ORIGIN_SKIP",
            &fixture.resource_trade_no,
            ApiTradeType::Withdraw,
            true,
        )
        .await;
    }

    async fn collect_chain_success(&self, fixture: &CollectResourceGateFixture) {
        mark_collect_chain_success(&self.scenario().collect_pool, &fixture.trade_no).await;
    }

    async fn failed_collect_resource_receipt(&self, fixture: &CollectResourceGateFixture) {
        seed_failed_resource_receipt_row(
            &self.scenario().resource_pool,
            Some(&fixture.trade_no),
            &fixture.resource_trade_no,
        )
        .await;
    }

    async fn failed_resource_receipt_without_origin_trade(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        seed_failed_resource_receipt_row(
            &self.scenario().resource_pool,
            None,
            &fixture.resource_trade_no,
        )
        .await;
    }

    async fn same_address_collect_with_unconsumed_platform_resource(
        &self,
        first: &CollectResourceGateFixture,
        second: &CollectResourceGateFixture,
    ) {
        let uid = "uid_collect_resource_gate_overlap";
        seed_tron_collect_resource_gate_basics(&self.scenario().wallet_pool, uid).await;
        self.scenario().env.recorder.succeed_next_api_backend_call_with_data(serde_json::json!({
            "uid": uid,
            "threshold": 0,
            "chainConfigs": [{
                "chainCode": "tron",
                "chainAddressType": null,
                "normalAddress": {
                    "index": 0,
                    "address": "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5"
                },
                "riskAddress": {
                    "index": 0,
                    "address": "TW6h166qfNfibxgovAnVyDDMNV1BFXp5A5"
                }
            }]
        }));
        self.scenario().env.recorder.succeed_next_api_backend_call_with_data(serde_json::json!({
            "dlRes": true,
            "dlTradeNo": second.resource_trade_no,
        }));
        seed_same_address_collect_resource_window(
            &self.scenario().wallet_pool,
            &self.scenario().collect_pool,
            &first.trade_no,
            &second.trade_no,
            "TOverlap111111111111111111111111111111",
            uid,
        )
        .await;
    }

    async fn tron_native_collect_with_insufficient_amount(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        seed_tron_native_collect_order(
            &self.scenario().wallet_pool,
            &self.scenario().collect_pool,
            &fixture.trade_no,
        )
        .await;
    }

    async fn tron_token_collect_with_insufficient_fee_funding(
        &self,
        fixture: &CollectResourceGateFixture,
        funding_addr: &str,
    ) {
        seed_tron_token_collect_with_fee_funding_wallet(
            &self.scenario().wallet_pool,
            &self.scenario().collect_pool,
            &fixture.trade_no,
            funding_addr,
        )
        .await;
        for uid in ["uid_collect_fee_reservation", "uid_collect_fee_funding"] {
            self.scenario().env.recorder.succeed_next_api_backend_call_with_data(
                serde_json::json!({
                    "uid": uid,
                    "threshold": 0,
                    "chainConfigs": [{
                        "chainCode": "tron",
                        "chainAddressType": null,
                        "normalAddress": { "index": 0, "address": funding_addr },
                        "riskAddress": { "index": 0, "address": funding_addr }
                    }]
                }),
            );
        }
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectResourceGateWhen {
    async fn resource_result_ack_is_sent(&self, fixture: &CollectResourceGateFixture);

    async fn resource_receipt_upload_is_sent(&self, fixture: &CollectResourceGateFixture);

    async fn resource_gate_is_evaluated(&self, fixture: &CollectResourceGateFixture);
}

#[async_trait::async_trait(?Send)]
impl CollectResourceGateWhen for WhenRole<'_, CollectResourceGateScenario> {
    async fn resource_result_ack_is_sent(&self, fixture: &CollectResourceGateFixture) {
        send_resource_result_ack_via_worker(self.scenario().env.ctx(), &fixture.resource_trade_no)
            .await
            .expect("send resource result ack");
    }

    async fn resource_receipt_upload_is_sent(&self, fixture: &CollectResourceGateFixture) {
        upload_resource_tx_exec_receipt_via_worker(
            self.scenario().env.ctx(),
            &fixture.resource_trade_no,
        )
        .await
        .expect("upload resource tx exec receipt");
    }

    async fn resource_gate_is_evaluated(&self, fixture: &CollectResourceGateFixture) {
        eval_collect_resource_gate_via_worker(self.scenario().env.ctx(), &fixture.trade_no)
            .await
            .expect("evaluate resource gate");
    }
}

#[async_trait::async_trait(?Send)]
pub(crate) trait CollectResourceGateThen {
    async fn origin_collect_gate_is_released_by_successful_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn origin_collect_gate_is_not_released(&self, fixture: &CollectResourceGateFixture);

    async fn collect_can_build(&self, fixture: &CollectResourceGateFixture);

    async fn collect_cannot_build(&self, fixture: &CollectResourceGateFixture);

    async fn collect_waits_for_local_delegate(&self, fixture: &CollectResourceGateFixture);

    async fn platform_resource_request_uses_full_order_energy(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn collect_fails_amount_shortage_without_resource_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    );

    async fn collect_fails_fee_reservation_without_resource_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    );
}

#[async_trait::async_trait(?Send)]
impl CollectResourceGateThen for ThenRole<'_, CollectResourceGateScenario> {
    async fn origin_collect_gate_is_released_by_successful_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        let collect = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().origin_collect_gate_is_released(
            &collect,
            ApiResourceGateResult::PlatformDelegateSuccess,
        );
    }

    async fn origin_collect_gate_is_not_released(&self, fixture: &CollectResourceGateFixture) {
        let collect = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().origin_collect_gate_is_not_released(&collect);
    }

    async fn collect_can_build(&self, fixture: &CollectResourceGateFixture) {
        let is_candidate =
            self.scenario().load().is_collect_build_candidate(&fixture.trade_no).await;
        self.scenario().assert().collect_can_build(is_candidate);
    }

    async fn collect_cannot_build(&self, fixture: &CollectResourceGateFixture) {
        let is_candidate =
            self.scenario().load().is_collect_build_candidate(&fixture.trade_no).await;
        self.scenario().assert().collect_cannot_build(is_candidate);
    }

    async fn collect_waits_for_local_delegate(&self, fixture: &CollectResourceGateFixture) {
        let collect = self.scenario().load().collect(&fixture.trade_no).await;
        self.scenario().assert().collect_waits_for_local_delegate(&collect);
    }

    async fn platform_resource_request_uses_full_order_energy(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        let mut captured = None;
        for _ in 0..5 {
            let req = pop_request_with_retry(&self.scenario().env.recorder)
                .await
                .expect("capture platform resource apply request");
            if req.path.contains("/awallet/aw/trans/resourceDl/apply") {
                captured = Some(req);
                break;
            }
        }
        let captured = captured.expect("capture platform resource apply request");
        let payload = decrypt_captured_api_backend_body(&captured.body);
        assert_eq!(payload["tradeNo"].as_str(), Some(fixture.trade_no.as_str()));
        assert_eq!(payload["resourceAmount"].as_f64(), Some(14_650.0));
        assert_eq!(payload["nativeTokenAmount"].as_i64(), Some(200));
    }

    async fn collect_fails_amount_shortage_without_resource_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        let collect = self.scenario().load().collect(&fixture.trade_no).await;
        let delegation_count =
            self.scenario().load().resource_delegation_count(&fixture.trade_no).await;
        self.scenario()
            .assert()
            .collect_fails_amount_shortage_without_resource_delegation(&collect, delegation_count);
    }

    async fn collect_fails_fee_reservation_without_resource_delegation(
        &self,
        fixture: &CollectResourceGateFixture,
    ) {
        let collect = self.scenario().load().collect(&fixture.trade_no).await;
        let delegation_count =
            self.scenario().load().resource_delegation_count(&fixture.trade_no).await;
        assert_eq!(collect.status, ApiCollectStatus::InsufficientBalance);
        assert_eq!(collect.err_code, Some(ErrCode::BalanceInsufficient));
        assert!(collect.err_msg.as_deref().is_some_and(|msg| msg.contains("fee reservation")));
        assert!(collect.resource_gate_released_at.is_none());
        assert!(collect.resource_dependency_trade_no.is_none());
        assert_eq!(delegation_count, 0);
    }
}

#[async_trait::async_trait(?Send)]
trait CollectResourceGateLoad {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity;

    async fn is_collect_build_candidate(&self, trade_no: &str) -> bool;

    async fn resource_delegation_count(&self, trade_no: &str) -> usize;
}

#[async_trait::async_trait(?Send)]
impl CollectResourceGateLoad for LoadRole<'_, CollectResourceGateScenario> {
    async fn collect(&self, trade_no: &str) -> ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(&self.scenario().collect_pool, trade_no)
            .await
            .expect("load collect")
    }

    async fn is_collect_build_candidate(&self, trade_no: &str) -> bool {
        ApiCollectRepo::scan_can_build(&self.scenario().collect_pool, 10_000)
            .await
            .expect("scan collect build candidates")
            .iter()
            .any(|collect| collect.trade_no == trade_no)
    }

    async fn resource_delegation_count(&self, trade_no: &str) -> usize {
        ApiResourceDelegationRepo::list_by_origin_trade_no(&self.scenario().resource_pool, trade_no)
            .await
            .expect("list resource delegations by origin")
            .len()
    }
}

trait CollectResourceGateAssert {
    fn origin_collect_gate_is_released(
        &self,
        collect: &ApiCollectEntity,
        expected_result: ApiResourceGateResult,
    );

    fn origin_collect_gate_is_not_released(&self, collect: &ApiCollectEntity);

    fn collect_can_build(&self, is_candidate: bool);

    fn collect_cannot_build(&self, is_candidate: bool);

    fn collect_waits_for_local_delegate(&self, collect: &ApiCollectEntity);

    fn collect_fails_amount_shortage_without_resource_delegation(
        &self,
        collect: &ApiCollectEntity,
        delegation_count: usize,
    );
}

impl CollectResourceGateAssert for AssertRole<'_, CollectResourceGateScenario> {
    fn origin_collect_gate_is_released(
        &self,
        collect: &ApiCollectEntity,
        expected_result: ApiResourceGateResult,
    ) {
        assert!(collect.resource_gate_released_at.is_some());
        assert_eq!(collect.resource_gate_result, Some(expected_result));
    }

    fn origin_collect_gate_is_not_released(&self, collect: &ApiCollectEntity) {
        assert!(
            collect.resource_gate_released_at.is_none(),
            "collect unexpectedly released: result={:?}, block_reason={:?}, dependency_trade_no={:?}, dependency_type={:?}",
            collect.resource_gate_result,
            collect.resource_block_reason,
            collect.resource_dependency_trade_no,
            collect.resource_dependency_type
        );
        assert!(collect.resource_gate_result.is_none());
    }

    fn collect_can_build(&self, is_candidate: bool) {
        assert!(is_candidate, "released collect should be eligible for BuildTx");
    }

    fn collect_cannot_build(&self, is_candidate: bool) {
        assert!(
            !is_candidate,
            "blocked collect should not be eligible for BuildTx before local delegation fallback"
        );
    }

    fn collect_waits_for_local_delegate(&self, collect: &ApiCollectEntity) {
        assert!(
            collect.resource_gate_released_at.is_none(),
            "collect unexpectedly released: result={:?}, block_reason={:?}, dependency_trade_no={:?}, dependency_type={:?}",
            collect.resource_gate_result,
            collect.resource_block_reason,
            collect.resource_dependency_trade_no,
            collect.resource_dependency_type
        );
        assert!(collect.resource_dependency_trade_no.is_none());
        assert_eq!(
            collect.resource_dependency_type,
            Some(ApiResourceDependencyType::LocalDelegate)
        );
        assert_eq!(collect.resource_block_reason, Some(ApiResourceBlockReason::NeedLocalDelegate));
    }

    fn collect_fails_amount_shortage_without_resource_delegation(
        &self,
        collect: &ApiCollectEntity,
        delegation_count: usize,
    ) {
        assert_eq!(collect.status, ApiCollectStatus::InsufficientBalance);
        assert_eq!(collect.err_code, Some(ErrCode::BalanceInsufficient));
        assert!(
            collect
                .err_msg
                .as_deref()
                .unwrap_or_default()
                .contains("collect amount is insufficient"),
            "unexpected collect error message: {:?}",
            collect.err_msg
        );
        assert!(
            collect.resource_gate_released_at.is_none(),
            "amount-shortage collect should not release resource gate"
        );
        assert!(collect.resource_gate_result.is_none());
        assert_eq!(delegation_count, 0, "amount-shortage collect created resource delegation");
    }
}
