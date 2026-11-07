use crate::init;

#[tokio::test]
async fn test_delegate_is_open() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let res = backend_api.delegate_is_open().await.unwrap();
    tracing::info!("{res:?}");

    Ok(())
}

#[tokio::test]
async fn test_delegate_complete() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let order = "672343049017657afff102f1";
    let res = backend_api.delegate_complete(&order).await.unwrap();
    tracing::info!("{res:?}");

    Ok(())
}

#[tokio::test]
async fn test_delegate_query_order() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let order = "66e6b46c3ebdf9433dcb3c49";
    let res = backend_api.delegate_query_order(&order).await.unwrap();
    tracing::info!("{res:?}");

    Ok(())
}

#[tokio::test]
async fn test_delegate_order() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1";
    let energy = 10000;
    let res = backend_api.delegate_order(&address, energy).await.unwrap();
    tracing::info!("{res:?}");

    Ok(())
}

#[tokio::test]
async fn test_vote_list() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let res = backend_api.vote_list().await.unwrap();
    tracing::info!("{res:#?}");

    Ok(())
}

#[tokio::test]
#[ignore = "live backend smoke test; run manually with --ignored --nocapture"]
async fn live_vote_list_print_names() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?;

    let res = backend_api.vote_list().await?;
    let missing_name_count = res.node_resp_list.iter().filter(|node| node.name.is_none()).count();

    println!("total_node: {}", res.total_node);
    println!("total_vote_count: {}", res.total_vote_count);
    println!("nodes_missing_name: {missing_name_count}");
    println!("first_nodes:");
    for (index, node) in res.node_resp_list.iter().take(30).enumerate() {
        println!(
            "#{index}: name={:?}, address={}, vote_count={}, url={}, brokerage={}, apr={}",
            node.name, node.address, node.vote_count, node.url, node.brokerage, node.apr
        );
    }

    assert!(!res.node_resp_list.is_empty(), "vote/list returned no nodes");

    Ok(())
}
