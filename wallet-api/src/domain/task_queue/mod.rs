mod task_handle;
pub(crate) mod task_manager;
use wallet_database::{
    entities::{
        multisig_queue::QueueTaskEntity,
        task_queue::{TaskName, TaskQueueEntity},
    },
    repositories::task_queue::TaskQueueRepoTrait,
};
use wallet_transport_backend::request::TokenQueryPriceReq;

use crate::mqtt::payload::incoming::{
    announcement::BulletinMsg,
    init::Init,
    rpc::RpcChange,
    signature::{
        OrderMultiSignAccept, OrderMultiSignAcceptCompleteMsg, OrderMultiSignCancel,
        OrderMultiSignCreated, OrderMultiSignServiceComplete,
    },
    transaction::{
        AcctChange, MultiSignTransAccept, MultiSignTransAcceptCompleteMsg, MultiSignTransCancel,
    },
};

pub struct TaskDomain<T> {
    phantom: std::marker::PhantomData<T>,
}
impl<T: TaskQueueRepoTrait> Default for TaskDomain<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: TaskQueueRepoTrait> TaskDomain<T> {
    pub fn new() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

pub(crate) enum InitializationTask {
    PullAnnouncement,
    PullHotCoins,
    InitTokenPrice,
    ProcessUnconfirmMsg,
    SetBlockBrowserUrl,
    SetFiat,
    RecoverQueueData,
}

pub(crate) enum BackendApiTask {
    BackendApi(BackendApiTaskData),
}

// 所有请求后端的task，公用结构
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct BackendApiTaskData {
    pub endpoint: String,
    pub body: serde_json::Value,
}

impl BackendApiTaskData {
    pub fn new<T>(endpoint: &str, body: &T) -> Result<Self, crate::ServiceError>
    where
        T: serde::Serialize,
    {
        Ok(Self {
            endpoint: endpoint.to_string(),
            body: wallet_utils::serde_func::serde_to_value(body)?,
        })
    }
}

pub(crate) enum MqttTask {
    OrderMultiSignAccept(OrderMultiSignAccept),
    OrderMultiSignAcceptCompleteMsg(OrderMultiSignAcceptCompleteMsg),
    OrderMultiSignServiceComplete(OrderMultiSignServiceComplete),
    OrderMultiSignCreated(OrderMultiSignCreated),
    OrderMultiSignCancel(OrderMultiSignCancel),
    MultiSignTransAccept(MultiSignTransAccept),
    MultiSignTransCancel(MultiSignTransCancel),
    MultiSignTransAcceptCompleteMsg(MultiSignTransAcceptCompleteMsg),
    AcctChange(AcctChange),
    Init(Init),
    BulletinMsg(BulletinMsg),
    RpcChange(RpcChange),
}

pub(crate) enum CommonTask {
    QueryCoinPrice(TokenQueryPriceReq),
    QueryQueueResult(QueueTaskEntity),
    RecoverMultisigAccountData(String),
}

pub(crate) struct TaskItem {
    pub(crate) id: Option<String>,
    pub(crate) task: Task,
}

impl TaskItem {
    pub fn new(task: Task) -> Self {
        Self { id: None, task }
    }

    pub fn new_with_id(id: &str, task: Task) -> Self {
        Self {
            id: Some(id.to_string()),
            task,
        }
    }
}

pub(crate) struct Tasks(Vec<TaskItem>);

impl Tasks {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(mut self, task: Task) -> Self {
        self.0.push(TaskItem::new(task));
        self
    }

    pub fn push_with_id(mut self, id: &str, task: Task) -> Self {
        self.0.push(TaskItem::new_with_id(id, task));
        self
    }

