pub(crate) mod task_type;
use std::collections::{BTreeMap, HashMap};

use super::task_manager::dispatcher::PriorityTask;
use crate::{
    infrastructure::task_queue::{
        CommonTask, MqttTask, RecoverDataBody,
        backend::{BackendApiTask, BackendApiTaskData},
        initialization::InitializationTask,
        mqtt_api::ApiMqttStruct,
        task::task_type::TaskType,
    },
    messaging::mqtt::topics,
};
use std::any::Any;
use wallet_database::{
    entities::{
        multisig_queue::QueueTaskEntity,
        task_queue::{CreateTaskQueueEntity, KnownTaskName, TaskName, TaskQueueEntity},
    },
    repositories::task_queue::TaskQueueRepo,
};
use wallet_transport_backend::request::TokenQueryPriceReq;

#[async_trait::async_trait]
pub(crate) trait TaskTrait: Send + Sync {
    fn get_name(&self) -> TaskName;
    fn get_type(&self) -> TaskType;
    fn get_body(&self) -> Result<Option<String>, crate::error::service::ServiceError>;

    async fn execute(
        &self,
        id: &str,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError>;

    fn as_any(&self) -> &dyn Any;
}

pub(crate) struct TaskItem {
    pub(crate) id: Option<String>,
    pub(crate) task: Box<dyn TaskTrait>,
    pub(crate) remark: Option<String>,
}

impl TaskItem {
    pub fn new<T: TaskTrait + 'static>(task: T) -> Self {
        Self { id: None, task: Box::new(task), remark: None }
    }

    pub fn new_with_id<T: TaskTrait + 'static>(id: &str, task: T) -> Self {
        Self { id: Some(id.to_string()), task: Box::new(task), remark: None }
    }

    pub fn with_remark(mut self, remark: &str) -> Self {
        self.remark = Some(remark.to_string());
        self
    }

    // pub fn new(task: Task) -> Self {
    //     Self { id: None, task }
    // }

    // pub fn new_with_id(id: &str, task: Task) -> Self {
    //     Self {
    //         id: Some(id.to_string()),
    //         task,
    //     }
    // }
}

pub(crate) struct Tasks(Vec<TaskItem>);

#[cfg(any(test, feature = "integration-tests"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TaskExecutionMode {
    Normal = 0,
    Noop = 1,
}

#[cfg(any(test, feature = "integration-tests"))]
static TASK_EXECUTION_MODE: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(TaskExecutionMode::Normal as u8);

#[cfg(any(test, feature = "integration-tests"))]
pub(crate) fn set_task_execution_mode_for_test(mode: TaskExecutionMode) {
    TASK_EXECUTION_MODE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(any(test, feature = "integration-tests"))]
fn is_noop_task_execution_mode() -> bool {
    TASK_EXECUTION_MODE.load(std::sync::atomic::Ordering::Relaxed) == TaskExecutionMode::Noop as u8
}

pub(crate) async fn dispatch_task_entities_with_ctx(
    ctx: &'static crate::context::Context,
    entities: Vec<TaskQueueEntity>,
) -> Result<(), crate::error::service::ServiceError> {
    #[cfg(any(test, feature = "integration-tests"))]
    if is_noop_task_execution_mode() || crate::testkit::ordinary_business::is_noop_context(ctx) {
        return Ok(());
    }

    if entities.is_empty() {
        return Ok(());
    }
    let pool = ctx.task_pool()?;
    // SQL is also the recovery/cleanup authority; never parse owned task names.
    let mut default_entities = Vec::new();
    for row in entities {
        if TaskQueueRepo::externally_owned(&pool, row.remark.as_deref()).await? {
            tracing::debug!(task_id=%row.id,"default consumer leaves externally owned record untouched");
        } else {
            default_entities.push(row);
        }
    }
    if default_entities.is_empty() {
        return Ok(());
    }

    let handles = ctx.get_global_handles().await;
    let Some(handles) = handles.upgrade() else {
        return Err(crate::error::service::ServiceError::System(
            crate::error::system::SystemError::Internal(
                "global handles not ready; cannot dispatch tasks".to_string(),
            ),
        ));
    };

    let task_sender = handles.get_global_task_manager();

    let mut grouped_tasks: BTreeMap<u8, Vec<TaskQueueEntity>> = BTreeMap::new();
    for task_entity in default_entities {
        match TryInto::<Box<dyn TaskTrait>>::try_into(&task_entity) {
            Ok(task) => {
                let priority = super::task_manager::scheduler::assign_priority(&*task, false)?;
                grouped_tasks.entry(priority).or_default().push(task_entity);
            }
            Err(e) => {
                tracing::error!("task_entity.try_into() error: {}", e);
                TaskQueueRepo::delete_task(&pool, &task_entity.id).await?;
            }
        };
    }

    for (priority, tasks) in grouped_tasks {
        if let Err(e) = task_sender.get_task_sender().send(PriorityTask { priority, tasks }) {
            tracing::error!("send task queue error: {}", e);
        }
    }

    TaskQueueRepo::delete_oldest_by_status_when_exceeded(&pool, 200000, 2).await?;
    Ok(())
}

