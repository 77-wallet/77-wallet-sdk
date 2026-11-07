use crate::{
    error::service::ServiceError,
    messaging::mqtt::message::{BizType, Message},
};
use wallet_database::entities::task_queue::WalletType;

/// Invalidation signal: fetch the authoritative snapshot using the inner merchant scope.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwmCfgChange {
    cfg_type: u8,
    app_id: String,
    org_id: String,
    sn: String,
    wuid: String,
}

impl AwmCfgChange {
    pub(crate) fn from_message(message: &Message) -> Result<Self, ServiceError> {
        let change: Self = serde_json::from_value(message.body.clone()).map_err(|_| {
            ServiceError::Parameter("invalid application config change body".into())
        })?;
        if !matches!(message.biz_type, BizType::AwmCfgChange)
            || message.wallet_type != Some(WalletType::ApiWaw)
            || message.msg_id.trim().is_empty()
            || change.app_id.trim().is_empty()
            || change.org_id.trim().is_empty()
            || change.wuid.trim().is_empty()
            || change.sn.trim().is_empty()
            || message.uid.as_deref() != Some(change.wuid.as_str())
            || message.sn != change.sn
        {
            return Err(ServiceError::Parameter("invalid application config change scope".into()));
        }
        Ok(change)
    }

    pub(crate) async fn exec(
        &self,
        ctx: &'static crate::context::Context,
        _msg_id: &str,
    ) -> Result<(), ServiceError> {
        if self.cfg_type != 1 {
            return Ok(());
        }
        crate::service::api_wallet::app_warning_config::AppWarningConfigService::new(ctx)
            .sync_for_wallet_scope(&self.wuid, &self.org_id, &self.app_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wire() -> serde_json::Value {
        serde_json::json!({"msgId":"cfg-1","appId":"device-app","bizType":"AWM_CFG_CHANGE",
            "body":{"cfgType":1,"appId":"merchant-app","orgId":"org","sn":"device","wuid":"wallet"},
            "clientId":"client","deviceType":"ANDROID","sn":"device","uid":"wallet","walletType":"API_WAW"})
    }
    #[tokio::test]
    async fn unrelated_config_type_does_not_query_backend() {
        let (_dir, ctx) = crate::service::api_wallet::business_notification::global_business_query_tests::isolated_context().await;
        let mut raw = wire();
        raw["body"]["cfgType"] = serde_json::json!(2);
        let change = AwmCfgChange::from_message(&serde_json::from_value(raw).unwrap()).unwrap();
        change.exec(ctx, "cfg-1").await.unwrap();
    }

    #[test]
    fn cfg_change_validates_envelope_scope_and_preserves_durable_body() {
        let message: Message = serde_json::from_value(wire()).unwrap();
        let change = AwmCfgChange::from_message(&message).unwrap();
        assert_eq!(change.app_id, "merchant-app");
        let restored: AwmCfgChange =
            serde_json::from_str(&serde_json::to_string(&change).unwrap()).unwrap();
        assert_eq!(restored.wuid, "wallet");
        for (field, value) in
            [("uid", "other"), ("sn", "other"), ("msgId", ""), ("walletType", "API_SUB")]
        {
            let mut raw = wire();
            raw[field] = serde_json::json!(value);
            if let Ok(message) = serde_json::from_value::<Message>(raw) {
                assert!(AwmCfgChange::from_message(&message).is_err());
            }
        }
        let mut raw = wire();
        raw.as_object_mut().unwrap().remove("msgId");
        assert!(serde_json::from_value::<Message>(raw).is_err());
    }
}
