use super::{
    multisig::{
        OrderMultiSignAcceptCompleteMsgFrontend, OrderMultiSignAcceptFrontend,
        OrderMultiSignCreatedFrontend, OrderMultiSignServiceCompleteFrontend,
        OrderMultisignCanceledFrontend,
    },
    other::{
        ChainChangeFrontend, ConnectionErrorFrontend, DebugFront, ErrFront,
        TransactionProcessFrontend,
    },
    permission::PermissionChangeFrontend,
    transaction::{
        AcctChangeFrontend, ConfirmationFrontend, MultiSignTransAcceptCompleteMsgFrontend,
    },
};
use crate::messaging::{
    mqtt::topics::{
        BulletinMsg,
        api_wallet::{
            cmd::{
                address_use::AddressUseMsg, dev_change::AwmCmdDevChangeMsg,
                fee_res::AwmCmdFeeResMsg, unbind_uid::AwmCmdUidUnbindMsg,
            },
            trans_result::AwmOrderTransResMsg,
        },
    },
    notify::api_wallet::{
        ApiWalletSyncAssetsMsgFront, AwmCmdActiveMsgFront, AwmCmdAddrExpandMsgFront,
        AwmOrderTransMsgFront, CollectFeeNotEnoughFront, CollectFront, FeeFront, WithdrawFront,
        WithdrawNoPassFront,
    },
};

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub enum NotifyEvent {
    OrderMultiSignAccept(OrderMultiSignAcceptFrontend),
    OrderMultiSignAcceptCompleteMsg(OrderMultiSignAcceptCompleteMsgFrontend),
    OrderMultiSignServiceComplete(OrderMultiSignServiceCompleteFrontend),
    OrderMultiSignCreated(OrderMultiSignCreatedFrontend),
    OrderMultisignCanceled(OrderMultisignCanceledFrontend),
    Confirmation(ConfirmationFrontend),
    MultiSignTransAcceptCompleteMsg(MultiSignTransAcceptCompleteMsgFrontend),
    AcctChange(AcctChangeFrontend),
    TokenPriceChange(crate::response_vo::standard_wallet::coin::TokenPriceChangeRes),
    // Init(Init),
    BulletinMsg(BulletinMsg),

    FetchBulletinMsg,
    MqttConnected,
    MqttDisconnected,
    KeepAlive,
    ConnectionError(ConnectionErrorFrontend),
    Debug(DebugFront),
    Err(ErrFront),

    // 执行交易的过程
    TransactionProcess(TransactionProcessFrontend),
    ChainChange(ChainChangeFrontend),

    // 资源
    // ResourceChange(ResourceChangeFrontend),
    // 权限变更事件
    PermissionChanger(PermissionChangeFrontend),
    // 恢复多签数据完成
    RecoverComplete,
    // 多签交易取消
    MultisigTransCancel,
    // 多签交易执行
    MultiSignTransExecute,

    // 其他
    // 同步资产
    SyncAssets,
    ApiWalletSyncAssets(ApiWalletSyncAssetsMsgFront),

    // API wallet
    AwmCmdActive(AwmCmdActiveMsgFront),
    AwmCmdUidUnbind(AwmCmdUidUnbindMsg),
    AwmCmdAddrExpand(AwmCmdAddrExpandMsgFront),
    AwmCmdFeeRes(AwmCmdFeeResMsg),
    AwmOrderTrans(AwmOrderTransMsgFront),
    AwmOrderTransRes(AwmOrderTransResMsg),
    AddressUse(AddressUseMsg),
    Withdraw(WithdrawFront),
    WithdrawNoPass(WithdrawNoPassFront),
    Collect(CollectFront),
    CollectFeeNotEnough(CollectFeeNotEnoughFront),
    Fee(FeeFront),
    AddressRecovery(AwmCmdAddrExpandMsgFront),
    AwmCmdDevChange(AwmCmdDevChangeMsg),
    ApiWalletAcctChange(AcctChangeFrontend),
    /// API wallet notification invalidation; consumers reload lists and unread count.
    ApiWalletNotificationChanged {},
}

