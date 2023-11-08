use uuid::Uuid;

use super::TaskEntity;
use crate::model::entity::{
    job::JobResources,
    task::{ExecuteUsecase, TaskStatus},
};

#[async_trait::async_trait]
pub trait TaskStatusReporter<T: TaskEntity> {
    /// Report status only
    async fn report(&self, id: Uuid, status: TaskStatus) -> anyhow::Result<()>;

    /// Report status with description
    async fn report_msg(&self, id: Uuid, status: TaskStatus, message: &str) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub trait JobResourcesReporter: TaskStatusReporter<ExecuteUsecase> {
    /// Report status with used resources
    async fn report_resources(
        &self,
        id: Uuid,
        status: TaskStatus,
        resources: JobResources,
    ) -> anyhow::Result<()>;
}
