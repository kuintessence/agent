mod sinfo;
mod squeue;

use anyhow::bail;
use anyhow::Context;
use dep_inj_target::dep_inj_target;

use self::sinfo::{NodeAlloc, NodeTotal};
use super::SchedulerStat;
use super::{SchedulerTotalResources, SchedulerUsedResources};
use crate::infrastructure::command::MaybeSsh;

#[dep_inj_target]
pub struct Slurm;

#[async_trait::async_trait]
impl<Deps> SchedulerStat for Slurm<Deps>
where
    Deps: MaybeSsh + Send + Sync,
{
    async fn total(&self) -> anyhow::Result<SchedulerTotalResources> {
        let output = self
            .prj_ref()
            .command("sinfo")
            .args(sinfo::Info::<NodeTotal>::ARGS)
            .output()
            .await
            .context("sinfo")?;
        if !output.status.success() {
            bail!(
                "sinfo total terminated with an exception. Exit status: {}, stderr:\n {}",
                output.status,
                String::from_utf8(output.stderr)?
            );
        }

        let info = sinfo::Info::<NodeTotal>::new(&output.stdout)?;
        let resources = info.total();
        Ok(SchedulerTotalResources {
            memory: resources.memory,
            core_number: resources.cpus,
            node_number: info.node_count(),
        })
    }

    async fn used(&self) -> anyhow::Result<SchedulerUsedResources> {
        let output = self
            .prj_ref()
            .command("sinfo")
            .args(sinfo::Info::<NodeAlloc>::ARGS)
            .output()
            .await
            .context("sinfo")?;
        if !output.status.success() {
            bail!(
                "sinfo alloc terminated with an exception. Exit status: {}, stderr:\n {}",
                output.status,
                String::from_utf8(output.stderr)?
            );
        }
        let info = sinfo::Info::<NodeAlloc>::new(&output.stdout)?;

        let output = self
            .prj_ref()
            .command("squeue")
            .args(squeue::Status::ARGS)
            .output()
            .await
            .context("squeue")?;

        if !output.status.success() {
            bail!(
                "squeue terminated with an exception. Exit status: {}, stderr:\n {}",
                output.status,
                String::from_utf8(output.stderr)?
            );
        }
        let jobs_status = squeue::Status::new(&output.stdout);

        let resources = info.alloc();
        let (queuing_task_count, running_task_count) = jobs_status.qr_count();
        Ok(SchedulerUsedResources {
            allocated_memory: resources.alloc_memory,
            allocated_cpu_count: resources.alloc_cpus,
            queuing_task_count,
            running_task_count,
            used_node_count: resources.alloc_nodes,
        })
    }
}
