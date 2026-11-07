use wallet_transport_backend::request::{AppInstallSaveReq, LanguageInitReq, VersionViewReq};

use crate::init;
#[tokio::test]
async fn test_app_install_save() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;

    let req = AppInstallSaveReq {
        sn: "2275b5608dbae9a49ddd7257e98ef657f2013040c70176cbf938d8c1ffaa0afc".to_string(),
        channel: "android_google_shop".to_string(),
        device_type: "ANDROID".to_string(),
    };
    let res = backend_api.app_install_save(req).await.unwrap();

    println!("[test_chain_default_list] res: {res:?}");
    Ok(())
}

#[tokio::test]
async fn test_app_install_download() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;

    let res = backend_api.app_install_download().await.unwrap();

    println!("[test_app_install_download] res: {res:?}");
    Ok(())
}

#[tokio::test]
async fn test_token() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;

    let res = backend_api.rpc_token("6f88a37aca2384cec6029d5983fac0e2").await.unwrap();

    println!("[test_chain_default_list] res: {res:?}");
    Ok(())
}

#[tokio::test]
async fn test_version_view() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;
    // let r#type = "android_google_shop".to_string();
    let r#type = "official_website";
    let req = VersionViewReq::new(r#type);
    let res = backend_api.version_view(req).await.unwrap();

    println!("[test_chain_default_list] res: {res:?}");
    Ok(())
}

#[tokio::test]
async fn test_version_download_url() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;
    // https://api.77wallet.org//version/view/https%3A%2F%2F77.im%2F%23%2Fdownload

    let url = "";
    let encode_url = urlencoding::encode(url);
    let res = backend_api.version_download_url(&encode_url).await.unwrap();

    println!("[test_chain_default_list] res: {res:?}");
    Ok(())
}
#[tokio::test]
async fn test_language_init() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;

    let req = LanguageInitReq {
        client_id: "6f88a37aca2384cec6029d5983fac0e2".to_string(),
        lan: "CHINESE_SIMPLIFIED".to_string(),
    };
    let res = backend_api.language_init(req).await.unwrap();

    println!("[test_language_init] res: {res:?}");
    Ok(())
}

#[tokio::test]
async fn test_mqtt_init() -> Result<(), wallet_transport_backend::Error> {
    let sn = "3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e";
    let backend_api = init(sn)?;

    let res = backend_api.mqtt_init().await.unwrap();

    println!("[test_language_init] res: {res:?}");
    Ok(())
}