impl Tasks {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push<T: TaskTrait + 'static>(mut self, task: T) -> Self {
        self.0.push(TaskItem::new(task));
        self
    }

    // pub fn push(mut self, task: Task) -> Self {
    //     self.0.push(TaskItem::new(task));
    //     self
    // }

    pub fn push_with_id<T: TaskTrait + 'static>(mut self, id: &str, task: T) -> Self {
        self.0.push(TaskItem::new_with_id(id, task));
        self
    }

    pub fn push_with_remark<T: TaskTrait + 'static>(
        mut self,
        id: &str,
        remark: &str,
        task: T,
    ) -> Self {
        self.0.push(TaskItem::new_with_id(id, task).with_remark(remark));
        self
    }

    async fn create_task_entities(
        &self,
    ) -> Result<Vec<CreateTaskQueueEntity>, crate::error::service::ServiceError> {
        let mut create_entities = Vec::new();
        for task in self.0.iter() {
            // task.task

            let request_body = task.task.get_body()?;
            let remark = task.remark.clone();
            let create_req = if let Some(id) = &task.id {
                TaskQueueRepo::build_mqtt_task(id, task.task.get_name(), request_body, remark)?
            } else {
                TaskQueueRepo::build_backend_task(task.task.get_name(), request_body, remark)?
            };
            create_entities.push(create_req);
        }

        Ok(create_entities)
    }

    async fn dispatch_tasks(
        ctx: &'static crate::context::Context,
        entities: Vec<TaskQueueEntity>,
    ) -> Result<(), crate::error::service::ServiceError> {
        dispatch_task_entities_with_ctx(ctx, entities).await
    }

    pub(crate) async fn send_with_ctx(
        self,
        ctx: &'static crate::context::Context,
    ) -> Result<(), crate::error::service::ServiceError> {
        #[cfg(any(test, feature = "integration-tests"))]
        if is_noop_task_execution_mode() || crate::testkit::ordinary_business::is_noop_context(ctx)
        {
            return Ok(());
        }

        let entities = self.persist_with_ctx(ctx).await?;
        Self::dispatch_tasks(ctx, entities).await?;
        Ok(())
    }

    /// Persist cleanup authority before deleting the facts needed to rebuild it.
    pub(crate) async fn persist_with_ctx(
        self,
        ctx: &'static crate::context::Context,
    ) -> Result<Vec<TaskQueueEntity>, crate::error::service::ServiceError> {
        if self.0.is_empty() {
            return Ok(Vec::new());
        }
        let create_entities = self.create_task_entities().await?;
        Ok(TaskQueueRepo::create_multi_task(&ctx.task_pool()?, &create_entities).await?)
    }

    #[deprecated(note = "pass Context explicitly with send_with_ctx")]
    pub(crate) async fn send(self) -> Result<(), crate::error::service::ServiceError> {
        self.send_with_ctx(crate::get_context()?).await
    }
}

type TaskFactoryFn = fn(&str) -> Result<Box<dyn TaskTrait>, crate::error::service::ServiceError>;

#[macro_export]
macro_rules! register_tasks {
    ($map:ident, $($name:expr => $ty:ty => $constructor:expr),* $(,)?) => {
        $(
            $map.insert($name, |body: &str| {
                let parsed: $ty = wallet_utils::serde_func::serde_from_str(body)?;
                Ok($constructor(parsed))
            });
        )*
    };
}

#[macro_export]
macro_rules! register_tasks_no_parse {
    ($map:ident, $($name:expr => $constructor:expr),* $(,)?) => {
        $(
            $map.insert($name, |_| Ok($constructor));
        )*
    };
}

static TASK_REGISTRY: once_cell::sync::Lazy<
    std::collections::HashMap<KnownTaskName, TaskFactoryFn>,
