use std::sync::Arc;
use std::time::Duration;

use reqwest_middleware::ClientWithMiddleware;
use tokio::time::sleep;
use url::Url;

use crate::infrastructure::ioc::Container;
use crate::infrastructure::service::resource_stat::ResourceStat;

/// the period for reporting
const REPORT_PERIOD: Duration = Duration::from_secs(60 * 60);

pub struct ResourceReporter {
    stat: Arc<Container>,
    update_url: Url,
    http_client: Arc<ClientWithMiddleware>,
}

impl ResourceReporter {
    pub fn new(container: Arc<Container>, base_url: Url) -> Self {
        Self {
            http_client: container.default_http_client.clone(),
            stat: container,
            update_url: base_url.join("agent/UpdateUsedResource").unwrap(),
        }
    }

    pub async fn run(&self) {
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
