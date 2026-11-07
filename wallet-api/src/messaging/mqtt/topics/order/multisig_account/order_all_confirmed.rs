use crate::{
    context::Context, domain::multisig::MultisigDomain,
    messaging::mqtt::topics::OrderMultiSignAcceptCompleteMsg,
};
use wallet_database::entities::multisig_account::MultisigAccountStatus;

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderAllConfirmed {
    /// 多签账户id
    multisig_account_id: String,
    /// 所有参与方地址
    members: Vec<wallet_transport_backend::ConfirmedAddress>,
}

impl OrderAllConfirmed {
    pub(crate) fn name(&self) -> String {
        "ORDER_MULTI_SIGN_ALL_MEMBER_ACCEPTED".to_string()
    }
}

impl OrderAllConfirmed {
    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let event_name = self.name();
        tracing::info!(
            event_name = %event_name,
            ?self,
            "Starting to process OrderAllConfirmed"
        );

        let account =
            MultisigDomain::check_multisig_account_exists_with_ctx(ctx, &self.multisig_account_id)
                .await?;

        let Some(_account) = account else {
            tracing::warn!(
                "[OrderAllConfirmed] faild account not found {}",
                self.multisig_account_id
            );
            return Ok(());
        };

        let _ = ctx;
        if _account.status == MultisigAccountStatus::Pending.to_i8() {
            OrderMultiSignAcceptCompleteMsg::all_members_confirmed_with_ctx(
                &self.members,
                &self.multisig_account_id,
                1,
                ctx,
            )
            .await?;
        }

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
    use crate::{messaging::mqtt::topics::OrderAllConfirmed, testkit::env::get_manager};

    #[tokio::test]
    async fn test_() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        let (manager, _) = get_manager().await.unwrap();

        let raw = r#"{"multisigAccountId":"257922832858746880","members":[{"address":"TKfzG9aNQ5vBNwQHGB3w7cCHkGZ6YQBcT9","pubkey":"0400D477DEE2BBDB5424E09DF5937099D61BBBC4087F27D1362D6368196987350981F7480663450DCF081A4A207101C25F8FEF5E3096F7211D4C12B112C337C009","status":1,"uid":"8d102007bae33499ccf614475195fa16bf2bdaa5778b8b1be3d3ce224ad8a451"},{"address":"TKKjkyjSMZ9iy8ATJsLp1X4yNqr39Q5v8Q","pubkey":"0494CB36619B3BEA08AF584CD66A343650930579AD88A00DD8EDE771579BBBF45AADB1E58677BD9ADAE6663F3195055890F527E457FB462F3E112E298B13AEE2AC","status":1,"uid":"3a0f935b5a44dd58812efde1a5175b975bc1368531f69e76315a98cbdb921923"}]}"#;
        let res = serde_json::from_str::<OrderAllConfirmed>(&raw).unwrap();

        let _c = res.exec("x", manager.ctx).await?;
        Ok(())
    }
}
