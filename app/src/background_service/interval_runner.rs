use alice_architecture::hosting::IBackgroundService;
use domain::service::{RunJobService, TaskSchedulerService};
use std::time::Duration;
use tokio::time::interval;
use tracing::Instrument;

pub struct IntervalRunner {
    service: std::sync::Arc<dyn RunJobService>,
    scheduler: std::sync::Arc<dyn TaskSchedulerService>,
    interval: Duration,
}

#[async_trait::async_trait]
impl IBackgroundService for IntervalRunner {
    async fn run(&self) {
        let mut interval = interval(self.interval);
        loop {
            let service = self.service.clone();
            let scheduler = self.scheduler.clone();
            tokio::spawn(
                async move {
                    if let Err(e) = service.refresh_all_status().await {
                        tracing::error!("{}", e);
                    }
                    if let Err(e) = scheduler.delete_all_completed_tasks().await {
                        tracing::error!("{}", e);
                    }
                    if let Err(e) = scheduler.schedule_next_task().await {
                        tracing::error!("{}", e);
                    }
                }
                .instrument(tracing::trace_span!("interval_runner")),
            );
            interval.tick().await;
        }
    }
}

impl IntervalRunner {
    pub fn new(
        interval: u64,
        scheduler: std::sync::Arc<dyn TaskSchedulerService>,
        service: std::sync::Arc<dyn RunJobService>,
    ) -> Self {
        Self {
            interval: Duration::from_secs(interval),
            service,
            scheduler,
        }
    }
}
