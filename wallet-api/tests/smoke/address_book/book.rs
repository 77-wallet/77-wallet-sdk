use crate::get_manager;
use anyhow::Result;

#[tokio::test]
#[ignore = "smoke test writes address book data into local test data"]
async fn test_add_address_book() -> Result<()> {
    let wallet_manager = get_manager().await;

    let name = "2".to_string();
    let address = "TNPTj8Dbba6YxW5Za6tFh6SJMZGbUyucXQ".to_string();
    let chain_code = "tron".to_string();
    let detail = wallet_manager.create_address_book(name, address, chain_code).await?;

    tracing::info!("{}", serde_json::to_string(&detail).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test mutates address book data in local test data"]
async fn test_update_address_book() -> Result<()> {
    let wallet_manager = get_manager().await;

    let id = 1;
    let name = "f".to_string();
    let address = "TUe3T6ErJvnoHMQwVrqK246MWeuCEBbyuR".to_string();
    let chain_code = "tron".to_string();
    let detail = wallet_manager.update_address_book(id, name, address, chain_code).await?;

    tracing::info!("{}", serde_json::to_string(&detail).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test deletes address book data from local test data"]
async fn test_update_delete_address_book() -> Result<()> {
    let wallet_manager = get_manager().await;
    let id = 1;
    let detail = wallet_manager.delete_address_book(id).await?;

    tracing::info!("{:?}", detail);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test reads address book data from local test data"]
async fn test_list_address_book() -> Result<()> {
    let wallet_manager = get_manager().await;

    let chain_code: Option<String> = None;
    let detail = wallet_manager.list_address_book(chain_code, 0, 10).await?;

    tracing::info!("{}", serde_json::to_string(&detail).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test checks address validation through wallet manager"]
async fn test_valid_address() -> Result<()> {
    let wallet_manager = get_manager().await;

    // tron test
    // let address = "TKZnKVp8A5SPH9qkHZyh9JKa9Tj4kW5V5h".to_string();
    // // let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
    // let chain_code = "tron".to_string();

    // let res = wallet_manager.is_valid_address(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // btc test
    let address = "bc1qplxq3sfd56ruya09tg5znjqcnna30wsz9wtfr7".to_string();
    let chain_code = "btc".to_string();

    let res = wallet_manager.is_valid_address(address, chain_code).await?;
    tracing::info!("res {:?}", res);

    // bnb test
    // let address = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string();
    // let chain_code = "bnb".to_string();

    // let res = wallet_manager.is_valid_address(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // solana test
    // let address = "6Hsn4noVtYMvCauR6wrM5JMmLAkfucoLF9koTq4P1Awf".to_string();
    // let chain_code = "sol".to_string();

    // let res = wallet_manager.is_valid_address(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // // eth test
    // let address = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string();
    // let chain_code = "eth".to_string();

    // let res = wallet_manager.is_valid_address(address, chain_code).await;
    // tracing::info!("res {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test checks address status through wallet manager"]
async fn test_address_status() -> Result<()> {
    let wallet_manager = get_manager().await;

    // tron test
    let address = "TAqUJ9enU8KkZYySA51iQim7TxbbdLR2wn".to_string();
    // let address = "TXDK1qjeyKxDTBUeFyEQiQC7BgDpQm64g1".to_string();
    let chain_code = "tron".to_string();

    let res = wallet_manager.address_status(address, chain_code).await?;
    tracing::info!("res {:?}", serde_json::to_string(&res).unwrap());

    // btc test
    // let address = "bc1qplxq3sfd56ruya09tg5znjqcnna30wsz9wtfr7".to_string();
    // let chain_code = "btc".to_string();

    // let res = wallet_manager.address_status(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // // bnb test
    // let address = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string();
    // let chain_code = "bnb".to_string();
    // let res = wallet_manager.address_status(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // // solana test
    // let address = "6Hsn4noVtYMvCauR6wrM5JMmLAkfucoLF9koTq4P1Awf".to_string();
    // let chain_code = "sol".to_string();
    // let res = wallet_manager.address_status(address, chain_code).await;
    // tracing::info!("res {:?}", res);

    // // eth test
    // let address = "0xdc4778f200c36a1C9dEeb3164cEE8366aD1F9455".to_string();
    // let chain_code = "eth".to_string();
    // let res = wallet_manager.address_status(address, chain_code).await;
    // tracing::info!("res {:?}", res);
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test finds fixed address through wallet manager"]
async fn test_find_address() -> Result<()> {
    let wallet_manager = get_manager().await;
    let chain_code = "tron".to_string();
    let address = "TUe3T6ErJvnoHMQwVrqK246MWeuCEBbyuR".to_string();

    let res = wallet_manager.find_by_address(address, chain_code).await?;
    tracing::info!("{}", serde_json::to_string(&res).unwrap());
    Ok(())
}
