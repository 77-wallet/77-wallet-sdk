use crate::init;
use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::{
    AddressInitReq,
    api_wallet::{
        address::{AddressListReq, ApiAddressInitReq, AssetListReq, ExpandAddressCompleteReq},
        swap::ApiInitSwapReq,
    },
};

#[serial_test::serial]
#[tokio::test]
async fn test_expand_address() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;

    let uid = "eb7a5f6ce1234b0d9de0d63750d6aa2c1661e89a3cc9c1beb23aad3bd324071c";

    let mut req = ApiAddressInitReq::new(1);
    let address_param = AddressInitReq::new(
        uid,
        "TMao3zPmTqNJWg3ZvQtXQxyW1MuYevTMHt",
        1,
        "tron",
        "1",
        vec![],
        "test",
    );
    req.address_list.add_address(address_param);

    let res = backend_api.expand_address(&req).await?;

    println!("[test_chain_default_list] res: {res:?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_expand_address_complete() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let uid = "c08b22451afdb497c0bf127e679978a5d167e733d00020cf6c83849bdfb2b5d5";

    let req = ExpandAddressCompleteReq::new(
        uid,
        "69282ce4bdfa8cc191031bb4",
        "tron_c08b22451afdb497c0bf127e679978a5d167e733d00020cf6c83849bdfb2b5d5",
        true,
        None,
    );
    backend_api.expand_address_complete(req).await?;

    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_query_used_address_list() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let uid = "e09bbd108fe4ec95af404cd86f92608da1ca4191f1a7b35ed50d333635a95cfe";
    let chain_code = "eth";
    let page_num = 171;
    let page_size = 1000;
    let req = AddressListReq::new(uid, chain_code, page_num, page_size);
    let res = backend_api.query_used_address_list(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res)?;
    println!("{res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_query_asset_list() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let uid = "ef98e62f7057e2c6cee9314ee017875b283dccaaeeeabc9370f8afa7a3a5e186";
    // let uid = "2b3c9d25a6d68fd127a77c4d8fefcb6c2466ac40e5605076ee3e1146f5f66993";
    let chain_code = "tron";
    let req = AssetListReq::new(uid, chain_code, vec![0, 1]);
    let res = backend_api.query_asset_list(&req).await?;
    let res = wallet_utils::serde_func::serde_to_string(&res).unwrap();
    println!("{res:#?}");
    Ok(())
}
