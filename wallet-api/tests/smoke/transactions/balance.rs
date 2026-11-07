use crate::get_manager;
use anyhow::{Context, Result};

#[tokio::test]
#[ignore = "requires backend/chain balance providers and fixed chain addresses"]
async fn chain_balance_native_empty_token_supported_chains() -> Result<()> {
    let wallet_manager = get_manager().await;
    let cases = [
        ("eth", "ETH", "0x998522f928A37837Fa8d6743713170243b95f98a"),
        ("bnb", "BNB", "0x998522f928A37837Fa8d6743713170243b95f98a"),
        ("btc", "BTC", "bc1qgs3l6uh0atn3ks807anzy8sqhvtc2j9dv8axa7"),
        ("ltc", "LTC", "LPksEuS2ZeN89BwKQkJw4HAAivrruFDn3j"),
        ("sol", "SOL", "37qZgmfhQNvjTfycUeXte3sAucAY4iaqoTZfhFxZb7L1"),
        ("sui", "SUI", "0xfba1550112b16f3608669c8ab4268366c7bacb3a2cb844594ad67c21af85a1dd"),
        ("ton", "TON", "UQAj45nzNLyAKtnP038PCrqGxwUEpgdrGyz9keGedamIafpw"),
        ("tron", "TRX", "TQACP632EQvyecJTG5wTvMuqy8a4f4TJVr"),
    ];

    for (chain_code, symbol, address) in cases {
        let balance_none = wallet_manager
            .chain_balance(address, chain_code, symbol, None)
            .await
            .with_context(|| format!("chain_balance(None) failed: {chain_code}"))?;
        let balance_empty = wallet_manager
            .chain_balance(address, chain_code, symbol, Some(String::new()))
            .await
            .with_context(|| format!("chain_balance(Some(\"\")) failed: {chain_code}"))?;

        assert_eq!(
            balance_none.decimals, balance_empty.decimals,
            "decimals mismatch on chain={chain_code}"
        );
    }

    Ok(())
}
