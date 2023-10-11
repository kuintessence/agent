use alice_architecture::repository::{IDBRepository, IMutableRepository, IReadOnlyRepository};
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
impl IReadOnlyRepository<SubTask> for JsonDb {
    async fn get_by_id(&self, uuid: &str) -> anyhow::Result<SubTask> {
        let id = Uuid::parse_str(uuid)?;
        let sub_tasks = self.sub_tasks.lock().await;
        let sub_task = sub_tasks
            .iter()
            .find(|x| x.id == id)
            .ok_or(anyhow::anyhow!("No such task id."))?;
        Ok(sub_task.clone())
    }
    async fn get_all(&self) -> anyhow::Result<Vec<SubTask>> {
        Ok(self.sub_tasks.lock().await.clone())
    }
}

#[async_trait::async_trait]
impl IMutableRepository<SubTask> for JsonDb {
    /// 更新数据
    async fn update(&self, entity: SubTask) -> anyhow::Result<SubTask> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index = sub_tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        sub_tasks.push(entity.clone());
        Ok(entity)
    }
    /// 插入数据
    async fn insert(&self, entity: SubTask) -> anyhow::Result<SubTask> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        if let Some(x) = sub_tasks.iter().position(|x| x.id == entity.id) {
            sub_tasks.remove(x);
        }
        if let TaskType::UsecaseExecution { files, .. } = entity.task_type.clone() {
            for file in files {
                self.insert(File {
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
        Ok(entity)
    }

    /// 删除数据
    async fn delete(&self, entity: SubTask) -> anyhow::Result<bool> {
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index = sub_tasks
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        Ok(true)
    }
    /// 使用 uuid 删除数据，`entity` 是用于指示当前实现类型的泛型模板，防止 Rust 产生方法重载的问题，
    /// 但对于大多数数据库可尝试使用以下代码：
    /// ``` no_run
    /// // 建立一个空的枚举用于指示类型
    /// let n: Option<TYPE> = None;
    /// self.delete_by_id(entity.id.as_str(), n).await?;
    /// ```
    async fn delete_by_id(&self, uuid: &str, _entity: Option<SubTask>) -> anyhow::Result<bool> {
        let id = Uuid::parse_str(uuid)?;
        let mut sub_tasks = self.sub_tasks.lock().await;
        let index =
            sub_tasks.iter().position(|x| x.id == id).ok_or(anyhow::anyhow!("No Such id"))?;
        sub_tasks.remove(index);
        Ok(true)
    }
    /// 提交变更，在带有事务的数据库将提交事务，否则该方法应该仅返回 `Ok(true)`
    ///
    async fn save_changed(&self) -> anyhow::Result<bool> {
        self.save_changed().await
    }
}

impl IDBRepository<SubTask> for JsonDb {}

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
