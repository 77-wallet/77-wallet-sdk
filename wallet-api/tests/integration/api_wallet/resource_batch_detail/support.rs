use wallet_api::testkit::api_wallet_resource::{
    TestBatchBroadcast, TestBatchResourceOperation, batch_resource_bill_detail,
    record_batch_resource_broadcasts,
};

pub(super) struct BatchResourceFixture {
    pub(super) uid: String,
    pub(super) owner: String,
    pub(super) receiver: String,
    pub(super) amount: i64,
    pub(super) tx_hash: String,
    pub(super) transaction_fee: String,
}

impl BatchResourceFixture {
    pub(super) fn delegate(tag: &str) -> Self {
        Self {
            uid: format!("uid_{tag}"),
            owner: format!("owner_{tag}"),
            receiver: format!("receiver_{tag}"),
            amount: 2,
            tx_hash: format!("hash_{tag}"),
            transaction_fee: "123".to_string(),
        }
    }

    pub(super) fn undelegate(tag: &str) -> Self {
        Self { amount: 3, ..Self::delegate(tag) }
    }
}

pub(super) struct BatchResourceDetailScenario;

impl BatchResourceDetailScenario {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) async fn given_successful_batch_broadcast(&self, fixture: &BatchResourceFixture) {
        record_batch_resource_broadcasts(
            &fixture.uid,
            &fixture.owner,
            "energy",
            TestBatchResourceOperation::BatchDelegate,
            vec![TestBatchBroadcast {
                receiver_address: fixture.receiver.clone(),
                amount: fixture.amount,
                tx_hash: fixture.tx_hash.clone(),
                transaction_fee: fixture.transaction_fee.clone(),
            }],
        )
        .await
        .expect("persist successful API batch resource broadcast");
    }

    pub(super) async fn given_successful_batch_undelegate_broadcast(
        &self,
        fixture: &BatchResourceFixture,
    ) {
        record_batch_resource_broadcasts(
            &fixture.uid,
            &fixture.owner,
            "bandwidth",
            TestBatchResourceOperation::BatchUndelegate,
            vec![TestBatchBroadcast {
                receiver_address: fixture.receiver.clone(),
                amount: fixture.amount,
                tx_hash: fixture.tx_hash.clone(),
                transaction_fee: fixture.transaction_fee.clone(),
            }],
        )
        .await
        .expect("persist successful API batch resource reclaim broadcast");
    }

    pub(super) async fn given_successful_single_delegate_broadcast(
        &self,
        fixture: &BatchResourceFixture,
    ) {
        self.given_single_broadcast(fixture, "energy", TestBatchResourceOperation::Delegate).await;
    }

    pub(super) async fn given_successful_single_undelegate_broadcast(
        &self,
        fixture: &BatchResourceFixture,
    ) {
        self.given_single_broadcast(fixture, "bandwidth", TestBatchResourceOperation::Undelegate)
            .await;
    }

    async fn given_single_broadcast(
        &self,
        fixture: &BatchResourceFixture,
        resource: &str,
        operation: TestBatchResourceOperation,
    ) {
        record_batch_resource_broadcasts(
            &fixture.uid,
            &fixture.owner,
            resource,
            operation,
            vec![TestBatchBroadcast {
                receiver_address: fixture.receiver.clone(),
                amount: fixture.amount,
                tx_hash: fixture.tx_hash.clone(),
                transaction_fee: fixture.transaction_fee.clone(),
            }],
        )
        .await
        .expect("persist successful API single resource broadcast");
    }

    pub(super) async fn when_bill_detail_is_requested(
        &self,
        tx_hash: &str,
        owner: &str,
    ) -> wallet_api::response_vo::api_wallet::transaction::ApiBillDetailVo {
        batch_resource_bill_detail(tx_hash, owner).await.expect("query API resource bill detail")
    }

    pub(super) async fn then_bill_detail_is_missing(&self, tx_hash: &str, owner: &str) {
        let error = batch_resource_bill_detail(tx_hash, owner)
            .await
            .expect_err("failed batch broadcast must not have API bill detail");
        assert!(error.to_string().contains("api resource operation not found"));
    }
}
