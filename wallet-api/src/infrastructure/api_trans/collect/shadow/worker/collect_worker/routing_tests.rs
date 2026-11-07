use super::ShadowCollectWorker;
use crate::{
    domain::api_wallet::strategy::StrategyDomain,
    infrastructure::api_trans::{AddressLockManager, collect::shadow::ShadowAdvancer},
};
use wallet_database::{
    entities::{api_account::CreateApiAccountVo, api_collect::ApiCollectStatus},
    repositories::api_wallet::{account::ApiAccountRepo, collect::ApiCollectRepo},
};
use wallet_transport_backend::request::api_wallet::strategy::{
    ChainConfig, IndexAndAddress, Strategy,
};

const NORMAL: &str = "0x1111111111111111111111111111111111111111";
const RISK: &str = "0x2222222222222222222222222222222222222222";
const UPDATED_RISK: &str = "0x3333333333333333333333333333333333333333";

struct RoutingScenario {
    ctx: &'static crate::context::Context,
    worker: ShadowCollectWorker,
    intents: tokio::sync::mpsc::Receiver<
        crate::infrastructure::api_trans::collect::shadow::CollectIntent,
    >,
    uid: String,
    trade: String,
    chain: String,
}

impl RoutingScenario {
    async fn given(chain: &str, risk: u8) -> Self {
        let ctx = crate::testkit::context::api_trans_test_ctx().await;
        let uid = format!("routing_{}", uuid::Uuid::new_v4().simple());
        let from = format!("0x{}00000000", uuid::Uuid::new_v4().simple());
        let wallet = format!("wallet-{uid}");
        ApiAccountRepo::upsert_account_multi(
            &ctx.api_wallet_pool().unwrap(),
            vec![CreateApiAccountVo::new(
                0,
                &from,
                "fixture-public-key",
                &wallet,
                &uid,
                "m/44'/60'/0'/0/0",
                0,
                chain,
                "routing",
                wallet_database::entities::api_wallet::ApiWalletType::SubAccount,
            )],
        )
        .await
        .unwrap();
        let trade = format!("trade-{uid}");
        ApiCollectRepo::upsert_api_collect(
            &ctx.api_transaction_pool().unwrap(),
            &uid,
            "routing",
            &from,
            RISK,
            "1",
            "fixture-digest",
            chain,
            None,
            "ETH",
            &trade,
            2,
            ApiCollectStatus::Init,
            risk,
        )
        .await
        .unwrap();
        let (tx, intents) = tokio::sync::mpsc::channel(8);
        let worker = ShadowCollectWorker::new(
            ctx,
            std::sync::Arc::new(AddressLockManager::new()),
            std::sync::Arc::new(ShadowAdvancer::new(ctx, tx, None)),
        );
        let scenario = Self { ctx, worker, intents, uid, trade, chain: chain.into() };
        scenario.given_strategy(chain, RISK).await;
        scenario
    }

    async fn given_strategy(&self, chain: &str, risk_address: &str) {
        StrategyDomain::save_local_collect_strategy(
            self.ctx,
            &self.uid,
            &Strategy::new(
                &self.uid,
                50,
                vec![ChainConfig {
                    chain_code: chain.into(),
                    chain_address_type: None,
                    normal_address: IndexAndAddress { index: Some(0), address: NORMAL.into() },
                    risk_address: IndexAndAddress { index: Some(1), address: risk_address.into() },
                }],
            ),
        )
        .await
        .unwrap();
    }

    async fn order(&self) -> wallet_database::entities::api_collect::ApiCollectEntity {
        ApiCollectRepo::get_api_collect_by_trade_no(
            &self.ctx.api_transaction_pool().unwrap(),
            &self.trade,
        )
        .await
        .unwrap()
    }

    async fn when_resolve(&self) -> Result<String, crate::error::service::ServiceError> {
        self.worker.resolve_collect_to_addr(&self.order().await).await
    }

    async fn then_resolution_has_no_execution_side_effect(&mut self) {
        let order = self.order().await;
        assert_eq!(order.to_addr, RISK);
        assert!(order.raw_tx.is_none());
        assert!(order.tx_hash.is_none());
        assert!(order.building_at.is_none());
        assert!(order.finished_at.is_none());
        assert!(self.intents.try_recv().is_err());
    }
}

#[tokio::test]
async fn collect_routing_risk_uses_risk_address_on_each_new_evm_chain() {
    for chain in ["polygon", "arbitrum", "base", "avalanche", "scroll"] {
        let mut scenario = RoutingScenario::given(chain, 2).await;
        assert_eq!(scenario.when_resolve().await.unwrap(), RISK, "chain={chain}");
        scenario.then_resolution_has_no_execution_side_effect().await;
    }
}

#[tokio::test]
async fn collect_routing_normal_and_legacy_default_keep_normal_address() {
    for risk in [0, 1] {
        let mut scenario = RoutingScenario::given("base", risk).await;
        assert_eq!(scenario.when_resolve().await.unwrap(), NORMAL, "risk={risk}");
        scenario.then_resolution_has_no_execution_side_effect().await;
    }
}

#[tokio::test]
async fn collect_routing_retry_uses_latest_risk_strategy() {
    let mut scenario = RoutingScenario::given("base", 2).await;
    assert_eq!(scenario.when_resolve().await.unwrap(), RISK);
    scenario.given_strategy(&scenario.chain, UPDATED_RISK).await;
    assert_eq!(scenario.when_resolve().await.unwrap(), UPDATED_RISK);
    scenario.then_resolution_has_no_execution_side_effect().await;
}

#[tokio::test]
async fn collect_routing_missing_chain_fails_without_using_other_chain_or_mutating_order() {
    let mut scenario = RoutingScenario::given("base", 2).await;
    scenario.given_strategy("scroll", UPDATED_RISK).await;
    let error = scenario.when_resolve().await.unwrap_err();
    assert!(matches!(error,
        crate::error::service::ServiceError::Business(
            crate::error::business::BusinessError::ApiWallet(
                crate::error::business::api_wallet::ApiWalletError::ChainConfigNotFound(ref chain)
            )
        ) if chain == "base"
    ));
    scenario.then_resolution_has_no_execution_side_effect().await;
}
