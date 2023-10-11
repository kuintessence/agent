mod counter;
mod grant;

use anyhow::bail;
use reqwest::header::AUTHORIZATION;
use reqwest::Client;
use url::Url;

use self::counter::Counter;
use self::grant::{poll_grant, PollError};
use crate::config::AgentConfig;
use crate::infrastructure::http::authorization::Bearer;
use crate::infrastructure::service::keycloak::LoginInfo;
use crate::infrastructure::service::{
    keycloak::{self, GrantInfo},
    resource_stat::ResourceStat,
};

/// Login,
/// initialize `TOKEN` in `crate::token`,
/// print the fetched agent ID.
///
/// Return error when login fails.
pub async fn go(
    agent_config: &AgentConfig,
    resource_stat: &ResourceStat,
) -> anyhow::Result<GrantInfo> {
    let login_config = &agent_config.login;
    let client = Client::new();

    let data: LoginInfo =
        keycloak::login(&client, &login_config.url, &login_config.client_id).await?;
    println!("{data}");

    let grant_info = {
        let counter = Counter::new(data.expires_in);
        counter.render()?; // render for the first second
        tokio::select! {
            done = counter => {
                done?;
                return Err(PollError::Timeout("verification timeout".to_owned()).into());
            }
            info = poll_grant(
                keycloak::grant_request(
                    &client,
                    &login_config.token_url,
                    &login_config.client_id,
                    &data.device_code,
                )
            ) => {
                info?
            }
        }
    };

    // Register agent itself with resources in computing orchestration system
    let bearer = Bearer::new(&grant_info.access_token);
    let reg_url = agent_config.report_url.parse::<Url>()?.join("/agent/Register")?;
    let status = client
        .post(reg_url)
        .header(AUTHORIZATION, bearer.as_str())
        .json(&resource_stat.total().await?)
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
