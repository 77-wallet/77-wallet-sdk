use wallet_database::{
    ApiResourceDbPool,
    entities::{api_resource_delegation::NewApiResourceDelegation, api_trade_type::ApiTradeType},
    repositories::api_wallet::resource_delegation::ApiResourceDelegationRepo,
};

pub(crate) async fn insert_local_undelegation(
    pool: &ApiResourceDbPool,
    resource_trade_no: &str,
    origin_trade_no: &str,
) {
    ApiResourceDelegationRepo::upsert(
        pool,
        NewApiResourceDelegation::local_undelegate(
            "uid",
            resource_trade_no,
            origin_trade_no,
            ApiTradeType::Collect as i64,
            "owner",
            "receiver",
            "5",
            "1000",
        ),
    )
    .await
    .expect("insert local undelegate task");
}

pub(crate) async fn mark_local_undelegation_broadcasted(
    pool: &ApiResourceDbPool,
    resource_trade_no: &str,
    tx_hash: &str,
) {
    ApiResourceDelegationRepo::claim_build_slot(pool, resource_trade_no)
        .await
        .expect("claim build slot");
    ApiResourceDelegationRepo::mark_broadcast_success(pool, resource_trade_no, tx_hash)
        .await
        .expect("mark broadcast success");
}
