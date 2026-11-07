use crate::get_manager;

// 拉取热门币
#[tokio::test]
async fn test_pull_host_coin() {
    let wallet = get_manager().await;

    let endpoint = "token/queryByPage".to_string();
    let body = r#"{"orderColumn":"create_time","orderType":"DESC","defaultToken":true,"excludeNameList":[],"pageNum":0,"pageSize":100}"#.to_string();

    let req = wallet.request(endpoint, body).await;

    tracing::warn!("req: {req:?}");
}

#[tokio::test]
async fn test_get_token_price() {
    let wallet = get_manager().await;

    let endpoint = "token/queryPrice".to_string();
    let body = r#"[{"orderColumn":"create_time","orderType":"DESC","defaultToken":true,"excludeNameList":[],"pageNum":0,"pageSize":1000}]"#.to_string();

    let req = wallet.request(endpoint, body).await;

    tracing::warn!("req: {req:?}");
}