> = once_cell::sync::Lazy::new(|| {
    let mut map: HashMap<KnownTaskName, TaskFactoryFn> = HashMap::new();

    // Backend + Mqtt + Common：需要解析 request_body 的任务
    register_tasks!(map,
        KnownTaskName::BackendApi => BackendApiTaskData => |parsed| Box::new(BackendApiTask::BackendApi(parsed)),

        KnownTaskName::OrderMultiSignAccept => topics::OrderMultiSignAccept => |parsed| Box::new(MqttTask::OrderMultiSignAccept(parsed)),
        KnownTaskName::MultiSignTransCancel => topics::MultiSignTransCancel => |parsed| Box::new(MqttTask::MultiSignTransCancel(parsed)),
        KnownTaskName::OrderMultiSignAcceptCompleteMsg => topics::OrderMultiSignAcceptCompleteMsg =>|parsed| Box::new(MqttTask::OrderMultiSignAcceptCompleteMsg(parsed)),
        KnownTaskName::OrderMultiSignServiceComplete => topics::OrderMultiSignServiceComplete => |parsed| Box::new(MqttTask::OrderMultiSignServiceComplete(parsed)),
        KnownTaskName::OrderMultiSignCreated => topics::OrderMultiSignCreated => |parsed| Box::new(MqttTask::OrderMultiSignCreated(parsed)),
        KnownTaskName::OrderMultiSignCancel => topics::OrderMultiSignCancel =>|parsed| Box::new(MqttTask::OrderMultiSignCancel(parsed)),
        KnownTaskName::MultiSignTransAccept => topics::MultiSignTransAccept => |parsed| Box::new(MqttTask::MultiSignTransAccept(parsed)),
        KnownTaskName::MultiSignTransAcceptCompleteMsg => topics::MultiSignTransAcceptCompleteMsg =>|parsed| Box::new(MqttTask::MultiSignTransAcceptCompleteMsg(parsed)),
        KnownTaskName::AcctChange => topics::AcctChange => |parsed| Box::new(MqttTask::AcctChange(parsed)),
        KnownTaskName::BulletinMsg => topics::BulletinMsg => |parsed| Box::new(MqttTask::BulletinMsg(parsed)),
        KnownTaskName::PermissionAccept => topics::PermissionAccept => |parsed| Box::new(MqttTask::PermissionAccept(parsed)),
        KnownTaskName::MultiSignTransExecute => topics::MultiSignTransExecute =>|parsed| Box::new(MqttTask::MultiSignTransExecute(parsed)),
        KnownTaskName::CleanPermission => topics::CleanPermission => |parsed| Box::new(MqttTask::CleanPermission(parsed)),
        KnownTaskName::OrderAllConfirmed => topics::OrderAllConfirmed => |parsed| Box::new(MqttTask::OrderAllConfirmed(parsed)),

        KnownTaskName::AwmOrderTrans => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmOrderTransRes => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmCmdAddrExpand => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmCmdFeeRes => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmCmdActive => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmCmdUidUnbind => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AddressUse => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::AwmCmdDevChange => ApiMqttStruct => |parsed| Box::new(MqttTask::ApiMqttStruct(parsed)),
        KnownTaskName::ApiWalletAcctChange => topics::api_wallet::acct_change::ApiWalletAcctChange => |parsed| Box::new(MqttTask::ApiWalletAcctChange(parsed)),
        KnownTaskName::AwmCfgChange => topics::api_wallet::cfg_change::AwmCfgChange => |parsed| Box::new(MqttTask::AwmCfgChange(parsed)),
        KnownTaskName::AwmAddressInsufficient => topics::api_wallet::address_insufficient::AwmAddressInsufficient => |parsed| Box::new(MqttTask::AwmAddressInsufficient(parsed)),

        KnownTaskName::CheckApiAddressBalance => crate::application::api_wallet_balance_check::ApiAddressBalanceCheck => |parsed| Box::new(CommonTask::CheckApiAddressBalance(parsed)),
        KnownTaskName::QueryCoinPrice => TokenQueryPriceReq => |parsed| Box::new(CommonTask::QueryCoinPrice(parsed)),
        KnownTaskName::QueryQueueResult => QueueTaskEntity => |parsed| Box::new(CommonTask::QueryQueueResult(parsed)),
        KnownTaskName::RecoverMultisigAccountData => RecoverDataBody => |parsed| Box::new(CommonTask::RecoverMultisigAccountData(parsed)),
        KnownTaskName::CreateApiAccountDeferred => crate::domain::api_wallet::account::CreateAccountDeferredData => |parsed| Box::new(CommonTask::CreateApiAccountDeferred(parsed)),
        // KnownTaskName::SyncNodesAndLinkToChains => Vec<NodeEntity> => |parsed| Box::new(CommonTask::SyncNodesAndLinkToChains(parsed)),
    );

    // Initialization：不需要解析 request_body 的任务
    register_tasks_no_parse!(map,
        KnownTaskName::PullAnnouncement => Box::new(InitializationTask::PullAnnouncement),
        KnownTaskName::PullApiWalletCoins => Box::new(InitializationTask::PullApiWalletCoins),
        KnownTaskName::PullHotCoins => Box::new(InitializationTask::PullHotCoins),
        KnownTaskName::SetBlockBrowserUrl => Box::new(InitializationTask::SetBlockBrowserUrl),
        KnownTaskName::SetFiat => Box::new(InitializationTask::SetFiat),
        KnownTaskName::RecoverQueueData => Box::new(InitializationTask::RecoverQueueData),
        KnownTaskName::InitMqtt => Box::new(InitializationTask::InitMqtt),
    );

    map
});