impl NotifyEvent {
    pub(crate) fn event_name(&self) -> String {
        match self {
            NotifyEvent::ApiWalletNotificationChanged {} => {
                "API_WALLET_NOTIFICATION_CHANGED".to_string()
            }
            NotifyEvent::OrderMultiSignAccept(_) => "ORDER_MULTI_SIGN_ACCEPT".to_string(),
            NotifyEvent::OrderMultiSignAcceptCompleteMsg(_) => {
                "ORDER_MULTI_SIGN_ACCEPT_COMPLETE_MSG".to_string()
            }
            NotifyEvent::OrderMultiSignServiceComplete(_) => {
                "ORDER_MULTI_SIGN_SERVICE_COMPLETE".to_string()
            }
            NotifyEvent::OrderMultiSignCreated(_) => "ORDER_MULTI_SIGN_CREATED".to_string(),
            NotifyEvent::OrderMultisignCanceled(_) => "ORDER_MULTI_SIGN_CANCEL".to_string(),
            NotifyEvent::Confirmation(_) => "CONFIRMATION".to_string(),
            NotifyEvent::MultiSignTransAcceptCompleteMsg(_) => {
                "MULTI_SIGN_TRANS_ACCEPT_COMPLETE_MSG".to_string()
            }
            NotifyEvent::AcctChange(_) => "ACCT_CHANGE".to_string(),
            NotifyEvent::TokenPriceChange(_) => "TOKEN_PRICE_CHANGE".to_string(),
            NotifyEvent::BulletinMsg(_) => "BULLETIN_MSG".to_string(),

            NotifyEvent::ChainChange(_) => "CHAIN_CHANGE".to_string(),

            NotifyEvent::FetchBulletinMsg => "FETCH_BULLETIN_MSG".to_string(),
            NotifyEvent::MqttConnected => "MQTT_CONNECTED".to_string(),
            NotifyEvent::MqttDisconnected => "MQTT_DISCONNECTED".to_string(),
            NotifyEvent::KeepAlive => "KEEP_ALIVE".to_string(),
            NotifyEvent::ConnectionError(_) => "CONNECTION_ERROR".to_string(),
            NotifyEvent::Debug(_) => "DEBUG".to_string(),
            NotifyEvent::Err(_) => "ERR".to_string(),
            NotifyEvent::TransactionProcess(_) => "TRANSACTION_PROCESS".to_string(),

            // 权限变更事件
            NotifyEvent::PermissionChanger(_) => "PERMISSION_CHANGE".to_string(),
            // 恢复多签数据完成
            NotifyEvent::RecoverComplete => "RECOVER_COMPLETE".to_string(),
            NotifyEvent::MultisigTransCancel => "MULTISIG_TRANS_CANCE".to_string(),
            // 多签交易执行
            NotifyEvent::MultiSignTransExecute => "MULTI_SIGN_TRANS_EXECUTE".to_string(),

            // 其他
            // 同步资产
            NotifyEvent::SyncAssets => "SYNC_ASSETS".to_string(),
            NotifyEvent::ApiWalletSyncAssets(_) => "API_WALLET_SYNC_ASSETS".to_string(),

            // api wallet
            NotifyEvent::AwmCmdActive(_) => "AWM_CMD_ACTIVE".to_string(),
            NotifyEvent::AwmCmdUidUnbind(_) => "AWM_CMD_UID_UNBIND".to_string(),
            NotifyEvent::AwmCmdAddrExpand(_) => "AWM_CMD_ADDR_EXPAND".to_string(),
            NotifyEvent::AwmCmdFeeRes(_) => "AWM_CMD_FEE_RES".to_string(),
            NotifyEvent::AwmOrderTrans(_) => "AWM_ORDER_TRANS".to_string(),
            NotifyEvent::AwmOrderTransRes(_) => "AWM_ORDER_TRANS_RES".to_string(),
            NotifyEvent::AddressUse(_) => "ADDRESS_USE".to_string(),
            NotifyEvent::Withdraw(_) => "WITHDRAW".to_string(),
            NotifyEvent::WithdrawNoPass(_) => "WITHDRAW_NO_PASS".to_string(),
            NotifyEvent::Collect(_) => "COLLECT".to_string(),
            NotifyEvent::CollectFeeNotEnough(_) => "COLLECT_FEE_NOT_ENOUGH".to_string(),
            NotifyEvent::Fee(_) => "FEE".to_string(),
            NotifyEvent::AddressRecovery(_) => "ADDRESS_RECOVERY".to_string(),
            NotifyEvent::AwmCmdDevChange(_) => "AWM_CMD_DEV_CHANGE".to_string(),
            NotifyEvent::ApiWalletAcctChange(_) => "API_WALLET_ACCT_CHANGE".to_string(),
        }
    }
}
