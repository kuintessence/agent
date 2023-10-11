use alice_architecture::repository::{IDBRepository, IMutableRepository, IReadOnlyRepository};
use domain::{
    model::entity::{file::FileStatus, *},
    repository::IFileRepository,
};
use uuid::Uuid;

use crate::infrastructure::database::JsonDb;

#[async_trait::async_trait]
impl IReadOnlyRepository<File> for JsonDb {
    async fn get_by_id(&self, uuid: &str) -> anyhow::Result<File> {
        let id = Uuid::parse_str(uuid)?;
        let task_files = self.task_files.lock().await;
        let task_file = task_files
            .iter()
            .find(|x| x.id == id)
            .ok_or(anyhow::anyhow!("No Such job id."))?;
        Ok(task_file.clone())
    }
    async fn get_all(&self) -> anyhow::Result<Vec<File>> {
        Ok(self.task_files.lock().await.clone())
    }
}

/// 可变仓储，对修改数据的仓储进行抽象
#[async_trait::async_trait]
impl IMutableRepository<File> for JsonDb {
    /// 更新数据
    async fn update(&self, entity: File) -> anyhow::Result<File> {
        let mut task_files = self.task_files.lock().await;
        let index = task_files
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        task_files.remove(index);
        task_files.push(entity.clone());
        Ok(entity)
    }
    /// 插入数据
    async fn insert(&self, entity: File) -> anyhow::Result<File> {
        let mut task_files = self.task_files.lock().await;
        if let Some(x) = task_files.iter().position(|x| x.id == entity.id) {
            task_files.remove(x);
        }
        task_files.push(entity.clone());
        Ok(entity)
    }
    /// 删除数据
    async fn delete(&self, entity: File) -> anyhow::Result<bool> {
        let mut task_files = self.task_files.lock().await;
        let index = task_files
            .iter()
            .position(|x| x.id == entity.id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        task_files.remove(index);
        Ok(true)
    }
    /// 使用 uuid 删除数据，`entity` 是用于指示当前实现类型的泛型模板，防止 Rust 产生方法重载的问题，
    /// 但对于大多数数据库可尝试使用以下代码：
    /// ``` no_run
    /// // 建立一个空的枚举用于指示类型
    /// let n: Option<TYPE> = None;
    /// self.delete_by_id(entity.id.as_str(), n).await?;
    /// ```
    async fn delete_by_id(&self, uuid: &str, _entity: Option<File>) -> anyhow::Result<bool> {
        let id = Uuid::parse_str(uuid)?;
        let mut task_files = self.task_files.lock().await;
        let index = task_files
            .iter()
            .position(|x| x.id == id)
            .ok_or(anyhow::anyhow!("No Such id"))?;
        task_files.remove(index);
        Ok(true)
    }
    /// 提交变更，在带有事务的数据库将提交事务，否则该方法应该仅返回 `Ok(true)`
    ///
    async fn save_changed(&self) -> anyhow::Result<bool> {
        self.save_changed().await
    }
}

impl IDBRepository<File> for JsonDb {}

#[async_trait::async_trait]
impl IFileRepository for JsonDb {
    async fn find_files_by_task(&self, id: &str) -> anyhow::Result<Vec<File>> {
        let id = Uuid::parse_str(id)?;
        let task_files = self.task_files.lock().await;
        let task_file = task_files.iter().filter(|&x| x.related_task_body == id);
        Ok(task_file.cloned().collect())
    }
    async fn update_task_file_status(&self, id: &str, status: FileStatus) -> anyhow::Result<File> {
        let mut file: File = self.get_by_id(id).await?;
        file.status = status;
        self.update(file.clone()).await?;
        Ok(file)
    }
}
