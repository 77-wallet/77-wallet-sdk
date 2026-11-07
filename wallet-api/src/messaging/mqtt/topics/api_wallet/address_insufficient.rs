use rust_decimal::Decimal;
use wallet_database::{
    ApiWalletDbPool,
    entities::{api_business_notification::NewApiBusinessNotification, task_queue::WalletType},
    repositories::api_business_notification::ApiBusinessNotificationRepo,
};

use crate::{
    error::service::ServiceError,
    messaging::{
        mqtt::message::{BizType, Message},
        notify::{FrontendNotifyEvent, event::NotifyEvent},
    },
};

/// Confirmed backend fields; amounts are coin quantities, not smallest units.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwmAddressInsufficientBody {
    pub(crate) remaining: Decimal,
    pub(crate) token: String,
    pub(crate) symbol: String,
    pub(crate) merchant_name: String,
    #[serde(rename = "waddressType")]
    pub(crate) w_address_type: String,
    pub(crate) threshold_value: Decimal,
    pub(crate) chain_code: String,
    #[serde(rename = "targetAddress")]
    pub(crate) address: String,
}

/// Carry the outer UID into the durable task body before the MQTT envelope is dropped.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct AwmAddressInsufficient {
    pub(crate) uid: String,
    pub(crate) body: AwmAddressInsufficientBody,
    #[serde(default)]
    pub(crate) received_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AwmAddressInsufficient {
    pub(crate) fn from_message(message: &Message) -> Result<Self, ServiceError> {
        if !matches!(message.biz_type, BizType::AwmAddressInsufficient)
            || message.wallet_type != Some(WalletType::ApiWaw)
        {
            return Err(ServiceError::Parameter(
                "balance alert must target an API withdrawal wallet".into(),
            ));
        }
        Self::validate_msg_id(&message.msg_id)?;
        let uid = message
            .uid
            .as_deref()
            .filter(|uid| !uid.trim().is_empty())
            .ok_or_else(|| ServiceError::Parameter("balance alert UID is missing".into()))?
            .to_owned();
        let body: AwmAddressInsufficientBody = serde_json::from_value(message.body.clone())
            .map_err(|e| ServiceError::Parameter(format!("invalid balance alert body: {e}")))?;
        if body.chain_code.trim().is_empty()
            || body.address.trim().is_empty()
            || body.symbol.trim().is_empty()
        {
            return Err(ServiceError::Parameter(
                "balance alert chain, address, or symbol is empty".into(),
            ));
        }
        Ok(Self { uid, body, received_at: Some(chrono::Utc::now()) })
    }

    pub(crate) async fn exec(
        &self,
        ctx: &'static crate::context::Context,
        msg_id: &str,
    ) -> Result<(), ServiceError> {
        let pool = ctx.api_wallet_pool()?;
        let key = self.persist(&pool, msg_id).await?;
        // Publish only durable state, before optional balance-check scheduling can fail.
        // Delivery is best effort: an unavailable App must not fail persisted MQTT work.
        if let Err(error) = FrontendNotifyEvent::new(NotifyEvent::ApiWalletNotificationChanged {})
            .send_with_ctx(ctx)
            .await
        {
            tracing::warn!(%error, "failed to publish API wallet notification refresh");
        }
        if let Some(row) = ApiBusinessNotificationRepo::find(&pool, &self.uid, &key).await? {
            crate::application::api_wallet_balance_check::enqueue(ctx, &row, None).await?;
        }
        Ok(())
    }

    fn validate_msg_id(msg_id: &str) -> Result<(), ServiceError> {
        if msg_id.trim().is_empty() {
            return Err(ServiceError::Parameter("balance alert msgId is empty".into()));
        }
        Ok(())
    }

    /// Reconstruct the previous ID only to locate immutable historical rows.
    fn legacy_event_id(&self, msg_id: &str) -> String {
        let encode = |value: &str| {
            value.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        };
        let kind = BizType::AwmAddressInsufficient
            .business_notification_kind()
            .expect("supported notification kind");
        format!("mqtt-event-v1/{}/{}/{}", encode(&self.uid), encode(kind), encode(msg_id))
    }

    async fn persist(&self, pool: &ApiWalletDbPool, msg_id: &str) -> Result<String, ServiceError> {
        Self::validate_msg_id(msg_id)?;
        let legacy_id = self.legacy_event_id(msg_id);
        if ApiBusinessNotificationRepo::find(pool, &self.uid, &legacy_id).await?.is_some() {
            return Ok(legacy_id);
        }
        // Backend msgId is globally unique and stable on replay; preserve its exact bytes.
        let key = format!(
            "{}:{}:{}:{}",
            self.uid, self.body.chain_code, self.body.address, self.body.token
        );
        let mut payload_body = serde_json::to_value(&self.body)
            .map_err(|e| ServiceError::Parameter(format!("invalid balance alert payload: {e}")))?;
        payload_body["notifiedAt"] =
            serde_json::json!(self.received_at.unwrap_or_else(chrono::Utc::now));
        let payload = serde_json::to_string(&payload_body)
            .map_err(|e| ServiceError::Parameter(format!("invalid balance alert payload: {e}")))?;
        ApiBusinessNotificationRepo::ingest(
            pool,
            NewApiBusinessNotification {
                id: msg_id,
                uid: &self.uid,
                kind: BizType::AwmAddressInsufficient
                    .business_notification_kind()
                    .expect("supported notification kind"),
                business_key: &key,
                payload: &payload,
            },
        )
        .await?;
        Ok(msg_id.to_owned())
    }
}

#[cfg(test)]
#[path = "address_insufficient/event_id_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "address_insufficient/notification_event_tests.rs"]
mod notification_event_tests;
