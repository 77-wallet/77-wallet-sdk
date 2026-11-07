use permission_change::PermissionChange;
use wallet_database::entities::{asset_token_key::AssetTokenKey, bill::BillKind};
pub mod permission_change;

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
    Confirmation,
    ReceiveSuccess,
    TransferSuccess,
    TransferFailure,
    ResourceChange,
    PermissionChange,
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationType::DeployInvite => write!(f, "DeployInvite"),
            NotificationType::DeployCompletion => write!(f, "DeployCompletion"),
            NotificationType::Confirmation => write!(f, "Confirmation"),
            NotificationType::ReceiveSuccess => write!(f, "ReceiveSuccess"),
            NotificationType::TransferSuccess => write!(f, "TransferSuccess"),
            NotificationType::TransferFailure => write!(f, "TransferFailure"),
            NotificationType::ResourceChange => write!(f, "ResourceChange"),
            NotificationType::PermissionChange => write!(f, "PermissionChange"),
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

// 确认通知结构体
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigConfirmationNotification {
    account_name: String,
    account_address: String,
    pub(crate) multisig_account_id: String,
    bill_kind: BillKind,
    notification_type: NotificationType,
}

// 交易通知结构体
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionNotification {
    account_type: AccountType,
    account_name: String,
    account_address: String,
    #[serde(default)]
    pub(crate) from_addr: String,
    #[serde(default)]
    pub(crate) to_addr: String,
    pub(crate) transaction_amount: f64,
    pub(crate) symbol: String,
    #[serde(default)]
    pub(crate) chain_code: String,
    transaction_status: TransactionStatus,
    pub(crate) transaction_hash: String,
    pub token_address: AssetTokenKey,
    notification_type: NotificationType,
}

// 交易通知结构体
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNotification {
    account_address: String,
    account_name: String,
    pub(crate) multisig_account_id: String,
    bill_kind: BillKind,
    status: bool,
    pub(crate) transaction_hash: String,
    notification_type: NotificationType,
}

// 通知类型枚举
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Notification {
    Multisig(MultisigNotification),
    Transaction(TransactionNotification),
    Confirmation(SigConfirmationNotification),
    Resource(ResourceNotification),
    PermissionChange(PermissionChange),
}

// 实现通知的格式化方法
impl Notification {
    pub fn type_name(&self) -> String {
        match self {
            Notification::Multisig(data) => data.notification_type.to_string(),
            Notification::Transaction(data) => data.notification_type.to_string(),
            Notification::Resource(data) => data.notification_type.to_string(),
            Notification::Confirmation(data) => data.notification_type.to_string(),
            Notification::PermissionChange(data) => data.notification_type.to_string(),
        }
    }

    pub fn serialize(&self) -> Result<String, crate::error::service::ServiceError> {
        serde_json::to_string(self).map_err(|e| {
            crate::error::service::ServiceError::Utils(
                wallet_utils::error::serde::SerdeError::Json(e).into(),
            )
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

    // 创建确认通知
    pub fn new_confirmation_notification(
        account_name: &str,
        account_address: &str,
        multisig_account_id: &str,
        bill_kind: BillKind,
        notification_type: NotificationType,
    ) -> Self {
        Notification::Confirmation(SigConfirmationNotification {
            account_name: account_name.to_string(),
            account_address: account_address.to_string(),
            multisig_account_id: multisig_account_id.to_string(),
            bill_kind,
            notification_type,
        })
    }

    // 创建交易通知
    pub fn new_transaction_notification(
        account_type: AccountType,
        account_name: &str,
        account_address: &str,
        from_addr: &str,
        to_addr: &str,
        transaction_amount: f64,
        currency: &str,
        chain_code: &str,
        transaction_status: &TransactionStatus,
        transaction_hash: &str,
        token_address: impl Into<AssetTokenKey>,
        notification_type: &NotificationType,
    ) -> Self {
        Notification::Transaction(TransactionNotification {
            account_type,
            account_name: account_name.to_string(),
            account_address: account_address.to_string(),
            from_addr: from_addr.to_string(),
            to_addr: to_addr.to_string(),
            transaction_amount,
            symbol: currency.to_string(),
            token_address: token_address.into(),
            chain_code: chain_code.to_string(),
            transaction_status: transaction_status.clone(),
            transaction_hash: transaction_hash.to_string(),
            notification_type: notification_type.clone(),
        })
    }

    pub(crate) fn _new_resource_notification(
        account_address: &str,
        account_name: &str,
        multisig_account_id: &str,
        bill_kind: BillKind,
        status: bool,
        transaction_hash: &str,
        notification_type: &NotificationType,
    ) -> Self {
        Notification::Resource(ResourceNotification {
            account_address: account_address.to_string(),
            account_name: account_name.to_string(),
            bill_kind,
            status,
            notification_type: notification_type.clone(),
            transaction_hash: transaction_hash.to_string(),
            multisig_account_id: multisig_account_id.to_string(),
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
        crate::error::service::ServiceError,
    > {
        let content = self.serialize()?;
        let r#type = self.type_name();
        Ok(wallet_database::repositories::system_notification::SystemNotificationRepo::build_create(
            id, &r#type, &content, status, key, value,
        ))
    }
}
