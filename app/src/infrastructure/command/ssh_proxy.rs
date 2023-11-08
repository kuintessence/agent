use tokio::process::Command;

use crate::config::SshProxyConfig;

#[derive(Debug, Clone)]
pub struct SshConfig {
    pub port: String,
    pub username_host: String,
    pub home_dir: String,
    pub save_dir: String,
}

/// An ssh proxy for command. It's transparent if not using ssh.
pub trait MaybeSsh {
    fn command(&self, cmd: &str) -> Command;
    fn is_ssh(&self) -> bool;
}

impl<Ctx> MaybeSsh for Ctx
where
    Ctx: AsRef<Option<SshConfig>>,
{
    fn command(&self, cmd: &str) -> Command {
        let Some(ssh) = self.as_ref() else {
            return Command::new(cmd);
        };

        let mut command = Command::new("ssh");
        command.args(["-p", &ssh.port, &ssh.username_host, cmd]);
        command
    }

    fn is_ssh(&self) -> bool {
        self.as_ref().is_some()
    }
}

impl SshConfig {
    pub fn new(config: &SshProxyConfig) -> Self {
        let SshProxyConfig {
            host,
            username,
            port,
            home_dir,
            save_dir,
        } = config;

        Self {
            port: port.to_string(),
            username_host: format!("{username}@{host}"),
            home_dir: home_dir.clone(),
            save_dir: save_dir.clone(),
        }
    }
}
