//! A small IOC container for login

use crate::{
    config::AgentConfig,
    infrastructure::{
        command::SshConfig,
        service::resource_stat::{
            Pbs, ResourceStat, ResourceStatImpl, SchedulerStat, SchedulerTotalResources,
            SchedulerUsedResources, Slurm, TotalResources, UsedResources,
        },
    },
};

#[derive(derive_more::AsRef)]
pub struct BootLoader {
    #[as_ref]
    ssh_config: Option<SshConfig>,
    job_scheduler: JobScheduler,
}

enum JobScheduler {
    Pbs,
    Slurm,
}

impl BootLoader {
    pub fn new(config: &AgentConfig) -> anyhow::Result<Self> {
        let ssh_config = config.ssh_proxy.as_ref().map(SshConfig::new);

        let job_scheduler = match config.scheduler.r#type.to_lowercase().as_str() {
            "pbs" => JobScheduler::Pbs,
            "slurm" => JobScheduler::Slurm,
            _ => {
                anyhow::bail!("Unknown `job.scheduler.type`")
            }
        };

        Ok(Self {
            ssh_config,
            job_scheduler,
        })
    }
}

#[async_trait::async_trait]
impl SchedulerStat for BootLoader {
    async fn total(&self) -> anyhow::Result<SchedulerTotalResources> {
        match self.job_scheduler {
            JobScheduler::Pbs => Pbs::inj_ref(self).total().await,
            JobScheduler::Slurm => Slurm::inj_ref(self).total().await,
        }
    }

    async fn used(&self) -> anyhow::Result<SchedulerUsedResources> {
        match self.job_scheduler {
            JobScheduler::Pbs => Pbs::inj_ref(self).used().await,
            JobScheduler::Slurm => Slurm::inj_ref(self).used().await,
        }
    }
}

#[async_trait::async_trait]
impl ResourceStat for BootLoader {
    async fn total(&self) -> anyhow::Result<TotalResources> {
        ResourceStatImpl::inj_ref(self).total().await
    }

    async fn used(&self) -> anyhow::Result<UsedResources> {
        ResourceStatImpl::inj_ref(self).used().await
    }
}
