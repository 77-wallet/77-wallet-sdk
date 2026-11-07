//! Live MQTT workflow replays. See docs/codex/testing.md: explicit test environment only.
//! These initialize a real client and can update backend/account state. Never run with prod config.

#[tokio::test]
#[ignore = "live backend/account replay; requires explicit dev/test authorization, local examples/config.yaml and matching test device/wallet fixtures"]
async fn live_acct_change() -> anyhow::Result<()> {
    wallet_utils::init_test_log();
    let (manager, _) = wallet_api::testkit::env::get_manager().await?;

    let change = r#"{"txHash":"c357a09e84a6dd1ad0d621641320f505fd23bc3c48251a5d524fd281de2870da:ftIuBQWDNv8Ik9FQy8aUIfzdrTbennywxOCmw6Ury1A=","chainCode":"ton","symbol":"TON","transferType":0,"txKind":1,"fromAddr":"UQDaL1eH_9TU3hceiO7ZsPDEdcmwDhZ0eDZ_NCOIrmjHoSQb","toAddr":"UQAJr_aCqkWARCMkTHYkpKL9B-kYOFvXxvyDumUXsZ79ZnYY","token":"","value":0.01,"transactionFee":0.002432489,"transactionTime":"2025-06-17 08:53:28","status":true,"isMultisig":0,"queueId":"","blockHeight":48927711,"notes":"","netUsed":0,"energyUsed":null}"#;
    let change = serde_json::from_str::<serde_json::Value>(&change)?;
    let _res =
        wallet_api::testkit::mqtt::replay_api_wallet_account_change(manager.ctx(), &change, "2")
            .await?;

    let change = r#"{"txHash":"c357a09e84a6dd1ad0d621641320f505fd23bc3c48251a5d524fd281de2870da:ftIuBQWDNv8Ik9FQy8aUIfzdrTbennywxOCmw6Ury1A=","chainCode":"ton","symbol":"TON","transferType":1,"txKind":1,"fromAddr":"UQDaL1eH_9TU3hceiO7ZsPDEdcmwDhZ0eDZ_NCOIrmjHoSQb","toAddr":"UQAJr_aCqkWARCMkTHYkpKL9B-kYOFvXxvyDumUXsZ79ZnYY","token":"","value":0.01,"transactionFee":0.002432489,"transactionTime":"2025-06-17 08:53:28","status":true,"isMultisig":0,"queueId":"","blockHeight":48927711,"notes":"","netUsed":0,"energyUsed":null}"#;
    let change = serde_json::from_str::<serde_json::Value>(&change)?;

    let _res =
        wallet_api::testkit::mqtt::replay_api_wallet_account_change(manager.ctx(), &change, "1")
            .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "live address expansion; requires explicit dev/test authorization, local examples/config.yaml and matching test UID/device fixtures"]
async fn live_address_allock() -> anyhow::Result<()> {
    wallet_utils::init_test_log();
    let (manager, _) = wallet_api::testkit::env::get_manager().await?;

    let change = r#"
            {
                "chain": "tron",
                "index": null,
                "batchId": "tron_88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640",
                "number": "50",
                "serialNo": "tron_88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640",
                "type": "CHA_BATCH",
                "uid": "88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640"
            }
        "#;
    let change = serde_json::from_str::<serde_json::Value>(&change)?;
    let _res =
        wallet_api::testkit::mqtt::replay_api_wallet_address_expand(manager.ctx(), &change, "2")
            .await?;

    Ok(())
}
