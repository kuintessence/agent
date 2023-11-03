use uuid::Uuid;

use super::SubTaskService;

#[async_trait::async_trait]
pub trait RunJobService: SubTaskService {
    async fn run_job(&self, id: Uuid) -> anyhow::Result<()>;
    async fn complete_job(&self, id: Uuid) -> anyhow::Result<()>;
    async fn fail_job(&self, id: Uuid, reason: &str) -> anyhow::Result<()>;
}