    pub(crate) async fn send(self) -> Result<(), crate::ServiceError> {
        use wallet_database::repositories::task_queue::TaskQueueRepoTrait as _;

        let task_sender = crate::manager::Context::get_global_task_manager()?;
        let pool = crate::manager::Context::get_global_sqlite_pool()?;
        let mut repo = wallet_database::factory::RepositoryFactory::repo(pool.clone());

        if self.0.is_empty() {
            return Ok(());
        }

        let mut create_entities = Vec::new();
        for task in self.0 {
            let request_body = task.task.get_request_body()?;
            let create_req = if let Some(id) = task.id {
                wallet_database::entities::task_queue::CreateTaskQueueEntity::with_mqtt_request_string(
                    id,
                    task.task.get_name(),
                    request_body,
                )?
            } else {
                wallet_database::entities::task_queue::CreateTaskQueueEntity::with_backend_request_string(
                    task.task.get_name(),
                    request_body,
                )?
            };
            create_entities.push(create_req);
        }

        let entities = repo.create_multi_task(&create_entities).await?;
        task_sender.get_task_sender().send(entities).unwrap();

        Ok(())
    }
}

pub(crate) enum Task {
    Initialization(InitializationTask),
    BackendApi(BackendApiTask),
    Mqtt(Box<MqttTask>),
    Common(CommonTask),
}

impl TryFrom<&TaskQueueEntity> for Task {
    type Error = crate::ServiceError;

    fn try_from(value: &TaskQueueEntity) -> Result<Self, Self::Error> {
        match value.task_name {
            TaskName::BackendApi => {
                let api_data = wallet_utils::serde_func::serde_from_str::<BackendApiTaskData>(
                    &value.request_body,
                )?;
                Ok(Task::BackendApi(BackendApiTask::BackendApi(api_data)))
            }
            TaskName::PullAnnouncement => {
                Ok(Task::Initialization(InitializationTask::PullAnnouncement))
            }
            TaskName::PullHotCoins => Ok(Task::Initialization(InitializationTask::PullHotCoins)),
            TaskName::InitTokenPrice => {
                Ok(Task::Initialization(InitializationTask::InitTokenPrice))
            }
            TaskName::SetBlockBrowserUrl => {
                Ok(Task::Initialization(InitializationTask::SetBlockBrowserUrl))
            }
            TaskName::SetFiat => Ok(Task::Initialization(InitializationTask::SetFiat)),
            TaskName::ProcessUnconfirmMsg => Ok(Task::Initialization(
                InitializationTask::ProcessUnconfirmMsg,
            )),
            TaskName::RecoverQueueData => {
                Ok(Task::Initialization(InitializationTask::RecoverQueueData))
            }
            TaskName::OrderMultiSignAccept => {
                let req = wallet_utils::serde_func::serde_from_str::<OrderMultiSignAccept>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignAccept(req))))
            }
            TaskName::MultiSignTransCancel => {
                let req = wallet_utils::serde_func::serde_from_str::<MultiSignTransCancel>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(MqttTask::MultiSignTransCancel(req))))
            }
            TaskName::OrderMultiSignAcceptCompleteMsg => {
                let req = wallet_utils::serde_func::serde_from_str::<
                    OrderMultiSignAcceptCompleteMsg,
                >(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(
                    MqttTask::OrderMultiSignAcceptCompleteMsg(req),
                )))
            }
            TaskName::OrderMultiSignServiceComplete => {
                let req = wallet_utils::serde_func::serde_from_str::<OrderMultiSignServiceComplete>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(
                    MqttTask::OrderMultiSignServiceComplete(req),
                )))
            }
            TaskName::OrderMultiSignCreated => {
                let req = wallet_utils::serde_func::serde_from_str::<OrderMultiSignCreated>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignCreated(req))))
            }
            TaskName::OrderMultiSignCancel => {
                let req = wallet_utils::serde_func::serde_from_str::<OrderMultiSignCancel>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignCancel(req))))
            }
            TaskName::MultiSignTransAccept => {
                let req = wallet_utils::serde_func::serde_from_str::<MultiSignTransAccept>(
                    &value.request_body,
                )?;
                Ok(Task::Mqtt(Box::new(MqttTask::MultiSignTransAccept(req))))
            }
            TaskName::MultiSignTransAcceptCompleteMsg => {
                let req = wallet_utils::serde_func::serde_from_str::<
                    MultiSignTransAcceptCompleteMsg,
                >(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(
                    MqttTask::MultiSignTransAcceptCompleteMsg(req),
                )))
            }
            TaskName::AcctChange => {
                let req =
                    wallet_utils::serde_func::serde_from_str::<AcctChange>(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(MqttTask::AcctChange(req))))
            }
            TaskName::Init => {
                let req = wallet_utils::serde_func::serde_from_str::<Init>(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(MqttTask::Init(req))))
            }
            TaskName::BulletinMsg => {
                let req =
                    wallet_utils::serde_func::serde_from_str::<BulletinMsg>(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(MqttTask::BulletinMsg(req))))
            }
            TaskName::RpcChange => {
                let req =
                    wallet_utils::serde_func::serde_from_str::<RpcChange>(&value.request_body)?;
                Ok(Task::Mqtt(Box::new(MqttTask::RpcChange(req))))
            }
            // common
            TaskName::QueryCoinPrice => {
                let req = wallet_utils::serde_func::serde_from_str::<TokenQueryPriceReq>(
                    &value.request_body,
                )?;
                Ok(Task::Common(CommonTask::QueryCoinPrice(req)))
            }
            TaskName::QueryQueueResult => {
                let req = wallet_utils::serde_func::serde_from_str::<QueueTaskEntity>(
                    &value.request_body,
                )?;
                Ok(Task::Common(CommonTask::QueryQueueResult(req)))
            }
            TaskName::RecoverMultisigAccountData => Ok(Task::Common(
                CommonTask::RecoverMultisigAccountData(value.request_body.clone()),
            )),
        }
    }
}

