use std::sync::Arc;
use std::time::Duration;

use alice_architecture::IBackgroundService;
use reqwest_middleware::ClientWithMiddleware;
use tokio::time::sleep;
use typed_builder::TypedBuilder;
use url::Url;

use crate::infrastructure::service::resource_stat::ResourceStat;

/// the period for reporting
const REPORT_PERIOD: Duration = Duration::from_secs(60 * 60);

#[derive(TypedBuilder)]
pub struct ResourceReporter {
    /// config.agent.report_url + "/agent/UpdateUsedResource"
    update_url: Url,
    http_client: Arc<ClientWithMiddleware>,
    stat: Arc<ResourceStat>,
}

#[async_trait::async_trait]
impl IBackgroundService for ResourceReporter {
    async fn run(&self) {
        loop {
            if let Err(e) = self.update().await {
                tracing::error!(
                    "Failed to update resources on computing orchestration system: {e}"
                );
            }
            sleep(REPORT_PERIOD).await;
        }
    }
}

impl ResourceReporter {
    async fn update(&self) -> anyhow::Result<()> {
        let resources = self.stat.used().await?;
        tracing::info!("Reporting resources: {resources:#?}");
        self.http_client.post(self.update_url.clone()).json(&resources).send().await?;
        Ok(())
    }
}
