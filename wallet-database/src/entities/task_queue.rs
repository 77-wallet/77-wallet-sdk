#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TaskQueueEntity {
    pub id: String,
    pub task_name: TaskName,
    pub request_body: String,
    pub r#type: u8,
    /// 0: pending, 1: running, 2: success, 3: failed
    pub status: u8,
    #[serde(skip_serializing)]
    pub created_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,
    #[serde(skip_serializing)]
    pub updated_at: Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct CreateTaskQueueEntity {
    pub id: String,
    pub task_name: TaskName,
    pub request_body: Option<String>,
    pub r#type: u8,
    pub status: u8,
}

impl CreateTaskQueueEntity {
    pub fn new(
        id: Option<String>,
        task_name: TaskName,
        request_body: Option<String>,
        r#type: u8,
        status: u8,
    ) -> Result<Self, crate::Error> {
        let id = id.unwrap_or_else(|| wallet_utils::snowflake::get_uid().unwrap().to_string());
        Ok(Self {
            id,
            task_name,
            request_body,
            r#type,
            status,
        })
    }

    pub fn with_backend_request_string(
        task_name: TaskName,
        request_body: Option<String>,
    ) -> Result<Self, crate::Error> {
        Self::new(None, task_name, request_body, 1, 0)
    }

    pub fn with_mqtt_request_string(
        id: String,
        task_name: TaskName,
        request_body: Option<String>,
    ) -> Result<Self, crate::Error> {
        Self::new(Some(id), task_name, request_body, 1, 0)
    }

    pub fn with_backend_request<T: serde::Serialize>(
        task_name: TaskName,
        request_body: Option<&T>,
    ) -> Result<Self, crate::Error> {
        let request_body = request_body
            .map(wallet_utils::serde_func::serde_to_string)
            .transpose()?;
        Self::new(None, task_name, request_body, 1, 0)
    }
}

#[derive(Debug, serde::Serialize, Clone, Copy, sqlx::Type)]
#[sqlx(rename_all = "PascalCase")]
pub enum TaskName {
    PullAnnouncement,
    PullHotCoins,
    InitTokenPrice,
    ProcessUnconfirmMsg,
    SetBlockBrowserUrl,
    SetFiat,
    RecoverQueueData,
    BackendApi,
    // mqtt
    OrderMultiSignAccept,
    OrderMultiSignAcceptCompleteMsg,
    OrderMultiSignServiceComplete,
    OrderMultiSignCancel,
    MultiSignTransAccept,
    MultiSignTransCancel,
    MultiSignTransAcceptCompleteMsg,
    AcctChange,
    Init,
    OrderMultiSignCreated,
    BulletinMsg,
    RpcChange,
    // common
    QueryCoinPrice,
    QueryQueueResult,
    RecoverMultisigAccountData,
}