impl TryFrom<&TaskQueueEntity> for Box<dyn TaskTrait> {
    type Error = crate::error::service::ServiceError;

    fn try_from(value: &TaskQueueEntity) -> Result<Self, Self::Error> {
        match &value.task_name {
            TaskName::Known(name) => {
                if let Some(builder) = TASK_REGISTRY.get(name) {
                    builder(&value.request_body)
                } else {
                    Err(crate::error::service::ServiceError::System(
                        crate::error::system::SystemError::Service(format!(
                            "Unknown task: {:?}",
                            name
                        )),
                    ))
                }
            }
            _ => Err(crate::error::service::ServiceError::System(
                crate::error::system::SystemError::Service("Unsupported TaskName type".to_string()),
            )),
        }
    }
}

// 将 数据库实体 转换为 Task(根据name匹配对应的类型)
// impl TryFrom<&TaskQueueEntity> for Task {
//     type Error = crate::ServiceError;

//     fn try_from(value: &TaskQueueEntity) -> Result<Self, Self::Error> {
//         match value.task_name {
//             TaskName::Known(KnownTaskName::BackendApi) => {
//                 let api_data =
//                     serde_func::serde_from_str::<BackendApiTaskData>(&value.request_body)?;
//                 Ok(Task::BackendApi(BackendApiTask::BackendApi(api_data)))
//             }
//             TaskName::Known(KnownTaskName::PullAnnouncement) => {
//                 Ok(Task::Initialization(InitializationTask::PullAnnouncement))
//             }
//             TaskName::Known(KnownTaskName::PullHotCoins) => {
//                 Ok(Task::Initialization(InitializationTask::PullHotCoins))
//             }
//             TaskName::Known(KnownTaskName::InitTokenPrice) => {
//                 Ok(Task::Initialization(InitializationTask::InitTokenPrice))
//             }
//             TaskName::Known(KnownTaskName::SetBlockBrowserUrl) => {
//                 Ok(Task::Initialization(InitializationTask::SetBlockBrowserUrl))
//             }
//             TaskName::Known(KnownTaskName::SetFiat) => {
//                 Ok(Task::Initialization(InitializationTask::SetFiat))
//             }
//             // TaskName::ProcessUnconfirmMsg => Ok(Task::Initialization(
//             //     InitializationTask::ProcessUnconfirmMsg,
//             // )),
//             TaskName::Known(KnownTaskName::RecoverQueueData) => {
//                 Ok(Task::Initialization(InitializationTask::RecoverQueueData))
//             }
//             TaskName::Known(KnownTaskName::InitMqtt) => {
//                 Ok(Task::Initialization(InitializationTask::InitMqtt))
//             }
//             TaskName::Known(KnownTaskName::OrderMultiSignAccept) => {
//                 let req = serde_func::serde_from_str::<topics::OrderMultiSignAccept>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignAccept(req))))
//             }
//             TaskName::Known(KnownTaskName::MultiSignTransCancel) => {
//                 let req = serde_func::serde_from_str::<topics::MultiSignTransCancel>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::MultiSignTransCancel(req))))
//             }
//             TaskName::Known(KnownTaskName::OrderMultiSignAcceptCompleteMsg) => {
//                 let req = serde_func::serde_from_str::<topics::OrderMultiSignAcceptCompleteMsg>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(
//                     MqttTask::OrderMultiSignAcceptCompleteMsg(req),
//                 )))
//             }
//             TaskName::Known(KnownTaskName::OrderMultiSignServiceComplete) => {
//                 let req = serde_func::serde_from_str::<topics::OrderMultiSignServiceComplete>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(
//                     MqttTask::OrderMultiSignServiceComplete(req),
//                 )))
//             }
//             TaskName::Known(KnownTaskName::OrderMultiSignCreated) => {
//                 let req = serde_func::serde_from_str::<topics::OrderMultiSignCreated>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignCreated(req))))
//             }
//             TaskName::Known(KnownTaskName::OrderMultiSignCancel) => {
//                 let req = serde_func::serde_from_str::<topics::OrderMultiSignCancel>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::OrderMultiSignCancel(req))))
//             }
//             TaskName::Known(KnownTaskName::MultiSignTransAccept) => {
//                 let req = serde_func::serde_from_str::<topics::MultiSignTransAccept>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::MultiSignTransAccept(req))))
//             }
//             TaskName::Known(KnownTaskName::MultiSignTransAcceptCompleteMsg) => {
//                 let req = serde_func::serde_from_str::<topics::MultiSignTransAcceptCompleteMsg>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(
//                     MqttTask::MultiSignTransAcceptCompleteMsg(req),
//                 )))
//             }
//             TaskName::Known(KnownTaskName::AcctChange) => {
//                 let req = serde_func::serde_from_str::<topics::AcctChange>(&value.request_body)?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::AcctChange(req))))
//             }
//             TaskName::Known(KnownTaskName::BulletinMsg) => {
//                 let req = serde_func::serde_from_str::<topics::BulletinMsg>(&value.request_body)?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::BulletinMsg(req))))
//             }
//             TaskName::Known(KnownTaskName::QueryCoinPrice) => {
//                 let req = serde_func::serde_from_str::<TokenQueryPriceReq>(&value.request_body)?;
//                 Ok(Task::Common(CommonTask::QueryCoinPrice(req)))
//             }
//             TaskName::Known(KnownTaskName::QueryQueueResult) => {
//                 let req = serde_func::serde_from_str::<QueueTaskEntity>(&value.request_body)?;
//                 Ok(Task::Common(CommonTask::QueryQueueResult(req)))
//             }
//             TaskName::Known(KnownTaskName::RecoverMultisigAccountData) => {
//                 let req = serde_func::serde_from_str::<RecoverDataBody>(&value.request_body)?;
//                 Ok(Task::Common(CommonTask::RecoverMultisigAccountData(req)))
//             }
//             TaskName::Known(KnownTaskName::SyncNodesAndLinkToChains) => {
//                 let req = serde_func::serde_from_str::<Vec<NodeEntity>>(&value.request_body)?;
//                 Ok(Task::Common(CommonTask::SyncNodesAndLinkToChains(req)))
//             }
//             TaskName::Known(KnownTaskName::PermissionAccept) => {
//                 let req =
//                     serde_func::serde_from_str::<topics::PermissionAccept>(&value.request_body)?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::PermissionAccept(req))))
//             }
//             TaskName::Known(KnownTaskName::MultiSignTransExecute) => {
//                 let req = serde_func::serde_from_str::<topics::MultiSignTransExecute>(
//                     &value.request_body,
//                 )?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::MultiSignTransExecute(req))))
//             }
//             TaskName::Known(KnownTaskName::CleanPermission) => {
//                 let req =
//                     serde_func::serde_from_str::<topics::CleanPermission>(&value.request_body)?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::CleanPermission(req))))
//             }
//             TaskName::Known(KnownTaskName::OrderAllConfirmed) => {
//                 let req =
//                     serde_func::serde_from_str::<topics::OrderAllConfirmed>(&value.request_body)?;
//                 Ok(Task::Mqtt(Box::new(MqttTask::OrderAllConfirmed(req))))
//             }
//             _ => Err(crate::SystemError::Service("Unknown task name".to_string()).into()),
//         }
//     }
// }

#[cfg(test)]
mod cfg_change_restore_tests {
    use super::*;

    #[test]
    fn cfg_change_task_restores_with_mqtt_confirmation_type() {
        let body = r#"{"cfgType":1,"appId":"app","orgId":"org","sn":"device","wuid":"wallet"}"#;
        let task = TASK_REGISTRY.get(&KnownTaskName::AwmCfgChange).unwrap()(body).unwrap();
        assert_eq!(task.get_name(), TaskName::Known(KnownTaskName::AwmCfgChange));
        assert_eq!(task.get_type(), TaskType::Mqtt);
        let restored: serde_json::Value =
            serde_json::from_str(&task.get_body().unwrap().unwrap()).unwrap();
        assert_eq!(restored["appId"], "app");
        assert_eq!(restored["wuid"], "wallet");
    }
}
