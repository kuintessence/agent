use alice_architecture::repository::{IDBRepository, IMutableRepository, IReadOnlyRepository};
use domain::{
    model::entity::{task::TaskStatus, *},
    repository::ITaskRepository,
};
use uuid::Uuid;

use crate::infrastructure::database::JsonDb;

#[async_trait::async_trait]
impl IReadOnlyRepository<Task> for JsonDb {
    async fn get_by_id(&self, uuid: &str) -> anyhow::Result<Task> {
        let id = Uuid::parse_str(uuid)?;
        let tasks = self.tasks.lock().await;
        let sub_tasks = self.sub_tasks.lock().await;
        let task = tasks
            .iter()
            .find(|x| x.id == id)
            .ok_or(anyhow::anyhow!("No such task id."))?
            .clone();
        let body = sub_tasks.iter().cloned().filter(|x| x.parent_id == id).collect();
        Ok(Task { body, ..task })
    }
    async fn get_all(&self) -> anyhow::Result<Vec<Task>> {
        Ok(self.tasks.lock().await.clone())
    }
}

/// 可变仓储，对修改数据的仓储进行抽象
#[async_trait::async_trait]
impl IMutableRepository<Task> for JsonDb {
    /// 更新数据
    async fn update(&self, entity: Task) -> anyhow::Result<Task> {
        let mut tasks = self.tasks.lock().await;
        let index = tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        tasks.remove(index);
        tasks.push(Task {
            update_time: chrono::Utc::now(),
            ..entity.clone()
        });
        Ok(entity)
    }
    /// 插入数据
    async fn insert(&self, entity: Task) -> anyhow::Result<Task> {
        let mut tasks = self.tasks.lock().await;
        if let Some(x) = tasks.iter().position(|x| x.id == entity.id) {
            tasks.remove(x);
        }
        for sub_task in entity.body.iter() {
            self.insert(sub_task.clone()).await?;
        }
        tasks.push(entity.clone());
        Ok(entity)
    }
    /// 删除数据
    async fn delete(&self, entity: Task) -> anyhow::Result<bool> {
        let mut tasks = self.tasks.lock().await;
        let index = tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        tasks.remove(index);
        Ok(true)
    }
    /// 使用 uuid 删除数据，`entity` 是用于指示当前实现类型的泛型模板，防止 Rust 产生方法重载的问题，
    /// 但对于大多数数据库可尝试使用以下代码：
    /// ``` no_run
    /// // 建立一个空的枚举用于指示类型
    /// let n: Option<TYPE> = None;
    /// self.delete_by_id(entity.id.as_str(), n).await?;
    /// ```
    async fn delete_by_id(&self, uuid: &str, _entity: Option<Task>) -> anyhow::Result<bool> {
        let id = Uuid::parse_str(uuid)?;
        let mut tasks = self.tasks.lock().await;
        let index = tasks.iter().position(|x| x.id == id).ok_or(anyhow::anyhow!("No Such id"))?;
        tasks.remove(index);
        Ok(true)
    }
    /// 提交变更，在带有事务的数据库将提交事务，否则该方法应该仅返回 `Ok(true)`
    ///
    async fn save_changed(&self) -> anyhow::Result<bool> {
        self.save_changed().await
    }
}

impl IDBRepository<Task> for JsonDb {}

#[async_trait::async_trait]
impl ITaskRepository for JsonDb {
    async fn get_next_queuing_id(&self) -> anyhow::Result<Option<Uuid>> {
        Ok(self
            .tasks
            .lock()
            .await
            .iter()
            .find(|x| x.status == TaskStatus::Queuing)
            .map(|x| x.id))
    }
}
