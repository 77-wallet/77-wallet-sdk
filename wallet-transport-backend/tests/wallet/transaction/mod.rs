use wallet_transport_backend::request::SignedTranUpdateHashReq;

use crate::init;

#[tokio::test]
#[ignore = "manual live smoke: queries dev gas tracker without broadcasting"]
pub async fn test_fee_oracle() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    for chain_code in ["eth", "polygon", "avalanche", "arbitrum", "base"] {
        let oracle = backend_api.gas_oracle(chain_code).await?;
        for (tier, value) in [
            ("safe", oracle.safe_gas_price),
            ("propose", oracle.propose_gas_price),
            ("fast", oracle.fast_gas_price),
        ] {
            let price = value
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|price| price.is_finite() && *price > 0.0)
                .unwrap_or_else(|| panic!("{chain_code} {tier} gas price must be positive"));
            tracing::info!(chain_code, tier, price, "dev gas tracker tier");
        }
    }

    Ok(())
}

#[tokio::test]
pub async fn test_signed_tran_update_hash() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let req = SignedTranUpdateHashReq {
        withdraw_id: "155061461155188736".to_string(),
        hash: "xxxxxxxxxxxx".to_string(),
        remark: "hello".to_string(),
        raw_data: "".to_string(),
    };

    let res = backend_api.signed_tran_update_trans_hash(&req).await.unwrap();
    tracing::info!("res  {:?}", res);

    Ok(())
}

#[tokio::test]
pub async fn test_records_list() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let cc = backend_api
        .record_lists(
            "tron",
            "TDmNZ4Wz7aMEt1tbRq7EVocWkxWn2SLoPE",
            Some("2024-09-28 00:00:00".to_string()),
        )
        .await
        .unwrap();

    println!("[record] res: {cc:#?}");

    Ok(())
}

#[tokio::test]
pub async fn test_gas_oracle() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize using init()

    let cc = backend_api.gas_oracle("scroll").await.unwrap();

    println!("[gas oracle] res: {cc:#?}");

    Ok(())
}
