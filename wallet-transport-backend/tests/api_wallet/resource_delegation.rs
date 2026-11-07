use crate::init;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::api_wallet::{
    resource_delegation::{ResourceApplyReq, ResourceType},
    swap::ApiInitSwapReq,
    transaction::TransType,
};

#[serial_test::serial]
#[tokio::test]
async fn test_apply_resource_delegation() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let req = ResourceApplyReq::new(
        "C2056282615807373312",
        "8276baee61e14956bf8ad036e4a5efb3",
        "6a044edb3f923904b04aaf71",
        Some("tron"),
        15,
        Some(14650.0),
        ResourceType::Energy,
        "TJZ7AVWQZ2V6nu5SwP718swoQe1yu2VWVv",
        TransType::ColRscDl,
    );
    let res = backend_api.apply_resource_delegation(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_apply_resource_delegation] res: {res}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_apply_resource_delegation_withdraw() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let req = ResourceApplyReq::new(
        "W2036586295360655361",
        "test-uid",
        "test-org",
        Some("tron"),
        50000,
        None,
        ResourceType::Energy,
        "T_withdraw_address",
        TransType::WdRscDl,
    );
    let res = backend_api.apply_resource_delegation(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_apply_resource_delegation_withdraw] res: {res}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_apply_resource_delegation_bandwidth() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let req = ResourceApplyReq::new(
        "C2036586295360655362",
        "test-uid",
        "test-org",
        Some("tron"),
        200000,
        None,
        ResourceType::Bandwidth,
        "T_address_needs_bandwidth",
        TransType::ColRscDl,
    );
    let res = backend_api.apply_resource_delegation(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("[test_apply_resource_delegation_bandwidth] res: {res}");
    Ok(())
}
