//! Test-only MQTT message execution helpers.

use crate::context::Context;
use rumqttc::v5::mqttbytes::v5::Publish;
use wallet_database::{
    ApiResourceDbPool, ApiTransactionDbPool, ApiWalletDbPool, DbPool, TaskDbPool,
};

pub async fn exec_wallet_order_payload(
    ctx: &'static Context,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let publish = Publish {
        dup: false,
        qos: rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
        retain: false,
        topic: "wallet/order".into(),
        pkid: 0,
        payload: serde_json::to_vec(payload)?.into(),
        properties: Default::default(),
    };
    crate::messaging::mqtt::handle::exec_incoming_publish(ctx, &publish).await
}

pub fn api_wallet_pool(
    ctx: &'static Context,
) -> Result<ApiWalletDbPool, crate::error::service::ServiceError> {
    ctx.api_wallet_pool()
}

pub fn api_transaction_pool(
    ctx: &'static Context,
) -> Result<ApiTransactionDbPool, crate::error::service::ServiceError> {
    ctx.api_transaction_pool()
}

pub fn api_resource_pool(
    ctx: &'static Context,
) -> Result<ApiResourceDbPool, crate::error::service::ServiceError> {
    ctx.api_resource_pool()
}

pub fn task_pool(ctx: &'static Context) -> Result<TaskDbPool, crate::error::service::ServiceError> {
    ctx.task_pool()
}

pub fn core_pool(ctx: &'static Context) -> Result<DbPool, crate::error::service::ServiceError> {
    ctx.get_global_sqlite_pool()
}

/// Manual smoke replay of the crate-private account-change step; does not change its behavior.
pub async fn replay_api_wallet_account_change(
    ctx: &'static Context,
    payload: &serde_json::Value,
    msg_id: &str,
) -> anyhow::Result<()> {
    let message = serde_json::from_value::<
        crate::messaging::mqtt::topics::api_wallet::acct_change::ApiWalletAcctChange,
    >(payload.clone())?;
    message.exec(ctx, msg_id).await?;
    Ok(())
}

/// Manual smoke replay of the crate-private address-expansion step.
pub async fn replay_api_wallet_address_expand(
    ctx: &'static Context,
    payload: &serde_json::Value,
    msg_id: &str,
) -> anyhow::Result<()> {
    let message = serde_json::from_value::<
        crate::messaging::mqtt::topics::api_wallet::cmd::address_allock::AwmCmdAddrExpandMsg,
    >(payload.clone())?;
    message.exec(ctx, msg_id).await?;
    Ok(())
}
