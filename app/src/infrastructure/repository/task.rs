use alice_architecture::repository::{DBRepository, MutableRepository, ReadOnlyRepository};
use domain::{
    model::entity::{task::TaskStatus, *},
    repository::ITaskRepository,
};
use uuid::Uuid;

use crate::infrastructure::database::JsonDb;

#[async_trait::async_trait]
impl ReadOnlyRepository<Task> for JsonDb {
    async fn get_by_id(&self, uuid: Uuid) -> anyhow::Result<Task> {
        let tasks = self.tasks.lock().await;
        let sub_tasks = self.sub_tasks.lock().await;
        let task = tasks
            .iter()
            .find(|x| x.id == uuid)
            .ok_or(anyhow::anyhow!("No such task id."))?
            .clone();
        let body = sub_tasks.iter().cloned().filter(|x| x.parent_id == uuid).collect();
        Ok(Task { body, ..task })
    }
    async fn get_all(&self) -> anyhow::Result<Vec<Task>> {
        Ok(self.tasks.lock().await.clone())
    }
}

/// 可变仓储，对修改数据的仓储进行抽象
#[async_trait::async_trait]
impl MutableRepository<Task> for JsonDb {
    /// 更新数据
    async fn update(&self, entity: &Task) -> anyhow::Result<()> {
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
        Ok(())
    }
    /// 插入数据
    async fn insert(&self, entity: &Task) -> anyhow::Result<Uuid> {
        let mut tasks = self.tasks.lock().await;
        if let Some(x) = tasks.iter().position(|x| x.id == entity.id) {
            tasks.remove(x);
        }
        for sub_task in entity.body.iter() {
            self.insert(sub_task).await?;
        }
        tasks.push(entity.clone());
        Ok(entity.id)
    }
    /// 删除数据
    async fn delete(&self, entity: &Task) -> anyhow::Result<()> {
        let mut tasks = self.tasks.lock().await;
        let index = tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        tasks.remove(index);
        Ok(())
    }
    /// 使用 uuid 删除数据，`entity` 是用于指示当前实现类型的泛型模板，防止 Rust 产生方法重载的问题，
    /// 但对于大多数数据库可尝试使用以下代码：
    /// ``` no_run
    /// // 建立一个空的枚举用于指示类型
    /// let n: Option<TYPE> = None;
    /// self.delete_by_id(entity.id.as_str(), n).await?;
    /// ```
    async fn delete_by_id(&self, uuid: Uuid) -> anyhow::Result<()> {
        let mut tasks = self.tasks.lock().await;
        let index = tasks.iter().position(|x| x.id == uuid).ok_or(anyhow::anyhow!("No Such id"))?;
        tasks.remove(index);
        Ok(())
    }
    /// 提交变更，在带有事务的数据库将提交事务，否则该方法应该仅返回 `Ok(true)`
    ///
    async fn save_changed(&self) -> anyhow::Result<bool> {
        self.save_changed().await
    }
}

impl DBRepository<Task> for JsonDb {}

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
