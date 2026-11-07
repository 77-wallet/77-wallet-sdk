use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::api_wallet::swap::ApiInitSwapReq;

use crate::init;

#[serial_test::serial]
#[tokio::test]
async fn test_swap() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    tracing::info!("[test_init_swap] req: {req:#?}");

    let res = backend_api.init_swap(&req).await?;

    tracing::info!("[test_init_swap] res: {res:#?}");
    Ok(())
}
