use crate::{
    context::Context,
    domain::api_wallet::account::ApiAccountDomain,
    messaging::notify::{FrontendNotifyEvent, event::NotifyEvent},
};

// biz_type = ADDRESS_USE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddressUseItem {
    /// uid
    pub uid: String,
    #[serde(rename = "chain")]
    pub chain_code: String,
    pub index: i32,
    pub client_id: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct AddressUseMsg(Vec<AddressUseItem>);

impl AddressUseMsg {
    pub(crate) async fn _exec(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        self._exec_with_ctx(ctx, _msg_id).await
    }

    pub(crate) async fn _exec_with_ctx(
        &self,
        ctx: &'static Context,
        _msg_id: &str,
    ) -> Result<(), crate::error::service::ServiceError> {
        for item in self.0.iter() {
            ApiAccountDomain::address_used_with_ctx(ctx, &item.chain_code, item.index, &item.uid)
                .await?;
        }

        let data = NotifyEvent::AddressUse(self.to_owned());
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        Ok(())
    }
}
