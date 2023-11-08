mod counter;
mod grant;
mod ioc;

use anyhow::bail;
use reqwest::header::AUTHORIZATION;
use reqwest::Client;

use self::grant::{poll_grant, PollError};
use self::ioc::BootLoader;
use crate::config::AgentConfig;
use crate::infrastructure::http::authorization::Bearer;
use crate::infrastructure::service::keycloak::LoginInfo;
use crate::infrastructure::service::keycloak::{self, GrantInfo};
use crate::infrastructure::service::resource_stat::ResourceStat;
use crate::login::counter::{counter, recover_cursor};

/// Login,
/// initialize `TOKEN` in `crate::token`,
/// print the fetched agent ID.
///
/// Return error when login fails.
pub async fn go(agent_config: &AgentConfig) -> anyhow::Result<GrantInfo> {
    let client_id = &agent_config.client_id;
    let resouce_stat = BootLoader::new(agent_config)?;
    let client = Client::new();

    let data: LoginInfo = keycloak::login(
        &client,
        agent_config.oidc_server.join("auth/device").unwrap(),
        client_id,
    )
    .await?;
    println!("{data}");

    let grant_info = {
        // let counter = Counter::new(data.expires_in);
        // counter.render()?; // render for the first second
        tokio::select! {
            done = counter(data.expires_in) => {
                done?;
                return Err(PollError::Timeout("verification timeout".to_owned()).into());
            }
            info = poll_grant(
                keycloak::grant_request(
                    &client,
                    agent_config.oidc_server.join("token").unwrap(),
                    client_id,
                    &data.device_code,
                )
            ) => {
                info?
            }
        }
    };
    recover_cursor()?;

    // Register agent itself with resources in computing orchestration system
    let bearer = Bearer::new(&grant_info.access_token);
    let reg_url = agent_config.server.join("agent/Register").unwrap();
    let status = client
        .post(reg_url)
        .header(AUTHORIZATION, bearer.as_str())
        .json(&resouce_stat.total().await?)
        .send()
        .await?
        .status();
    if !status.is_success() {
        bail!("failed to register in computing orchestration system: response status={status}");
    }

    let agent_id = bearer.payload()?.sub;
    println!("Your agent ID: {agent_id}");

    Ok(grant_info)
}

impl std::fmt::Display for LoginInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "User code: {}", self.user_code)?;
        write!(
            f,
            "Please verify your identity at: {}",
            self.verification_uri
        )?;

        Ok(())
    }
}
