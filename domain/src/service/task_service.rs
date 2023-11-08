use uuid::Uuid;

use crate::model::entity::task::Task;

#[async_trait::async_trait]
pub trait TaskService {
    type Body;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()>;
    async fn pause(&self, id: Uuid) -> anyhow::Result<()>;
    async fn resume(&self, id: Uuid) -> anyhow::Result<()>;
    async fn cancel(&self, id: Uuid) -> anyhow::Result<()>;
}