impl Task {
    pub fn get_name(&self) -> TaskName {
        match self {
            Task::Initialization(initialization_task) => match initialization_task {
                InitializationTask::PullAnnouncement => TaskName::PullAnnouncement,
                InitializationTask::PullHotCoins => TaskName::PullHotCoins,
                InitializationTask::InitTokenPrice => TaskName::InitTokenPrice,
                InitializationTask::ProcessUnconfirmMsg => TaskName::ProcessUnconfirmMsg,
                InitializationTask::SetBlockBrowserUrl => TaskName::SetBlockBrowserUrl,
                InitializationTask::SetFiat => TaskName::SetFiat,
                InitializationTask::RecoverQueueData => TaskName::RecoverQueueData,
            },
            Task::BackendApi(backend_api_task) => match backend_api_task {
                BackendApiTask::BackendApi(_) => TaskName::BackendApi,
            },
            Task::Mqtt(mqtt_task) => match &**mqtt_task {
                MqttTask::OrderMultiSignAccept(_) => TaskName::OrderMultiSignAccept,
                MqttTask::OrderMultiSignAcceptCompleteMsg(_) => {
                    TaskName::OrderMultiSignAcceptCompleteMsg
                }
                MqttTask::OrderMultiSignServiceComplete(_) => {
                    TaskName::OrderMultiSignServiceComplete
                }
                MqttTask::OrderMultiSignCreated(_) => TaskName::OrderMultiSignCreated,
                MqttTask::OrderMultiSignCancel(_) => TaskName::OrderMultiSignCancel,
                MqttTask::MultiSignTransAccept(_) => TaskName::MultiSignTransAccept,
                MqttTask::MultiSignTransCancel(_) => TaskName::MultiSignTransCancel,
                MqttTask::MultiSignTransAcceptCompleteMsg(_) => {
                    TaskName::MultiSignTransAcceptCompleteMsg
                }
                MqttTask::AcctChange(_) => TaskName::AcctChange,
                MqttTask::Init(_) => TaskName::Init,
                MqttTask::BulletinMsg(_) => TaskName::BulletinMsg,
                MqttTask::RpcChange(_) => TaskName::RpcChange,
            },
            Task::Common(common_task) => match common_task {
                CommonTask::QueryCoinPrice(_) => TaskName::QueryCoinPrice,
                CommonTask::QueryQueueResult(_) => TaskName::QueryQueueResult,
                CommonTask::RecoverMultisigAccountData(_) => TaskName::RecoverMultisigAccountData,
            },
        }
    }

