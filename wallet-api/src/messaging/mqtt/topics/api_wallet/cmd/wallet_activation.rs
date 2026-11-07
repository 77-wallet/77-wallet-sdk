use wallet_transport_backend::{
    request::api_wallet::msg::MsgAckReq, response_vo::api_wallet::wallet::ActiveStatus,
};

use crate::{
    context::Context,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
};

// biz_type = AWM_CMD_ACTIVE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwmCmdActiveMsg {
    #[serde(rename = "chain")]
    pub chain_code: String,
    pub uid: String,
    /// 激活状态: 0 未激活 /1已激活
    pub active: ActiveStatus,
}

impl AwmCmdActiveMsg {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let backend = ctx.get_global_backend_api();
        let mut msg_ack_req = MsgAckReq::default();
        msg_ack_req.push(_msg_id);
        backend.msg_ack(msg_ack_req).await?;
        let data = NotifyEvent::AwmCmdActive(self.into());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        // ApiWalletDomain::expand_address(&self.typ, self.index, &self.uid, &self.chain_code).await?;
        tracing::info!("WalletActivationMsg: {:?}", self);
        Ok(())
    }
}
