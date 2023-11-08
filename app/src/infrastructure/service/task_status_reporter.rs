use dep_inj::DepInj;
use domain::{
    model::entity::{
        job::JobResources,
        task::{ExecuteUsecase, TaskStatus},
    },
    service::{JobResourcesReporter, TaskEntity, TaskStatusReporter},
};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;
use uuid::Uuid;

use crate::{
    dto::{TaskResult, TaskResultWithResource},
    infrastructure::http::header::TASK_ID,
};

#[derive(DepInj)]
#[target(TaskStatusReporterImpl)]
pub struct TaskStatusReporterState {
    url: Url,
    client: ClientWithMiddleware,
}

impl TaskStatusReporterState {
    pub fn new(base_url: Url, client: ClientWithMiddleware) -> Self {
        Self {
            url: base_url.join("workflow-engine/ReceiveTaskStatus").unwrap(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl<T, Deps> TaskStatusReporter<T> for TaskStatusReporterImpl<Deps>
where
    T: TaskEntity,
    Deps: AsRef<TaskStatusReporterState> + Send + Sync,
{
    async fn report(&self, id: Uuid, status: TaskStatus) -> anyhow::Result<()> {
        tracing::info!(ID=%id, "*{}* task {status}", T::TYPE);
        self.client
            .post(self.url.clone())
            .header(TASK_ID, id.to_string())
            .json(&TaskResult {
                id,
                status,
                message: None,
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_msg(&self, id: Uuid, status: TaskStatus, message: &str) -> anyhow::Result<()> {
        tracing::info!(ID=%id, "*{}* task {status}: {message}", T::TYPE);
        self.client
            .post(self.url.clone())
            .header(TASK_ID, id.to_string())
            .json(&TaskResult {
                id,
                status,
                message: Some(message),
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl<Deps> JobResourcesReporter for TaskStatusReporterImpl<Deps>
where
    Deps: AsRef<TaskStatusReporterState> + Send + Sync,
{
    async fn report_resources(
        &self,
        id: Uuid,
        status: TaskStatus,
        resources: JobResources,
    ) -> anyhow::Result<()> {
        tracing::info!(ID=%id, ?resources, "*{}* task {status}", ExecuteUsecase::TYPE);
        self.client
            .post(self.url.clone())
            .header(TASK_ID, id.to_string())
            .json(&TaskResultWithResource {
                id,
                status,
                used_resources: resources,
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
