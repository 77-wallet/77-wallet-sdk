use wallet_transport_backend::request::FindConfigByKey;

use crate::init;

#[tokio::test]
async fn test_find_config_by_key() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let req = FindConfigByKey { key: "OFFICIAL:WEBSITE".to_string() };

    let res = backend_api.find_config_by_key(req).await.unwrap();

    println!("[find_config_by_key] res: {res:?}");

    Ok(())
}
