use std::ops::{Deref, DerefMut};
use std::path::Path;

use tokio::process::Command;

use super::ssh_proxy::SshConfig;

pub trait Scp {
    fn scp(&self) -> Option<(ScpCommand, &SshConfig)>;
}

impl<Ctx> Scp for Ctx
where
    Ctx: AsRef<Option<SshConfig>>,
{
    fn scp(&self) -> Option<(ScpCommand, &SshConfig)> {
        self.as_ref().as_ref().map(|ssh| {
            let mut base = Command::new("scp");
            base.args(["-P", &ssh.port]);
            (ScpCommand { ssh, base }, ssh)
        })
    }
}

pub struct ScpCommand<'a> {
    ssh: &'a SshConfig,
    base: Command,
}

impl Deref for ScpCommand<'_> {
    type Target = Command;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for ScpCommand<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl ScpCommand<'_> {
    /// Copy directory
    #[inline]
    pub fn rec(&mut self) -> &mut Self {
        self.base.arg("-r");
        self
    }

    #[inline]
    pub fn local_path(&mut self, p: impl AsRef<Path>) -> &mut Self {
        self.base.arg(p.as_ref());
        self
    }

    pub fn remote_path(&mut self, p: impl AsRef<Path>) -> &mut Self {
        self.base.arg(format!(
            "{}:{}",
            self.ssh.username_host,
            p.as_ref().to_string_lossy()
        ));
        self
    }
}
