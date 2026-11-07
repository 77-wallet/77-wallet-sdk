use std::collections::HashSet;

use wallet_database::{
    CoreDbPool, DbPool,
    entities::{
        asset_token_key::AssetTokenKey,
        bill::{BillExtraSwap, BillKind, NewBillEntity},
        multisig_queue::MultisigQueueStatus,
    },
    repositories::{
        bill::BillRepo, multisig_queue::MultisigQueueRepo,
        system_notification::SystemNotificationRepo,
    },
};
use wallet_types::constant::chain_code;

use crate::{
    context::Context,
    domain::{bill::BillDomain, multisig::MultisigQueueDomain},
    infrastructure::inner_event::{InnerEvent, SyncAssetsData},
    messaging::{
        notify::{FrontendNotifyEvent, event::NotifyEvent, transaction::AcctChangeFrontend},
        system_notification::{AccountType, Notification, NotificationType, TransactionStatus},
    },
    service::system_notification::SystemNotificationService,
};

// biz_type = ACCT_CHANGE
#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcctChange {
    // 交易hash
    pub tx_hash: String,
    // 链码
    pub chain_code: String,
    // 币种符号
    #[serde(deserialize_with = "wallet_utils::serde_func::deserialize_uppercase")]
    pub symbol: String,
    // 交易方式 0转入 1转出 2初始化
    pub transfer_type: i8,
    // 交易类型 1:普通交易，2:部署多签账号 3:服务费
    pub tx_kind: i8,
    // 发起方
    pub from_addr: String,
    // 接收方
    #[serde(default)]
    pub to_addr: String,
    // 合约地址
    #[serde(default)]
    pub token: Option<String>,
    // 交易额
    #[serde(default)]
    pub value: f64,
    // 手续费
    pub transaction_fee: f64,
    // 交易时间
    #[serde(default)]
    pub transaction_time: String,
    // 交易状态 true-成功 false-失败
    pub status: bool,
    // 是否多签 1-是，0-否
    #[serde(default)]
    pub is_multisig: i32,
    // 队列id
    #[serde(default)]
    pub queue_id: String,
    // 块高
    pub block_height: i64,
    // 备注
    #[serde(default)]
    pub notes: String,
    // 带宽消耗
    #[serde(default)]
    pub net_used: u64,
    // 能量消耗
    #[serde(default)]
    pub energy_used: Option<u64>,

    // 额外信息
    pub extra: Option<serde_json::Value>,
}

impl TryFrom<&AcctChange> for NewBillEntity<serde_json::Value> {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &AcctChange) -> Result<Self, Self::Error> {
        let tx_kind = BillKind::try_from(value.tx_kind)?;
        let status = if value.status { 2 } else { 3 };

        let consumer = wallet_chain_interact::BillResourceConsume::new_tron(
            value.net_used,
            value.energy_used.unwrap_or_default(),
        )
        .to_json_str()?;

        let mut bill = BillRepo::build_bill_with_extra::<serde_json::Value>(
            value.tx_hash.clone(),
            value.from_addr.clone(),
            value.to_addr.clone(),
            value.value,
            value.chain_code.clone(),
            value.symbol.clone(),
            value.is_multisig == 1,
            tx_kind,
            value.notes.clone(),
        );
        bill.tx_type = value.transfer_type;
        bill.token = AssetTokenKey::from(value.token.clone());
        bill.transaction_fee = value.transaction_fee.to_string();
        bill.transaction_time = wallet_utils::time::datetime_to_timestamp(&value.transaction_time);
        bill.status = status;
        bill.queue_id = value.queue_id.clone();
        bill.block_height = value.block_height.to_string();
        bill.resource_consume = consumer;
        bill.extra = value.extra.clone();

        Ok(bill)
    }
}

impl From<&AcctChange> for AcctChangeFrontend {
    fn from(value: &AcctChange) -> Self {
        Self {
            tx_hash: value.tx_hash.clone(),
            chain_code: value.chain_code.clone(),
            symbol: value.symbol.clone(),
            transfer_type: value.transfer_type,
            tx_kind: value.tx_kind,
            from_addr: value.from_addr.clone(),
            to_addr: value.to_addr.clone(),
            token: value.token.clone().into(),
            value: value.value,
            transaction_fee: value.transaction_fee,
            transaction_time: value.transaction_time.clone(),
            status: value.status,
            is_multisig: value.is_multisig,
            queue_id: value.queue_id.clone(),
            block_height: value.block_height,
            notes: value.notes.clone(),
        }
    }
}

impl AcctChange {
    pub(crate) async fn exec(
        &self,
        msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        self.exec_with_ctx(msg_id, ctx).await
    }

