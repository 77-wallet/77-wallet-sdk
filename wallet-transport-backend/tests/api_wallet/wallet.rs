use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::{
    request::{
        KeysInitReq,
        api_wallet::{
            swap::ApiInitSwapReq,
            wallet::{AppIdImportReq, AppIdUidUsageReq, BindAppIdReq, InitApiWalletReq},
        },
    },
    response_vo::api_wallet::wallet::UidStatus,
};

use crate::init;

#[serial_test::serial]
#[tokio::test]
async fn test_query_wallet_activation_info() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let res = backend_api
        .query_wallet_activation_info(
            "73fe9dcf4811f56552f6d87ebadc323cf5fbb56b72ae77bd5a10135f327eaeed",
        )
        .await?;

    println!("[test_query_wallet_activation_info] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_keys_uid_check() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let res = backend_api
        .keys_uid_check("319036ef9f98bc023fdc6611d136c8affecb1b545e928646e45eaeece0a6565d")
        .await?;

    println!("[test_keys_uid_check] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_query_uid_bind_info() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let res = backend_api
        .query_uid_bind_info("c91f78d83576dbaa8dce16285787aa2efbc9c0e606b54f7bc96e951d848496db")
        .await?;

    println!("[test_query_uid_bind_info] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_old_keys_init() -> Result<(), wallet_transport_backend::Error> {
    let sn = "b35f7b556b87c87bb1928ea6ab12ef6918b71f5c37fbd53b88e9353ea2093f0b";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let uid = "e09bbd108fe4ec95af404cd86f92608da1ca4191f1a7b35ed50d333635a95cfe";
    let client_id = "4206b0fecd683a1505d24a135b606e9c";
    let device_type = "ANDROID";
    let name = "api_wallet";
    let invite_code = None;
    let req = KeysInitReq::new(
        uid,
        sn,
        Some(client_id.to_string()),
        Some(device_type.to_string()),
        name,
        invite_code,
    );

    let res = backend_api.old_keys_init(&req).await?;

    println!("[test_old_keys_init] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_init_api_wallet() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    let mut req =
        InitApiWalletReq::new("73fe9dcf4811f56552f6d87ebadc323cf5fbb56b72ae77bd5a10135f327eaeed");

    req.set_recharge_uid("319036ef9f98bc023fdc6611d136c8affecb1b545e928646e45eaeece0a6565d");
    let res = backend_api.init_api_wallet(req).await?;

    println!("[test_init_api_wallet] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_appid_uid_usage() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let req = AppIdUidUsageReq::new(
        "0583c23559bc44acb4132c59f0f2be21",
        "17931d2265113d34604598200350c0e5eba860af969768c91d5aee7f499c08c1",
        UidStatus::ApiWaw,
    );

    let res = backend_api.appid_uid_usage(req).await?;

    println!("[test_appid_uid_usage] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_wallet_bind_appid() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let req = BindAppIdReq::new(
        "88a06da151b1d51c3f9e751ba398be4abb67e816359c849ef66ac0c7bbbd0640",
        "48c3ab3ce9d017ee8720e608646ff00a0957f7ea8f0d7edf38e868bcf06e6808",
        "bfd9b8aa4f384b839392e9018280e9fb",
        "3cf5ee2bf4971c12306cf24a1a2fabfac2a97e895f994325c935babc022185d3",
    );
    let res = backend_api.wallet_bind_appid(&req).await?;

    println!("[test_wallet_bind_appid] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_appid_import() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let mut req = AppIdImportReq::new("666");
    req.set_recharge_uid("1ef356952666777b0132b9ff3dd3becf0c6c0268d72641d9230c8435fda86ae0");
    // req.set_withdrawal_uid("4080938dda41a016b8c153be34b558345259a4b4116d5a88e004507341164b78");

    let res = backend_api.appid_import(req).await?;

    tracing::info!("[test_appid_import] res: {res:#?}");
    Ok(())
}

#[serial_test::serial]
#[tokio::test]
async fn test_appid_withdrawal_wallet_change() -> Result<(), wallet_transport_backend::Error> {
    let sn = "666";
    let backend_api = init(sn)?;
    let req = ApiInitSwapReq { sn: sn.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let res = backend_api.init_swap(&req).await?;
    if let Some(data) = res.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }

    let withdrawal_uid = "17931d2265113d34604598200350c0e5eba860af969768c91d5aee7f499c08c1";
    let org_app_id = "0583c23559bc44acb4132c59f0f2be21";
    let res = backend_api.appid_withdrawal_wallet_change(withdrawal_uid, org_app_id).await?;

    println!("[test_appid_withdrawal_wallet_change] res: {res:#?}");
    Ok(())
}
