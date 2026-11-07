use crate::get_manager;
use anyhow::Result;

#[tokio::test]
#[ignore = "generates local mnemonic material; run manually only"]
async fn test_phrase() -> Result<()> {
    let wallet_manager = get_manager().await;

    let res = wallet_manager.generate_phrase(1, 12)?;
    assert_eq!(res.phrases.len(), 12);
    tracing::info!("generated phrase word count = {}", res.phrases.len());

    Ok(())
}
