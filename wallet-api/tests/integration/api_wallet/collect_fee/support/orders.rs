use wallet_database::{
    ApiTransactionDbPool,
    entities::api_collect::{ApiCollectEntity, ApiCollectStatus},
    repositories::api_wallet::collect::ApiCollectRepo,
};

pub(crate) async fn seed_collect_order(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    to_addr: &str,
) -> ApiCollectEntity {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        "from-sol",
        to_addr,
        "0.000015",
        "digest",
        "sol",
        None,
        "SOL",
        trade_no,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert collect");

    ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await.expect("load collect")
}

pub(crate) async fn seed_eth_collect_order(
    pool: &ApiTransactionDbPool,
    trade_no: &str,
    from_addr: &str,
    to_addr: &str,
    value: &str,
) -> ApiCollectEntity {
    ApiCollectRepo::upsert_api_collect(
        pool,
        "uid",
        "collect",
        from_addr,
        to_addr,
        value,
        "digest",
        "eth",
        None,
        "ETH",
        trade_no,
        2,
        ApiCollectStatus::Init,
        1,
    )
    .await
    .expect("insert collect");

    ApiCollectRepo::get_api_collect_by_trade_no(pool, trade_no).await.expect("load collect")
}
