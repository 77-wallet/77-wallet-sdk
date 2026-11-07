use crate::{
    error::service::ServiceError,
    messaging::mqtt::{
        message::BizType, topics::api_wallet::address_insufficient::AwmAddressInsufficientBody,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wallet_database::{
    entities::api_business_notification::ApiBusinessNotification, pagination::Pagination,
};

/// Presentation fields only. Unsupported/malformed kinds retain metadata with null balance fields.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBusinessNotificationSummary {
    pub id: String,
    pub kind: String,
    pub is_read: bool,
    pub remaining: Option<String>,
    pub symbol: Option<String>,
    pub notified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBusinessNotificationDetail {
    pub id: String,
    pub kind: BizType,
    pub is_read: bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub remaining: String,
    pub token: String,
    pub symbol: String,
    pub merchant_name: String,
    pub threshold_value: String,
    pub target_address: String,
    pub chain_code: String,
    #[serde(rename = "waddressType")]
    pub w_address_type: String,
    pub notified_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalancePayload {
    #[serde(flatten)]
    body: AwmAddressInsufficientBody,
    notified_at: Option<DateTime<Utc>>,
}

fn balance_payload(row: &ApiBusinessNotification) -> Result<BalancePayload, ServiceError> {
    if BizType::from_business_notification_kind(&row.kind).is_none() {
        return Err(ServiceError::Parameter("unsupported business notification kind".into()));
    }
    let payload: BalancePayload = serde_json::from_str(&row.payload)
        .map_err(|_| ServiceError::Parameter("invalid balance notification payload".into()))?;
    if payload.body.address.trim().is_empty()
        || payload.body.chain_code.trim().is_empty()
        || payload.body.symbol.trim().is_empty()
    {
        return Err(ServiceError::Parameter(
            "balance notification chain, address, or symbol is empty".into(),
        ));
    }
    Ok(payload)
}

impl From<ApiBusinessNotification> for ApiBusinessNotificationSummary {
    fn from(row: ApiBusinessNotification) -> Self {
        let payload = balance_payload(&row).ok();
        Self {
            id: row.id,
            kind: row.kind,
            is_read: row.is_read,
            remaining: payload.as_ref().map(|p| p.body.remaining.to_string()),
            symbol: payload.as_ref().map(|p| p.body.symbol.clone()),
            // Old rows predate notifiedAt; only those valid legacy payloads use insertion time.
            notified_at: payload.map(|p| p.notified_at.unwrap_or(row.created_at)),
        }
    }
}

impl ApiBusinessNotificationDetail {
    pub(crate) fn from_record(row: ApiBusinessNotification) -> Result<Self, ServiceError> {
        let payload = balance_payload(&row)?;
        let body = payload.body;
        Ok(Self {
            id: row.id,
            kind: BizType::AwmAddressInsufficient,
            is_read: row.is_read,
            resolved_at: row.resolved_at,
            remaining: body.remaining.to_string(),
            token: body.token,
            symbol: body.symbol,
            merchant_name: body.merchant_name,
            threshold_value: body.threshold_value.to_string(),
            target_address: body.address,
            chain_code: body.chain_code,
            w_address_type: body.w_address_type,
            notified_at: payload.notified_at.unwrap_or(row.created_at),
        })
    }
}

pub(crate) fn summarize_page(
    page: Pagination<ApiBusinessNotification>,
) -> Pagination<ApiBusinessNotificationSummary> {
    Pagination {
        page: page.page,
        page_size: page.page_size,
        total_count: page.total_count,
        data: page.data.into_iter().map(Into::into).collect(),
    }
}

#[cfg(test)]
mod projection_tests;
