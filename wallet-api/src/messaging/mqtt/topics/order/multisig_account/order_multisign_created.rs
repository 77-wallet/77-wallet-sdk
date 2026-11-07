use crate::{
    context::Context,
    domain::multisig::MultisigDomain,
    messaging::notify::{
        FrontendNotifyEvent, event::NotifyEvent, multisig::OrderMultiSignCreatedFrontend,
    },
};
use wallet_database::{CoreDbPool, repositories::multisig_account::MultisigAccountRepo};

/*
    {
        "clientId": "666",
        "sn": "device458",
        "deviceType": "typeC",
        "bizType": "ORDER_MULTI_SIGN_CREATED",
        "body": {
            "multisigAccountId": "order-1",
            "multisigAccountAddress": "asdasdasdasd",
            "addressType": "p2wsh",
            "salt": "asdasd",
            "authorityAddr": "sadasdasd"
        }
    }
*/

// 服务费和部署完成后,所有参与方接受到的消息。
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OrderMultiSignCreated {
    /// 多签账户id
    pub multisig_account_id: String,
    /// 多签账户地址
    pub multisig_account_address: String,
    /// 地址类型
    pub address_type: String,
    /// btc solana 盐
    pub salt: String,
    /// solana 管理地址
    pub authority_addr: String,
    /// 部署的hash
    pub deploy_hash: String,
    /// 服务费hash
    pub fee_hash: String,
    pub fee_chain: Option<String>,
}

impl OrderMultiSignCreated {
    pub(crate) fn name(&self) -> String {
        "ORDER_MULTI_SIGN_CREATED".to_string()
    }
}

impl OrderMultiSignCreated {
    pub(crate) async fn exec_with_ctx(
        &self,
        _msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let event_name = self.name();
        let pool = ctx.get_global_sqlite_pool()?;
        tracing::info!(
            event_name = %event_name,
            ?self,
            "Starting to process OrderMultiSignCreated"
        );
        let OrderMultiSignCreated {
            multisig_account_id,
            multisig_account_address,
            address_type,
            salt,
            authority_addr,
            deploy_hash,
            fee_hash,
            fee_chain,
        } = &self;

        let core_pool = CoreDbPool::new(pool.clone());
        if MultisigAccountRepo::find_by_id(&core_pool, multisig_account_id).await?.is_none() {
            MultisigDomain::recover_multisig_account_by_id_with_ctx(ctx, multisig_account_id)
                .await?;
        }

        // update multisig account data
        MultisigAccountRepo::update_multisig_address(
            &core_pool,
            multisig_account_id,
            multisig_account_address,
            salt,
            authority_addr,
            address_type,
            deploy_hash,
            fee_hash,
            fee_chain.clone(),
        )
        .await
        .map_err(crate::error::service::ServiceError::Database)?;

        let account = MultisigAccountRepo::find_by_id(&core_pool, multisig_account_id)
            .await
            .map_err(crate::error::service::ServiceError::Database)?;

        if let Some(account) = account {
            // 初始化资产
            crate::domain::assets::AssetsDomain::init_default_multisig_assets(
                ctx,
                multisig_account_address.clone(),
                account.chain_code.clone(),
            )
            .await?;
        }

        let data = NotifyEvent::OrderMultiSignCreated(OrderMultiSignCreatedFrontend {
            multisig_account_id: multisig_account_id.to_string(),
            multisig_account_address: multisig_account_address.to_string(),
            address_type: address_type.to_string(),
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
    use crate::testkit::env::get_manager;

    #[tokio::test]
    async fn update_multisig_address() -> anyhow::Result<()> {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (manager, _) = get_manager().await?;

        let pool = manager.ctx.get_global_sqlite_pool()?;
        // 准备测试数据
        // let multisig_account_id = uuid::Uuid::new_v4(); // 生成一个新的 UUID 作为测试用的账户 ID
        let multisig_account_id = "216422221999116288";
        let multisig_account_address = "test_multisig_address".to_string();
        let salt = "random_salt".to_string();
        let authority_addr = "我是一个地址".to_string();
        let address_type = 1; // 假设 address_type 是一个整数
        let deploy_hash = "xxx".to_string();
        let fee_hash = "bb".to_string();
        let fee_chain = None;

        let core_pool = wallet_database::CoreDbPool::new(pool.clone());
        wallet_database::repositories::multisig_account::MultisigAccountRepo::update_multisig_address(
            &core_pool,
            &multisig_account_id.to_string(),
            &multisig_account_address,
            &salt,
            &authority_addr,
            &address_type.to_string(),
            &deploy_hash,
            &fee_hash,
            fee_chain,
        )
        .await?;
        Ok(())
    }
}
