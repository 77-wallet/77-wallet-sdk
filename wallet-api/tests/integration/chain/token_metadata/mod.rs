mod support;

use support::TokenMetadataScenario;

#[tokio::test]
async fn usdg_metadata_is_available_through_the_sdk_adapter() {
    // Given a Token-2022 USDG mint and no Metaplex metadata account.
    let scenario = TokenMetadataScenario::given_usdg_mint();

    // When the SDK adapter requests the fields used by custom-token lookup.
    let (decimals, symbol, name) = scenario.when_read_token_info().await.unwrap();

    // Then all fields are available without requesting a Metaplex account.
    assert_eq!(decimals, 6);
    assert_eq!(symbol, "USDG");
    // Keep the existing SolanaChain name normalization contract unchanged.
    assert_eq!(name, "GlobalDollar");
    scenario.then_only_mint_and_supply_were_requested();
}

#[tokio::test]
async fn missing_mint_fails_without_requesting_unrelated_metadata() {
    // Given an RPC that reports the mint account is absent.
    let scenario = TokenMetadataScenario::given_missing_mint();

    // When token information is requested.
    let err = scenario.when_read_token_info().await.unwrap_err();

    // Then the error is preserved and no alternative metadata is requested.
    assert!(err.to_string().contains("mint account not found"), "{err}");
    scenario.then_failed_at_mint_lookup();
}
