use wallet_transport_backend::{
    SignedCreateOrderReq, SignedUpdateRechargeHashReq, SingedOrderCancelReq,
    request::{FindAddressRawDataReq, SignedFindAddressReq},
};

use crate::init;

#[tokio::test]
async fn test_address_find_address_raw_data() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let typ = None;
    let raw_time = None;
    // let business_id = Some("218406973127921664".to_string());
    let business_id = Some("370973756060995584".to_string());
    let uid = Some("c25bcbb0cbe65b55d579bfd84840fabc28a12841042e5cd6fb4331aaa6769df3".to_string());
    let req = FindAddressRawDataReq::new(uid, typ, raw_time, business_id);
    let res = backend_api.address_find_address_raw_data(req).await.unwrap();

    println!("[test_address_find_address_raw_data] res: {res:#?}");

    Ok(())
}

#[tokio::test]
async fn test_signed_order_create() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let chain_code = "bnb_test";
    let address = "0x5985CE40d3dACf7c1352e464691BC7fb03215928";
    let multisig_address = "0x1C2Ce4352f86D37715EA3a8De1D7122ff8760149";

    let req = SignedCreateOrderReq::new(chain_code, address, multisig_address)
        .with_elements(&1.to_string(), "2");
    let res = backend_api.signed_order_create(req).await.unwrap();

    println!("[test_signed_order_create] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_cancel_multisig() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let req = SingedOrderCancelReq {
        order_id: "220236893877571584".to_string(),
        raw_data: "".to_string(),
    };
    let res = backend_api.signed_order_cancel(&req).await.unwrap();

    println!("[test_cancel_multisig] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_signed_find_address() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let req = SignedFindAddressReq { name: None, code: None, chain_code: "tron".to_string() };
    let res = backend_api.signed_find_address(req).await.unwrap();

    println!("[test_signed_find_address] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn signed_order_update_signed_hash() -> Result<(), wallet_transport_backend::Error> {
    Ok(())
}

#[tokio::test]
async fn signed_order_update_recharge_hash() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let req = SignedUpdateRechargeHashReq {
        order_id: "247856265135525888".to_string(),
        hash: "e655b2e81b40cd7f84a91e6eedf48aa4055f141efb3f7b203a5eff019e4513e2".to_string(),
        product_code: "1".to_string(),
        receive_chain_code: "tron".to_string(),
        receive_address: "TVT37a8fkJDoVaufGg6XcKqJwJncFyCnPB".to_string(),
        raw_data: "12000000000000003234373835363236353133353532353838380f000000000000004d756c74697369672d74726f6e2d3422000000000000005458444b31716a65794b784454425565467945516951433742674470516d3634673122000000000000005458444b31716a65794b784454425565467945516951433742674470516d3634673100000000000000000000000000000000020001040000000000000074726f6e010000000200000000000000000000000000000000000000400000000000000065363535623265383162343063643766383461393165366565646634386161343035356631343165666233663762323033613565666630313965343531336532040000000000000074726f6e00000000000000001400000000000000323032352d30342d30375430383a35373a31385a011b00000000000000323032352d30342d30375430393a30353a32332e3533323431375a02000000000000001200000000000000323437383536323635313335353235383838220000000000000054556533543645724a766e6f484d51775672714b3234364d5765754345426279755209000000000000006163636f756e745f31000100000000000000004000000000000000313337656236323431313861303232346634393164393466313533633261643362366535353636316462663638376438613862613863353961613761623335381400000000000000323032352d30342d30375430383a35373a31385a00120000000000000032343738353632363531333535323538383822000000000000005458444b31716a65794b784454425565467945516951433742674470516d3634673109000000000000006163636f756e745f3001018200000000000000303435423833303134434341434431433737464641353630413234454341363241324338354239303035333638323530343533303238413444463137354341383930323638453646413145333835443637343130314546363935423145444231433038453135373937354338463930303535383133433644453834354639444643444000000000000000313337656236323431313861303232346634393164393466313533633261643362366535353636316462663638376438613862613863353961613761623335381400000000000000323032352d30342d30375430383a35373a31385a00".to_string(),
        score_trans_id:"".to_string(),
    };
    let res = backend_api.signed_order_update_recharge_hash(&req).await.unwrap();

    println!("[signed_order_update_recharge_hash] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_query_multisig_account() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let id = "214178817818890240";
    let res = backend_api.check_multisig_account_is_cancel(id).await.unwrap();

    println!("[test_query_multisig_account] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_signed_fee_info_v3() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    // 创建测试请求
    let chain_code = "tron";
    let pay_chain_code = "bnb";
    let address = "0xa2d68BDAbA160D2284BAb79d7c24d7117586AE50";
    let uid = "c25bcbb0cbe65b55d579bfd84840fabc28a12841042e5cd6fb4331aaa6769df3";

    let req = wallet_transport_backend::request::SignedFeeListReq::new(
        chain_code,
        pay_chain_code,
        address,
        uid.to_string(),
    );
    let res = backend_api.signed_fee_info_v3(req).await.unwrap();

    println!("[test_signed_fee_info_v2] res: {res:#?}");

    Ok(())
}
