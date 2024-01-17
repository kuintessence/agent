mod pbs;
mod slurm;
mod storage;

use dep_inj_target::dep_inj_target;
use serde::Serialize;

use self::storage::stat;
use crate::infrastructure::command::MaybeSsh;

pub use self::{pbs::Pbs, slurm::Slurm};

#[async_trait::async_trait]
pub trait ResourceStat {
    async fn total(&self) -> anyhow::Result<TotalResources>;
    async fn used(&self) -> anyhow::Result<UsedResources>;
}

#[async_trait::async_trait]
pub trait SchedulerStat {
    async fn total(&self) -> anyhow::Result<SchedulerTotalResources>;
    async fn used(&self) -> anyhow::Result<SchedulerUsedResources>;
}

/// Total resources counted by scheduler
#[derive(Debug, Default)]
pub struct SchedulerTotalResources {
    memory: u64,
    core_number: usize,
    node_number: usize,
}

/// Used resources counted by scheduler
#[derive(Debug, Default)]
pub struct SchedulerUsedResources {
    allocated_memory: u64,
    allocated_cpu_count: usize,
    queuing_task_count: usize,
    running_task_count: usize,
    used_node_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalResources {
    memory: u64,
    core_number: usize,
    storage_capacity: u64,
    node_number: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsedResources {
    allocated_memory: u64,
    allocated_cpu_count: usize,
    used_storage: u64,
    queuing_task_count: usize,
    running_task_count: usize,
    used_node_count: usize,
}

#[dep_inj_target]
pub struct ResourceStatImpl;

#[async_trait::async_trait]
impl<Deps> ResourceStat for ResourceStatImpl<Deps>
where
    Deps: MaybeSsh + SchedulerStat + Send + Sync,
{
    async fn total(&self) -> anyhow::Result<TotalResources> {
        let SchedulerTotalResources {
            memory,
            core_number,
            node_number,
        } = self.prj_ref().total().await?;
        let storage_capacity = self.total_storage().await?;

        Ok(TotalResources {
            memory,
            core_number,
            storage_capacity,
            node_number,
        })
    }

    async fn used(&self) -> anyhow::Result<UsedResources> {
        let SchedulerUsedResources {
            allocated_memory,
            allocated_cpu_count,
            queuing_task_count,
            running_task_count,
            used_node_count,
        } = self.prj_ref().used().await?;
        let used_storage = self.used_storage().await?;

        Ok(UsedResources {
            allocated_memory,
            allocated_cpu_count,
            used_storage,
            queuing_task_count,
            running_task_count,
            used_node_count,
        })
    }
}

impl<Deps> ResourceStatImpl<Deps>
where
    Deps: MaybeSsh + Send + Sync,
{
    async fn total_storage(&self) -> anyhow::Result<u64> {
        if self.prj_ref().is_ssh() {
            stat::total(self.prj_ref()).await
        } else {
            Ok(storage::statvfs::total()?)
        }
    }

    async fn used_storage(&self) -> anyhow::Result<u64> {
        if self.prj_ref().is_ssh() {
            stat::used(self.prj_ref()).await
        } else {
            Ok(storage::statvfs::used()?)
        }
    }
}