    pub(crate) async fn exec_with_ctx(
        &self,
        msg_id: &str,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let pool = ctx.get_global_sqlite_pool()?;
        let core_pool = wallet_database::CoreDbPool::new(pool.clone());

        // bill create
        let tx = NewBillEntity::<serde_json::Value>::try_from(self)?;
        let tx_kind = tx.tx_kind;

        if tx.chain_code == chain_code::TON {
            Self::handle_ton_bill(tx, &pool).await?;
        } else {
            BillDomain::create_check_swap(tx, &core_pool).await?;
            // BillRepo::create(tx, &core_pool).await?;
        }

        if !self.queue_id.is_empty() {
            Self::handle_queue(&self, &pool, ctx).await?;
        }

        // 更新资产,不进行新增(垃圾币)
        Self::sync_assets_with_ctx(&self, ctx).await?;

        // 创建系统通知
        if tx_kind.needs_system_notify()
            && self.value != 0.0
            && !self.chain_code.is_empty()
            && !self.to_addr.is_empty()
            && !self.from_addr.is_empty()
        {
            Self::system_notification(msg_id, self, &pool, ctx).await?;
        }

        // send acct_change to frontend
        let change_frontend = AcctChangeFrontend::from(self);
        let data = NotifyEvent::AcctChange(change_frontend);
        FrontendNotifyEvent::new(data).send_with_ctx(ctx).await?;
        Ok(())
    }

    async fn handle_queue(
        change: &AcctChange,
        pool: &DbPool,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        let status =
            if change.status { MultisigQueueStatus::Success } else { MultisigQueueStatus::Fail };

        MultisigQueueRepo::update_status_hash(
            &change.queue_id,
            status,
            &change.tx_hash,
            &core_pool,
        )
        .await?;

        // 多签队列不存在可以允许 不上报忽略
        let rs = MultisigQueueDomain::update_raw_data_with_ctx(ctx, &change.queue_id, pool.clone())
            .await;
        match rs {
            Ok(_) => {}
            Err(e) => {
                if !matches!(e, crate::error::service::ServiceError::Database(_)) {
                    return Err(e);
                };
                tracing::error!(%e, "acct_change update queue fail");
            }
        }

        Ok(())
    }

