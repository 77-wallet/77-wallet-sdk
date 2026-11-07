use crate::init;
use wallet_transport_backend::{
    Error,
    request::{
        CustomTokenInitReq, TokenQueryHistoryPrice, TokenQueryPopularByPageReq, TokenQueryPrice,
        TokenQueryPriceReq,
    },
};

#[tokio::test]
async fn test_custom_token_init() -> Result<(), Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // 初始化加密器和API

    let req = CustomTokenInitReq {
        address: "0x628F76eAB0C1298F7a24d337bBbF1ef8A1Ea6A24".to_string(),
        chain_code: "eth".to_string(),
        symbol: "XRP".to_string(),
        token_name: "XRP(IBC)".to_string(),
        contract_address: Some("0x628F76eAB0C1298F7a24d337bBbF1ef8A1Ea6A24".to_string()),
        master: false,
        unit: 6,
    };
    let res = backend_api.custom_token_init(req).await.unwrap();

    println!("[test_custom_token_init] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_token_query_price() -> Result<(), Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // 初始化加密器和API

    let req = TokenQueryPriceReq(vec![TokenQueryPrice {
        chain_code: "tron".to_string(),
        contract_address_list: vec!["TNDSHKGBmgRx9mDYA9CnxPx55nu672yQw2".to_string()],
    }]);
    let res = backend_api.token_query_price(&req).await.unwrap();

    let res_str = wallet_utils::serde_func::serde_to_string(&res).unwrap();
    tracing::info!("[test_token_query_price] res: {res_str}");

    Ok(())
}

#[tokio::test]
async fn test_token_query_his_price() -> Result<(), Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // 初始化加密器和API

    let req = TokenQueryHistoryPrice {
        chain_code: "tron".to_string(),
        date_type: "DAY".to_string(),
        currency: "USD".to_string(),
        contract_address: "TSSMHYeV2uE9qYH95DqyoCuNCzEL1NvU3S".to_string(),
    };

    let res = backend_api.query_history_price(&req).await.unwrap();

    println!("[test_token_query_his_price] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_query_popular_by_page() -> Result<(), Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // 初始化加密器和API

    let req = TokenQueryPopularByPageReq::new(
        None,
        None,
        Some("marketValue".to_string()),
        Some("DESC".to_string()),
        0,
        300,
    );

    let res = backend_api.query_popular_by_page(&req).await.unwrap();

    let res_str = serde_json::to_string(&res).unwrap();
    tracing::info!("[test_query_popular_by_page] res: {res_str}");

    Ok(())
}
