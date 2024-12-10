use crate::entities::task_queue::{CreateTaskQueueEntity, TaskQueueEntity};

#[async_trait::async_trait]
pub trait TaskQueueRepoTrait: super::TransactionTrait {
    async fn create_multi_task(
        &mut self,
        req: &[CreateTaskQueueEntity],
    ) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::upsert_multi_task, req)
    }

    async fn create_task(&mut self, req: CreateTaskQueueEntity) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::upsert, req)
    }

    async fn failed_task_queue(&mut self) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::list, 3)
    }

    async fn running_task_queue(&mut self) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::list, 1)
    }

    async fn pending_task_queue(&mut self) -> Result<Vec<TaskQueueEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::list, 0)
    }

    async fn task_detail(&mut self, id: &str) -> Result<Option<TaskQueueEntity>, crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::get_task_queue, id)
    }

    async fn task_running(&mut self, id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::update_status, id, 1)
    }

    async fn task_done(&mut self, id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::update_status, id, 2)
    }

    async fn task_failed(&mut self, id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::update_status, id, 3)
    }

    async fn delete_task(&mut self, id: &str) -> Result<(), crate::Error> {
        let executor = self.get_conn_or_tx()?;
        crate::execute_with_executor!(executor, TaskQueueEntity::delete, id)
    }
}
