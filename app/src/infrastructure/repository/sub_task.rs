use alice_architecture::repository::{DBRepository, MutableRepository, ReadOnlyRepository};
use domain::{
    model::entity::{
        file::{FileStatus, FileType},
        task::{TaskStatus, TaskType},
        *,
    },
    repository::ISubTaskRepository,
};
use uuid::Uuid;

use crate::infrastructure::database::JsonDb;

#[async_trait::async_trait]
impl ReadOnlyRepository<SubTask> for JsonDb {
    async fn get_by_id(&self, uuid: Uuid) -> anyhow::Result<SubTask> {
        let sub_tasks = self.sub_tasks.lock().await;
        let sub_task = sub_tasks
            .iter()
            .find(|x| x.id == uuid)
            .ok_or(anyhow::anyhow!("No such task id."))?;
        Ok(sub_task.clone())
    }
    async fn get_all(&self) -> anyhow::Result<Vec<SubTask>> {
        Ok(self.sub_tasks.lock().await.clone())
    }
}

#[async_trait::async_trait]
impl MutableRepository<SubTask> for JsonDb {
    /// 更新数据
    async fn update(&self, entity: &SubTask) -> anyhow::Result<()> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index = sub_tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        sub_tasks.push(entity.clone());
        Ok(())
    }
    /// 插入数据
    async fn insert(&self, entity: &SubTask) -> anyhow::Result<Uuid> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        if let Some(x) = sub_tasks.iter().position(|x| x.id == entity.id) {
            sub_tasks.remove(x);
        }
        if let TaskType::UsecaseExecution { files, .. } = entity.task_type.clone() {
            for file in files {
                self.insert(&File {
                    id: file.id,
                    file_name: file.path.clone(),
                    related_task_body: entity.id,
                    file_type: file.file_type.clone(),
                    status: match file.file_type {
                        FileType::IN => FileStatus::RemoteOnly,
                        FileType::OUT => FileStatus::WaittingCreate,
                    },
                    is_optional: file.optional,
                    is_packaged: file.is_package,
                    is_generated: file.is_generated,
                    text: file.text,
                    metadata_id: file.metadata_id,
                })
                .await?;
            }
        }
        sub_tasks.push(entity.clone());
        Ok(entity.id)
    }

    /// 删除数据
    async fn delete(&self, entity: &SubTask) -> anyhow::Result<()> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index = sub_tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        Ok(())
    }
    async fn delete_by_id(&self, uuid: Uuid) -> anyhow::Result<()> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index = sub_tasks
            .iter()
            .position(|x| x.id == uuid)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        Ok(())
    }
    /// 提交变更，在带有事务的数据库将提交事务，否则该方法应该仅返回 `Ok(true)`
    ///
    async fn save_changed(&self) -> anyhow::Result<bool> {
        self.save_changed().await
    }
}

impl DBRepository<SubTask> for JsonDb {}

#[async_trait::async_trait]
impl ISubTaskRepository for JsonDb {
    async fn get_all_refreshable_task(&self) -> anyhow::Result<Vec<SubTask>> {
        Ok(self
            .sub_tasks
            .lock()
            .await
            .iter()
            .filter(|x| x.job_id != String::default() && x.status == TaskStatus::Running)
            .cloned()
            .collect())
    }
}
