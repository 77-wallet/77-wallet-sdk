use crate::harness::ordinary_business::{OTHER, OWNER, Scenario, TOKEN};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn ordinary_business_duplicate_add_preserves_existing_balance() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", TOKEN, 6, "17.25").await?;
    for _ in 0..2 {
        s.when_add("wallet-a", "eth", TOKEN).await?;
    }
    s.then_count(1).await?;
    s.then_balance(OWNER, "eth", TOKEN, "17.25").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_equivalent_contract_case_add_preserves_identity_and_balance()
-> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_coin("eth", TOKEN, 6).await?;
    s.when_add("wallet-a", "eth", &TOKEN.replace("ab", "AB")).await?;
    s.then_count(1).await?;
    s.then_balance(OWNER, "eth", TOKEN, "0").await?;
    s.given_balance(OWNER, "eth", TOKEN, "17.25").await?;
    for token in [TOKEN.to_owned(), TOKEN.replace("ab", "AB")] {
        s.when_add("wallet-a", "eth", &token).await?;
        s.then_count(1).await?;
        s.then_balance(OWNER, "eth", TOKEN, "17.25").await?;
    }
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_same_contract_is_independent_across_chains() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", TOKEN, 6, "17.25").await?;
    s.given_chain("polygon").await?;
    s.given_wallet("wallet-a", OWNER, "polygon").await?;
    s.given_coin("polygon", TOKEN, 6).await?;
    s.when_add("wallet-a", "polygon", TOKEN).await?;
    s.then_count(2).await?;
    s.then_balance(OWNER, "eth", TOKEN, "17.25").await?;
    s.then_balance(OWNER, "polygon", TOKEN, "0").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_add_isolated_to_selected_wallet() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_asset(OWNER, "eth", TOKEN, 6, "17.25").await?;
    s.given_wallet("wallet-b", OTHER, "eth").await?;
    s.when_add("wallet-b", "eth", TOKEN).await?;
    s.then_count(2).await?;
    s.then_balance(OWNER, "eth", TOKEN, "17.25").await?;
    s.then_balance(OTHER, "eth", TOKEN, "0").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_uppercase_metadata_and_concurrent_add_are_idempotent()
-> anyhow::Result<()> {
    let s = Scenario::given().await?;
    let upper = TOKEN.replace("ab", "AB");
    s.given_coin("eth", &upper, 6).await?;
    let (a, b) =
        tokio::join!(s.when_add("wallet-a", "eth", TOKEN), s.when_add("wallet-a", "eth", &upper));
    a?;
    b?;
    s.then_count(1).await?;
    // Match the metadata spelling used for persistence, then retry with the other spelling.
    s.given_balance(OWNER, "eth", &upper, "17.25").await?;
    s.when_add("wallet-a", "eth", TOKEN).await?;
    s.then_count(1).await?;
    s.then_balance(OWNER, "eth", &upper, "17.25").await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_non_evm_contract_case_remains_distinct() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_chain("sol").await?;
    s.given_wallet("wallet-a", OWNER, "sol").await?;
    s.given_coin("sol", "CaseSensitiveToken", 6).await?;
    s.when_add("wallet-a", "sol", "casesensitivetoken").await?;
    s.then_count(0).await?;
    s.when_add("wallet-a", "sol", "CaseSensitiveToken").await?;
    s.then_count(1).await?;
    s.rpc.then_consumed();
    Ok(())
}

#[tokio::test]
#[serial]
async fn ordinary_business_app_add_coin_accepts_equivalent_evm_case() -> anyhow::Result<()> {
    let s = Scenario::given().await?;
    s.given_coin("eth", TOKEN, 6).await?;
    s.manager
        .add_coin(wallet_api::request::coin::AddCoinReq {
            wallet_address: "wallet-a".into(),
            account_id: 1,
            chain_list: wallet_api::response_vo::standard_wallet::chain::ChainList(
                std::collections::HashMap::from([("eth".into(), TOKEN.replace("ab", "AB"))]),
            ),
        })
        .await?;
    s.then_count(1).await?;
    s.then_balance(OWNER, "eth", TOKEN, "0").await?;
    s.rpc.then_consumed();
    Ok(())
}
