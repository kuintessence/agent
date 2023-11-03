use alice_architecture::background_service::BackgroundService;
use domain::{sender::ISubTaskReportService, service::TaskSchedulerService};
use std::sync::Arc;
use tracing::Instrument;
use uuid::Uuid;

pub struct TaskSchedulerRunner {
    receiver: flume::Receiver<SubTaskReport>,
    scheduler_task: Arc<dyn TaskSchedulerService>,
}

#[async_trait::async_trait]
impl BackgroundService for TaskSchedulerRunner {
    async fn run(&self) {
        loop {
            let scheduler_task = self.scheduler_task.clone();
            match self.receiver.recv_async().await {
                Ok(report) => {
                    tokio::spawn(
                        async move {
                            let report = report.clone();
                            let scheduler_task = scheduler_task.clone();
                            match report.status {
                                SubTaskStatus::Completed => {
                                    match scheduler_task.complete_sub_task(report.id).await {
                                        Ok(()) => tracing::debug!(
                                            "Sub-task {} is reported to complete.",
                                            report.id
                                        ),
                                        Err(e) => tracing::error!("{}", e),
                                    }
                                }
                                SubTaskStatus::Failed => {
                                    match scheduler_task.fail_sub_task(report.id).await {
                                        Ok(()) => tracing::debug!(
                                            "Sub-task {} is reported to fail.",
                                            report.id
                                        ),
                                        Err(e) => tracing::error!("{}", e),
                                    }
                                }
                            }
                        }
                        .instrument(tracing::trace_span!("task_scheduler_runner")),
                    );
                }
                Err(e) => tracing::error!("{}", e),
            }
        }
    }
}

impl TaskSchedulerRunner {
    pub fn new(
        receiver: flume::Receiver<SubTaskReport>,
        scheduler_task: Arc<dyn TaskSchedulerService>,
    ) -> Self {
        Self {
            receiver,
            scheduler_task,
        }
    }
}

#[derive(Clone)]
pub struct SubTaskReport {
    pub id: Uuid,
    pub status: SubTaskStatus,
}

#[derive(Clone)]
pub enum SubTaskStatus {
    Completed,
    Failed,
}

pub struct SubTaskReportService {
    receiver: flume::Receiver<SubTaskReport>,
    sender: Arc<flume::Sender<SubTaskReport>>,
}

#[async_trait::async_trait]
impl ISubTaskReportService for SubTaskReportService {
    async fn report_completed_task(&self, id: Uuid) -> anyhow::Result<()> {
        Ok(self
            .sender
            .send_async(SubTaskReport {
                id,
                status: SubTaskStatus::Completed,
            })
            .await?)
    }
    async fn report_failed_task(&self, id: Uuid) -> anyhow::Result<()> {
        Ok(self
            .sender
            .send_async(SubTaskReport {
                id,
                status: SubTaskStatus::Failed,
            })
            .await?)
    }
}

impl SubTaskReportService {
    pub fn new() -> Self {
        let (sender, receiver): (flume::Sender<SubTaskReport>, flume::Receiver<SubTaskReport>) =
            flume::unbounded();
        Self {
            sender: Arc::from(sender),
            receiver,
        }
    }

    pub fn get_receiver(&self) -> flume::Receiver<SubTaskReport> {
        self.receiver.clone()
    }
}
