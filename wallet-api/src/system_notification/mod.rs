// /// 1/ 普通账户 2/多签账户建立 3/多签转账
// pub enum SystemNotificationType {
//     /// 普通地址充值/提币
//     ///
//     /// 普通地址充值：
//     /// 收款成功 status 2-成功 transfer_type 1-转出
//     ///
//     /// 普通地址提币：
//     /// 转账成功 status 2-成功 transfer_type 0-转入
//     /// 转账失败 status 3-失败 transfer_type 0-转入
//     CommonTransfer,
//     /// 多签地址收款/提币
//     ///
//     /// 多签地址收款：
//     /// 收款成功 status 2-成功 transfer_type 1-转出
//     /// 收款失败 status 3-失败 transfer_type 1-转出
//     ///
//     /// 多签地址提币：
//     /// 转账成功 status 2-成功 transfer_type 0-转入
//     /// 转账失败 status 3-失败 transfer_type 0-转入
//     MultisigTransfer,

//     /// 多签账户创建等待加入
//     MultisigWaitJoin,

//     /// 多签转账等待签名
//     MultisigWaitSign,

//     /// 多签账户创建成功
//     MultisigCreated,
// }

// impl SystemNotificationType {
//     pub(crate) fn to_i8(self) -> i8 {
//         match self {
//             SystemNotificationType::CommonTransfer => 1,
//             SystemNotificationType::MultisigTransfer => 2,
//             SystemNotificationType::MultisigWaitJoin => 3,
//             SystemNotificationType::MultisigWaitSign => 5,
//             // SystemNotificationType::MultisigCanceled => 6,
//             SystemNotificationType::MultisigCreated => 7,
//         }
//     }
// }

// #[derive(Debug, serde::Serialize)]
// #[serde(untagged)]
// pub enum Content {
//     #[serde(rename_all = "camelCase")]
//     CommonTransfer {
//         tx_hash: String,
//         wallet_name: String,
//         account_name: String,
//         uid: String,
//         // 交易方式 0转入 1转出
//         transfer_type: i8,
//         // 交易状态 1-pending 2-成功 3-失败
//         status: i8,
//     },
//     #[serde(rename_all = "camelCase")]
//     MultisigTransfer {
//         tx_hash: String,
//         multisig_account_id: String,
//         multisig_account_name: String,
//         multisig_account_address: String,
//         // 交易方式 0转入 1转出
//         transfer_type: i8,
//         // 交易状态 1-pending 2-成功 3-失败
//         status: i8,
//     },
//     #[serde(rename_all = "camelCase")]
//     MultisigWaitJoin {
//         multisig_account_id: String,
//         multisig_account_address: String,
//         multisig_account_name: String,
//     },
//     #[serde(rename_all = "camelCase")]
//     MultisigAcceptJoin {
//         multisig_account_id: String,
//         multisig_account_address: String,
//         multisig_account_name: String,
//         accept_address_list: Vec<String>,
//     },
//     #[serde(rename_all = "camelCase")]
//     MultisigWaitSign {
//         queue_id: String,
//         multisig_account_id: String,
//         multisig_account_address: String,
//         multisig_account_name: String,
//     },
//     // #[serde(rename_all = "camelCase")]
//     // MultisigCanceled {
//     //     multisig_account_id: String,
//     //     multisig_account_address: String,
//     //     multisig_account_name: String,
//     // },
//     #[serde(rename_all = "camelCase")]
//     MultisigUpgrade {
//         multisig_account_id: String,
//         multisig_account_address: String,
//         multisig_account_name: String,
//         /// 1/成功 2/失败
//         status: i8,
//     },
// }

// impl Content {
//     pub(crate) fn serialize(self) -> Result<String, crate::ServiceError> {
//         serde_json::to_string(&self).map_err(|e| {
//             crate::ServiceError::Utils(wallet_utils::error::serde::SerdeError::Json(e).into())
//         })
//     }
// }

// 账户类型枚举
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum AccountType {
    Multisig, // 多签账户
    Regular,  // 普通账户
}

// 通知的具体类型
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum NotificationType {
    DeployInvite,
    DeployCompletion,
    TransferConfirmation,
    ReceiveSuccess,
    TransferSuccess,
    TransferFailure,
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationType::DeployInvite => write!(f, "DeployInvite"),
            NotificationType::DeployCompletion => write!(f, "DeployCompletion"),
            NotificationType::TransferConfirmation => write!(f, "TransferConfirmation"),
            NotificationType::ReceiveSuccess => write!(f, "ReceiveSuccess"),
            NotificationType::TransferSuccess => write!(f, "TransferSuccess"),
            NotificationType::TransferFailure => write!(f, "TransferFailure"),
        }
    }
}

// 交易状态枚举
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum TransactionStatus {
    Received, // 已接收
    Sent,     // 已发送
    NotSent,  // 未发送
}

// 多签通知结构体
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultisigNotification {
    account_name: String,
    account_address: String,
    pub(crate) multisig_account_id: String,
    notification_type: NotificationType,
}

// 交易通知结构体
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionNotification {
    account_type: AccountType,
    account_name: String,
    account_address: String,
    pub(crate) transaction_amount: f64,
    pub(crate) currency: String,
    transaction_status: TransactionStatus,
    pub(crate) transaction_hash: String,
    notification_type: NotificationType,
}

// 通知类型枚举
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Notification {
    Multisig(MultisigNotification),
    Transaction(TransactionNotification),
}

// 实现通知的格式化方法
impl Notification {
    pub fn type_name(&self) -> String {
        match self {
            Notification::Multisig(data) => data.notification_type.to_string(),
            Notification::Transaction(data) => data.notification_type.to_string(),
        }
    }

    pub fn serialize(&self) -> Result<String, crate::ServiceError> {
        serde_json::to_string(self).map_err(|e| {
            crate::ServiceError::Utils(wallet_utils::error::serde::SerdeError::Json(e).into())
        })
    }

    // 创建多签通知
    pub fn new_multisig_notification(
        account_name: &str,
        account_address: &str,
        multisig_account_id: &str,
        notification_type: NotificationType,
    ) -> Self {
        Notification::Multisig(MultisigNotification {
            account_name: account_name.to_string(),
            account_address: account_address.to_string(),
            multisig_account_id: multisig_account_id.to_string(),
            notification_type,
        })
    }

    // 创建交易通知
    pub fn new_transaction_notification(
        account_type: AccountType,
        account_name: &str,
        account_address: &str,
        transaction_amount: f64,
        currency: &str,
        transaction_status: &TransactionStatus,
        transaction_hash: &str,
        notification_type: &NotificationType,
    ) -> Self {
        Notification::Transaction(TransactionNotification {
            account_type,
            account_name: account_name.to_string(),
            account_address: account_address.to_string(),
            transaction_amount,
            currency: currency.to_string(),
            transaction_status: transaction_status.clone(),
            transaction_hash: transaction_hash.to_string(),
            notification_type: notification_type.clone(),
        })
    }

    pub fn gen_create_system_notification_entity(
        &self,
        id: &str,
        status: i8,
        key: Option<String>,
        value: Option<String>,
    ) -> Result<
        wallet_database::entities::system_notification::CreateSystemNotificationEntity,
        crate::ServiceError,
    > {
        let content = self.serialize()?;
        let r#type = self.type_name();
        Ok(
            wallet_database::entities::system_notification::CreateSystemNotificationEntity::new(
                id, &r#type, &content, status, key, value,
            ),
        )
    }
}
