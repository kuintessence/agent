use uuid::Uuid;

#[async_trait::async_trait]
pub trait TaskReportService: Send + Sync {
    async fn report_completed_task(&self, id: Uuid) -> anyhow::Result<()>;
    async fn report_failed_task(&self, id: Uuid, message: &str) -> anyhow::Result<()>;
    async fn report_paused_task(&self, id: Uuid) -> anyhow::Result<()>;
    async fn report_resumed_task(&self, id: Uuid) -> anyhow::Result<()>;
    async fn report_deleted_task(&self, id: Uuid) -> anyhow::Result<()>;
    async fn report_started_task(&self, id: Uuid) -> anyhow::Result<()>;
}
