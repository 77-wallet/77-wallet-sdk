use crate::{
    application::api_wallet_resource::ApiResourceApplication,
    domain::api_wallet::resource::ApiBatchResourceBroadcast, error::service::ServiceError,
    response_vo::api_wallet::transaction::ApiBillDetailVo,
    service::api_wallet::transaction::ApiTransService, testkit::context::api_trans_test_ctx,
};
use wallet_chain_interact::tron::operations::stake::ResourceType;
use wallet_database::entities::api_resource_operation::ApiResourceOperationType;

#[derive(Debug, Clone, Copy)]
pub enum TestBatchResourceOperation {
    Delegate,
    Undelegate,
    BatchDelegate,
    BatchUndelegate,
}

#[derive(Debug)]
pub struct TestBatchBroadcast {
    pub receiver_address: String,
    pub amount: i64,
    pub tx_hash: String,
    pub transaction_fee: String,
}

pub async fn record_batch_resource_broadcasts(
    uid: &str,
    owner_address: &str,
    resource_type: &str,
    operation: TestBatchResourceOperation,
    broadcasts: Vec<TestBatchBroadcast>,
) -> Result<(), ServiceError> {
    let ctx = api_trans_test_ctx().await;
    let resource = ResourceType::try_from(resource_type)?;
    let operation_type = match operation {
        TestBatchResourceOperation::Delegate => ApiResourceOperationType::Delegate,
        TestBatchResourceOperation::Undelegate => ApiResourceOperationType::Undelegate,
        TestBatchResourceOperation::BatchDelegate => ApiResourceOperationType::BatchDelegate,
        TestBatchResourceOperation::BatchUndelegate => ApiResourceOperationType::BatchUndelegate,
    };
    let broadcasts = broadcasts
        .into_iter()
        .map(|broadcast| ApiBatchResourceBroadcast {
            receiver_address: broadcast.receiver_address,
            amount: broadcast.amount,
            tx_hash: broadcast.tx_hash,
            raw_tx: String::new(),
            transaction_fee: broadcast.transaction_fee,
        })
        .collect();

    ApiResourceApplication::new(ctx)
        .record_client_batch_resource_broadcasts(
            uid,
            owner_address,
            resource,
            operation_type,
            broadcasts,
        )
        .await
}

pub async fn batch_resource_bill_detail(
    tx_hash: &str,
    owner_address: &str,
) -> Result<ApiBillDetailVo, ServiceError> {
    ApiTransService::new(api_trans_test_ctx().await).bill_detail(tx_hash, owner_address).await
}
