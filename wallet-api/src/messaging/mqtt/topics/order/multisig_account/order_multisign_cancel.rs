use crate::context::Context;
use wallet_database::{CoreDbPool, repositories::multisig_account::MultisigAccountRepo};

use crate::messaging::notify::{
    FrontendNotifyEvent, event::NotifyEvent, multisig::OrderMultisignCanceledFrontend,
};

// 发起方取消多签账号消息，参与方同步自己多签账号的状态
// biz_type = ORDER_MULTI_SIGN_CANCEL
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignCancel {
    // 多签账户id
    multisig_account_id: String,
}

impl OrderMultiSignCancel {
    pub(crate) fn name(&self) -> String {
        "ORDER_MULTI_SIGN_CANCEL".to_string()
    }
}

impl OrderMultiSignCancel {
    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let event_name = self.name();
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = CoreDbPool::new(pool.clone());
        tracing::info!(
            event_name = %event_name,
            ?self,
            "Starting to process OrderMultiSignCancel"
        );
        let &OrderMultiSignCancel { ref multisig_account_id } = self;

        let multisig_account = MultisigAccountRepo::find_by_id(&core_pool, multisig_account_id)
            .await?
            .ok_or(crate::error::service::ServiceError::Business(
                crate::error::business::multisig_account::MultisigAccountError::NotFound.into(),
            ))?;

        // check
        MultisigAccountRepo::delete_in_status(&core_pool, multisig_account_id).await?;

        let data = NotifyEvent::OrderMultisignCanceled(OrderMultisignCanceledFrontend {
            multisig_account_id: multisig_account.id,
            multisig_account_address: multisig_account.address,
            address_type: multisig_account.address_type,
        });
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;

        Ok(())
    }

    pub(crate) async fn exec(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.exec_with_ctx(_msg_id, ctx).await
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {
    use crate::{messaging::mqtt::topics::OrderMultiSignCancel, testkit::env::get_manager};

    #[tokio::test]
    async fn test_() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        let (manager, _) = get_manager().await.unwrap();

        let raw = r#"{"multisigAccountId": "256890128948137984"}"#;
        let res = serde_json::from_str::<OrderMultiSignCancel>(&raw).unwrap();

        let _c = res.exec("x", manager.ctx).await?;
        Ok(())
    }
}