    pub fn get_request_body(&self) -> Result<Option<String>, crate::ServiceError> {
        Ok(match self {
            Task::Initialization(initialization_task) => match initialization_task {
                InitializationTask::PullAnnouncement => None,
                InitializationTask::PullHotCoins => None,
                InitializationTask::InitTokenPrice => None,
                InitializationTask::ProcessUnconfirmMsg => None,
                InitializationTask::SetBlockBrowserUrl => None,
                InitializationTask::SetFiat => None,
                InitializationTask::RecoverQueueData => None,
            },
            Task::BackendApi(backend_api_task) => match backend_api_task {
                BackendApiTask::BackendApi(api_data) => {
                    Some(wallet_utils::serde_func::serde_to_string(api_data)?)
                }
            },
            Task::Mqtt(mqtt_task) => match &**mqtt_task {
                MqttTask::OrderMultiSignAccept(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::OrderMultiSignAcceptCompleteMsg(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::OrderMultiSignServiceComplete(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::OrderMultiSignCancel(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::MultiSignTransAccept(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::MultiSignTransCancel(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::MultiSignTransAcceptCompleteMsg(req) => {
                    Some(wallet_utils::serde_func::serde_to_string(req)?)
                }
                MqttTask::OrderMultiSignCreated(order_multi_sign_created) => Some(
                    wallet_utils::serde_func::serde_to_string(order_multi_sign_created)?,
                ),
                MqttTask::AcctChange(acct_change) => {
                    Some(wallet_utils::serde_func::serde_to_string(acct_change)?)
                }
                MqttTask::Init(init) => Some(wallet_utils::serde_func::serde_to_string(init)?),
                MqttTask::BulletinMsg(bulletin_msg) => {
                    Some(wallet_utils::serde_func::serde_to_string(bulletin_msg)?)
                }
                MqttTask::RpcChange(rpc_change) => {
                    Some(wallet_utils::serde_func::serde_to_string(rpc_change)?)
                }
            },
            Task::Common(common_task) => match common_task {
                CommonTask::QueryCoinPrice(query_coin_price) => {
                    Some(wallet_utils::serde_func::serde_to_string(query_coin_price)?)
                }
                CommonTask::QueryQueueResult(queue) => {
                    Some(wallet_utils::serde_func::serde_to_string(queue)?)
                }
                CommonTask::RecoverMultisigAccountData(recover_multisig_account_data) => {
                    Some(recover_multisig_account_data.to_string())
                }
            },
        })
    }
}

/// 0: initialization, 1: backend_api, 2: mqtt
pub(crate) enum TaskType {
    Initialization,
    BackendApi,
    Mqtt,
    Common,
}

impl sqlx::FromRow<'_, sqlx::sqlite::SqliteRow> for TaskType {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        use sqlx::Row as _;
        let value = row.try_get::<i64, _>("type")?;
        match value {
            0 => Ok(TaskType::Initialization),
            1 => Ok(TaskType::BackendApi),
            2 => Ok(TaskType::Mqtt),
            3 => Ok(TaskType::Common),
            _ => Err(sqlx::Error::RowNotFound),
        }
    }
}

impl sqlx::Encode<'_, sqlx::sqlite::Sqlite> for TaskType {
    fn encode_by_ref(
        &self,
        buf: &mut <sqlx::sqlite::Sqlite as sqlx::database::HasArguments<'_>>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        match self {
            TaskType::Initialization => buf.push(sqlx::sqlite::SqliteArgumentValue::Int64(0)),
            TaskType::BackendApi => buf.push(sqlx::sqlite::SqliteArgumentValue::Int64(1)),
            TaskType::Mqtt => buf.push(sqlx::sqlite::SqliteArgumentValue::Int64(2)),
            TaskType::Common => buf.push(sqlx::sqlite::SqliteArgumentValue::Int64(3)),
        }
        sqlx::encode::IsNull::No
    }
}

impl sqlx::Decode<'_, sqlx::sqlite::Sqlite> for TaskType {
    fn decode(
        value: <sqlx::sqlite::Sqlite as sqlx::database::HasValueRef<'_>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        match value {
            0 => Ok(TaskType::Initialization),
            1 => Ok(TaskType::BackendApi),
            2 => Ok(TaskType::Mqtt),
            3 => Ok(TaskType::Common),
            _ => Err(Box::new(sqlx::Error::ColumnNotFound(
                "Invalid TaskType value".into(),
            ))),
        }
    }
}
