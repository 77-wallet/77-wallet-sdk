use std::collections::HashMap;

use crate::get_manager;
use anyhow::Result;

#[tokio::test]
#[ignore = "smoke test requires backend token price endpoint"]
async fn get_token_price() -> Result<()> {
    let wallet_manager = get_manager().await;

    let symbol = vec!["WIN".to_string()];

    let detail = wallet_manager.get_token_price(symbol).await?;

    println!("{}", wallet_utils::serde_func::serde_to_string(&detail).unwrap());
    Ok(())
}

#[tokio::test]
#[ignore = "smoke test requires backend coin market endpoint"]
async fn token_market_value() -> Result<()> {
    let wallet_manager = get_manager().await;

    let coins = HashMap::from([("eth".to_string(), "".to_string())]);

    let detail = wallet_manager.coin_market_value(coins).await?;

    println!("{}", wallet_utils::serde_func::serde_to_string(&detail).unwrap());
    Ok(())
}