    async fn sync_assets_with_ctx(
        acct_change: &AcctChange,
        ctx: &'static Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        if !acct_change.status {
            tracing::warn!("acct_change status is false, skip sync assets");
            return Ok(());
        }
        let handles = ctx.get_global_handles().await;
        if let Some(handles) = handles.upgrade() {
            let inner_event_handle = handles.get_global_inner_event_handle();
            let addr_list = vec![acct_change.from_addr.clone(), acct_change.to_addr.clone()];
            for token_key in acct_change.get_sync_asset_token_keys() {
                let data = SyncAssetsData::new_with_token_key(
                    addr_list.clone(),
                    acct_change.chain_code.clone(),
                    token_key,
                );
                inner_event_handle.send(InnerEvent::SyncAssets(data))?;
            }
        } else {
            tracing::warn!("acct_change status is false, skip sync assets");
        }
        // tracing::info!("发送同步资产事件");
        Ok(())
    }

    // swap 需要同步 from/to 两个 token，对应两个 token-key 事件
    fn get_sync_asset_token_keys(&self) -> Vec<AssetTokenKey> {
        let mut ordered = vec![AssetTokenKey::from_raw(self.token.as_deref())];

        if self.tx_kind == BillKind::Swap.to_i8()
            && let Some(extra) = &self.extra
            && let Ok(extra_swap) =
                wallet_utils::serde_func::serde_from_value::<BillExtraSwap>(extra.clone())
        {
            ordered.push(AssetTokenKey::from_raw(Some(extra_swap.from_token_address.as_str())));
            ordered.push(AssetTokenKey::from_raw(Some(extra_swap.to_token_address.as_str())));
        }

        let mut seen = HashSet::new();
        ordered.into_iter().filter(|key| seen.insert(key.clone())).collect()
    }

    pub async fn handle_ton_bill(
        mut tx: NewBillEntity<serde_json::Value>,
        pool: &DbPool,
    ) -> Result<(), crate::error::service::ServiceError> {
        let origin_hash = tx.hash.clone();
        let hashs = origin_hash.split(":").collect::<Vec<_>>();

        if hashs.len() == 2 {
            tx.hash = hashs[0].to_string();
            let in_hash = hashs[1];
            let core_pool = CoreDbPool::new(pool.clone());
            if let Some(bill) =
                BillRepo::get_by_hash_and_type(in_hash, tx.tx_type as i64, &core_pool).await?
            {
                BillRepo::update_all(&core_pool, tx, bill.id).await?;
            } else {
                BillRepo::create(tx, &core_pool).await?;
            }
        } else {
            let core_pool = CoreDbPool::new(pool.clone());
            BillRepo::create(tx, &core_pool).await?;
        }

        Ok(())
    }

    async fn system_notification(
        msg_id: &str,
        acct_change: &AcctChange,
        pool: &DbPool,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        let core_pool = CoreDbPool::new(pool.clone());
        let transaction_hash = BillDomain::handle_hash(&acct_change.tx_hash);
        if let Some(bill) = BillRepo::get_by_hash_opt(&acct_change.tx_hash, &core_pool).await? {
            if bill.tx_kind == BillKind::Swap.to_i8() {
                return Ok(());
            }
        }

        // 交易方式 0转入 1转出 2初始化
        let address = match acct_change.transfer_type {
            0 => acct_change.to_addr.as_str(),
            1 => acct_change.from_addr.as_str(),
            _ => return Ok(()),
        };

        // check system notification exists
        if SystemNotificationRepo::find_by_id(msg_id, &core_pool).await?.is_some() {
            tracing::warn!("system_noti already exists");
            return Ok(());
        }

        let (transaction_status, notification_type) =
            Self::get_notify_status(acct_change.transfer_type, acct_change.status)?;

        let account_type =
            if acct_change.is_multisig == 1 { AccountType::Multisig } else { AccountType::Regular };

        // build notify
        let notify = Notification::new_transaction_notification(
            account_type,
            "",
            address,
            &acct_change.from_addr,
            &acct_change.to_addr,
            acct_change.value,
            &acct_change.symbol,
            &acct_change.chain_code,
            &transaction_status,
            &transaction_hash,
            acct_change.token.clone(),
            &notification_type,
        );
        let req = notify.gen_create_system_notification_entity(
            msg_id,
            0,
            Some("tx_hash".to_string()),
            Some(transaction_hash),
        )?;

        let system_notification_service = SystemNotificationService::new(ctx);

        system_notification_service.add_multi_system_notification_with_key_value(&[req]).await?;
        Ok(())
    }

    fn get_notify_status(
        transfer_type: i8,
        status: bool,
    ) -> Result<(TransactionStatus, NotificationType), crate::error::service::ServiceError> {
        let (transaction_status, notification_type) = match (transfer_type, status) {
            (0, true) => (TransactionStatus::Received, NotificationType::ReceiveSuccess),
            (1, true) => (TransactionStatus::Sent, NotificationType::TransferSuccess),
            (1, false) => (TransactionStatus::NotSent, NotificationType::TransferFailure),
            (_, _) => {
                return Err(crate::error::service::ServiceError::Parameter(
                    "invaild status".to_string(),
                ));
            }
        };

        Ok((transaction_status, notification_type))
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod test {

    use crate::{messaging::mqtt::topics::AcctChange, testkit::env::get_manager};

    async fn init_manager() -> crate::manager::WalletManager {
        wallet_utils::init_test_log();
        // 修改返回类型为Result<(), anyhow::Error>
        let (manager, _) = get_manager().await.unwrap();
        manager
    }

    // 普通账交易
    #[tokio::test]
    async fn acct_change() -> anyhow::Result<()> {
        let manager = init_manager().await;

        let change = r#"{"txHash":"c357a09e84a6dd1ad0d621641320f505fd23bc3c48251a5d524fd281de2870da:ftIuBQWDNv8Ik9FQy8aUIfzdrTbennywxOCmw6Ury1A=","chainCode":"ton","symbol":"TON","transferType":0,"txKind":1,"fromAddr":"UQDaL1eH_9TU3hceiO7ZsPDEdcmwDhZ0eDZ_NCOIrmjHoSQb","toAddr":"UQAJr_aCqkWARCMkTHYkpKL9B-kYOFvXxvyDumUXsZ79ZnYY","token":"","value":0.01,"transactionFee":0.002432489,"transactionTime":"2025-06-17 08:53:28","status":true,"isMultisig":0,"queueId":"","blockHeight":48927711,"notes":"","netUsed":0,"energyUsed":null}"#;
        let change = serde_json::from_str::<AcctChange>(&change).unwrap();
        let _res = change.exec("2", manager.ctx).await.unwrap();

        let change = r#"{"txHash":"c357a09e84a6dd1ad0d621641320f505fd23bc3c48251a5d524fd281de2870da:ftIuBQWDNv8Ik9FQy8aUIfzdrTbennywxOCmw6Ury1A=","chainCode":"ton","symbol":"TON","transferType":1,"txKind":1,"fromAddr":"UQDaL1eH_9TU3hceiO7ZsPDEdcmwDhZ0eDZ_NCOIrmjHoSQb","toAddr":"UQAJr_aCqkWARCMkTHYkpKL9B-kYOFvXxvyDumUXsZ79ZnYY","token":"","value":0.01,"transactionFee":0.002432489,"transactionTime":"2025-06-17 08:53:28","status":true,"isMultisig":0,"queueId":"","blockHeight":48927711,"notes":"","netUsed":0,"energyUsed":null}"#;
        let change = serde_json::from_str::<AcctChange>(&change).unwrap();

        let _res = change.exec("1", manager.ctx).await.unwrap();
        Ok(())
    }
}
