use wallet_ecdh::GLOBAL_KEY;
use wallet_transport_backend::request::{
    QueryUnconfirmMsgReq,
    api_wallet::{msg::MsgAckExpiredResendReq, swap::ApiInitSwapReq},
};

use crate::init;

const AFFECTED_CLIENT_ID: &str = "fbfa3d796fbded391207a2ef9b168b4b";
const AFFECTED_SN: &str = "7da297a9cc18825ab81ec69e1bd86ad05b4b81792b790dadcc8139ba1d2cc05c";

async fn init_api_wallet_backend()
-> Result<wallet_transport_backend::api::BackendApi, wallet_transport_backend::Error> {
    let backend_api = init(AFFECTED_SN)?;
    let req =
        ApiInitSwapReq { sn: AFFECTED_SN.to_string(), client_pub_key: GLOBAL_KEY.secret_pub_key() };
    let response = backend_api.init_swap(&req).await?;
    if let Some(data) = response.data {
        GLOBAL_KEY.set_shared_secret(&data.pub_key)?;
    }
    Ok(backend_api)
}

#[serial_test::serial]
#[tokio::test]
#[ignore = "live backend regression; run manually with --ignored --nocapture"]
async fn affected_client_can_query_unconfirmed_messages()
-> Result<(), wallet_transport_backend::Error> {
    let backend_api = init(AFFECTED_SN)?;

    backend_api
        .query_unconfirm_msg(&QueryUnconfirmMsgReq { client_id: AFFECTED_CLIENT_ID.to_string() })
        .await?;

    Ok(())
}

#[serial_test::serial]
#[tokio::test]
#[ignore = "live backend regression; run manually with --ignored --nocapture"]
async fn affected_client_can_request_expired_ack_resend()
-> Result<(), wallet_transport_backend::Error> {
    let backend_api = init_api_wallet_backend().await?;

    backend_api.msg_ack_expired_resend(MsgAckExpiredResendReq::new(AFFECTED_CLIENT_ID)).await?;

    Ok(())
}
