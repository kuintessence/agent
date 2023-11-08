mod background_service;
mod config;
mod dto;
mod infrastructure;
mod login;

use std::sync::Arc;
use std::time::Duration;

use alice_infrastructure::config::build_config;
use anyhow::Context;
use colored::Colorize;

use self::background_service::prelude::*;
use self::config::AgentConfig;
use self::infrastructure::http::authorization::JwtPayload;
use self::infrastructure::ioc::Container;
use self::infrastructure::service::keycloak::GrantInfo;

#[tokio::main(worker_threads = 32)]
async fn main() -> anyhow::Result<()> {
    let config = build_config().with_context(|| "Failed to build config".red())?;
    let agent_config: AgentConfig = config.try_deserialize()?;

    // Don't log before login because it will break the login interface
    let GrantInfo {
        access_token,
        refresh_token,
    } = login::go(&agent_config).await.with_context(|| "Login failed".red())?;

    alice_infrastructure::telemetry::init_telemetry(&agent_config.common.telemetry)
        .with_context(|| "Failed to initialize logger".red())?;

    let container = Arc::new(
        Container::new(&agent_config, &access_token, refresh_token)
            .await
            .with_context(|| "Cannot build IOC container".red())?,
    );

    let background_services = async {
        let topic = JwtPayload::from_token(&access_token)?.preferred_username;
        let mq =
            KafkaMessageQueue::new(container.clone(), &agent_config.common.mq, [topic]).await?;

        let resource_reporter =
            ResourceReporter::new(container.clone(), agent_config.server.clone());

        let refresh_jobs_interval = Duration::from_secs(agent_config.refresh_jobs_interval.max(5));

        let background_services = [
            tokio::spawn(async move { mq.run().await }),
            tokio::spawn(async move { resource_reporter.run().await }),
            tokio::spawn(refresh_jobs(container.clone(), refresh_jobs_interval)),
        ];
        tracing::info!("COS Agent Started");

        Result::<_, anyhow::Error>::Ok(background_services)
    }
    .await
    .with_context(|| "Cannot setup background services".red())?;

    tokio::signal::ctrl_c().await.unwrap();
    tracing::info!("Stoping Services (ctrl-c handling).");
    for handle in background_services {
        handle.abort();
    }
    Ok(())
}
