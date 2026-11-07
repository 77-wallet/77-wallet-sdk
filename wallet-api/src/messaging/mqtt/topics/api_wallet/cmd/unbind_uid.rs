use crate::{
    context::Context,
    domain::api_wallet::wallet::ApiWalletDomain,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
};

// biz_type = AWM_CMD_UID_UNBIND
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AwmCmdUidUnbindMsg {
    /// uid
    pub uid: String,
}

impl AwmCmdUidUnbindMsg {
    pub(crate) async fn exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        let Self { uid } = self;
        ApiWalletDomain::new(ctx).unbind_uid(uid).await?;
        let data = NotifyEvent::AwmCmdUidUnbind(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        Ok(())
    }
}
