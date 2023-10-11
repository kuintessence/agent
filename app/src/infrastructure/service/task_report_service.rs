use std::sync::Arc;

use domain::{repository::ITaskRepository, service::TaskReportService};
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use typed_builder::TypedBuilder;

use crate::dto::{self, TaskResult, TaskUsedResource};
use crate::infrastructure::http::header::TASK_ID;

#[derive(TypedBuilder)]
pub struct TaskReportServiceImpl {
    url: Url,
    base: ClientWithMiddleware,
    repo: Arc<dyn ITaskRepository + Send + Sync>,
}

#[async_trait::async_trait]
impl TaskReportService for TaskReportServiceImpl {
    async fn report_completed_task(&self, id: &str) -> anyhow::Result<()> {
        let task = self.repo.get_by_id(id).await?;
        let used_resources = task
            .body
            .iter()
            .cloned()
            .find(|x| x.resource_used.is_some())
            .unwrap_or_default()
            .resource_used
            .map(|used_resources| TaskUsedResource {
                cpu: used_resources.cpu,
                avg_memory: used_resources.avg_memory,
                max_memory: used_resources.max_memory,
                storage: used_resources.storage,
                wall_time: used_resources.wall_time,
                cpu_time: used_resources.cpu_time,
                node: used_resources.node,
                start_time: used_resources.start_time,
                end_time: used_resources.end_time,
            });

        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Success,
                used_resources,
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_failed_task(&self, id: &str, message: &str) -> anyhow::Result<()> {
        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Failed,
                message: message.to_string(),
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_paused_task(&self, id: &str) -> anyhow::Result<()> {
        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Paused,
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_resumed_task(&self, id: &str) -> anyhow::Result<()> {
        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Continued,
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_deleted_task(&self, id: &str) -> anyhow::Result<()> {
        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Deleted,
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn report_started_task(&self, id: &str) -> anyhow::Result<()> {
        self.base
            .post(self.url.clone())
            .header(TASK_ID, id)
            .json(&TaskResult {
                id: id.to_string(),
                status: dto::TaskResultStatus::Start,
                ..Default::default()
            })
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
