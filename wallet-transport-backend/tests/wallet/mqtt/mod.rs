use wallet_transport_backend::request::{
    MsgConfirmSource, QueryUnconfirmMsgReq, SendMsgConfirm, SendMsgConfirmReq,
};

use crate::init;

#[tokio::test]
async fn test_send_msg_confirm() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let req = SendMsgConfirmReq {
        list: vec![SendMsgConfirm {
            id: "676059fcab8ff576d42076ef".to_string(),
            source: MsgConfirmSource::Mqtt,
        }],
    };
    let res = backend_api.send_msg_confirm(&req).await.unwrap();

    println!("[test_send_msg_confirm] res: {res:?}");

    Ok(())
}

#[tokio::test]
async fn test_send_msg_query_unconfirm_msg() -> Result<(), wallet_transport_backend::Error> {
    let backend_api = init("3f76bd432e027aa97d11f2c3f5092bee195991be461486f0466eec9d46940e9e")?; // Initialize the cryptor and API

    let client_id = "b27795bdc063bd2c8f5e630fbccf3ab9".to_string();

    let req = QueryUnconfirmMsgReq { client_id };

    let res = backend_api.query_unconfirm_msg(&req).await.unwrap();

    println!("[test_send_msg_query_unconfirm_msg] res: {res:?}");

    Ok(())
}
