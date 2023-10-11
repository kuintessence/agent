mod background_service;
mod config;
mod dto;
mod infrastructure;
mod login;

use std::sync::Arc;
use std::time::Duration;

use alice_architecture::IBackgroundService;
use alice_di::IServiceProvider;
use alice_infrastructure::config::build_config;
use alice_infrastructure::config::CommonConfig;
use anyhow::Context;
use colored::Colorize;
use url::Url;

use self::config::AgentConfig;
use self::infrastructure::{
    command::SshProxy,
    http::{authorization::JwtPayload, middleware::AuthMiddleware},
    service::{keycloak::GrantInfo, resource_stat::ResourceStat},
    service_provider::ServiceProvider,
};

#[tokio::main(worker_threads = 32)]
async fn main() -> anyhow::Result<()> {
    let config = build_config().with_context(|| "Failed to build config".red())?;
    let agent_config: AgentConfig = config.get("agent")?;
    let ssh_proxy = Arc::new(SshProxy::new(&agent_config.ssh_proxy));

    let resource_stat = ResourceStat::new(&agent_config.scheduler.r#type, ssh_proxy.clone())
        .with_context(|| format!("Unsupported scheduler: {}", &agent_config.scheduler.r#type))?;

    // Don't log before login because it will break the login interface
    let GrantInfo {
        access_token,
        refresh_token,
    } = login::go(&agent_config, &resource_stat)
        .await
        .with_context(|| "Login failed".red())?;

    let common_config: CommonConfig = config.get("common").unwrap_or_default();
    alice_infrastructure::telemetry::initialize_telemetry(common_config.telemetry())
        .with_context(|| "Failed to initialize logger".red())?;

    let sp = async {
        let token_url: Url = agent_config.login.token_url.parse()?;
        let auth_middleware = AuthMiddleware::new(
            token_url.clone(),
            &agent_config.login.client_id,
            &access_token,
            refresh_token,
            Duration::from_secs(1),
        );
        let topic = JwtPayload::from_token(&access_token)?.preferred_username;
        Result::<_, anyhow::Error>::Ok(Arc::new(
            ServiceProvider::build(
                config,
                ssh_proxy,
                Arc::new(auth_middleware),
                topic,
                Arc::new(resource_stat),
            )
            .await?,
        ))
    }
    .await
    .with_context(|| "Cannot build Service Provider".red())?;

    let tasks: Vec<Arc<dyn IBackgroundService + Send + Sync>> = sp.provide();
    let handles: Vec<_> = tasks
        .into_iter()
        .map(|task| tokio::spawn(async move { task.run().await }))
        .collect();
    tracing::info!("COS Agent Started.");
    tokio::signal::ctrl_c().await.unwrap();
    tracing::info!("Stoping Services (ctrl-c handling).");
    for handle in handles {
        handle.abort();
    }

    Ok(())
}
