
use alice_architecture::repository::DBRepository;
use uuid::Uuid;

use crate::model::entity::{file::FileStatus, File, SubTask, Task};

#[async_trait::async_trait]
pub trait IFileRepository: DBRepository<File> {
    async fn find_files_by_task(&self, id: Uuid) -> anyhow::Result<Vec<File>>;
    async fn update_task_file_status(&self, id: Uuid, status: FileStatus) -> anyhow::Result<File>;
}

#[async_trait::async_trait]
pub trait ISubTaskRepository: DBRepository<SubTask> {
    async fn get_all_refreshable_task(&self) -> anyhow::Result<Vec<SubTask>>;
}

#[async_trait::async_trait]
pub trait ITaskRepository: DBRepository<Task> {
    async fn get_next_queuing_id(&self) -> anyhow::Result<Option<uuid::Uuid>>;
}
