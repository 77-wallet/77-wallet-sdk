use crate::{consts::endpoint::api_wallet::MSG_ACK, request::api_wallet::msg::MsgAckReq};
use wallet_ecdh::GLOBAL_KEY;

use crate::{
    api::BackendApi, api_request::ApiBackendRequest,
    consts::endpoint::api_wallet::MSG_ACK_EXPIRED_RESEND,
    request::api_wallet::msg::MsgAckExpiredResendReq,
};

impl BackendApi {
    // api钱包查询链列表
    pub async fn msg_ack(&self, req: MsgAckReq) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;

        self.post_api_backend::<_, ()>(MSG_ACK, api_req).await?;
        Ok(())
    }

    pub async fn msg_ack_expired_resend(
        &self,
        req: MsgAckExpiredResendReq,
    ) -> Result<(), crate::Error> {
        GLOBAL_KEY.is_exchange_shared_secret()?;
        let api_req = ApiBackendRequest::new(req)?;

        self.post_api_backend::<_, ()>(MSG_ACK_EXPIRED_RESEND, api_req).await?;
        Ok(())
    }
}
