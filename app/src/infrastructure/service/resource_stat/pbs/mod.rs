mod pbsnodes;
mod qstat;

use anyhow::{bail, Context};
use dep_inj_target::dep_inj_target;

use self::pbsnodes::{NodeAssigned, NodeAvailable};
use super::{SchedulerStat, SchedulerTotalResources, SchedulerUsedResources};
use crate::infrastructure::command::MaybeSsh;

#[dep_inj_target]
pub struct Pbs;

#[async_trait::async_trait]
impl<Deps> SchedulerStat for Pbs<Deps>
where
    Deps: MaybeSsh + Send + Sync,
{
    async fn total(&self) -> anyhow::Result<SchedulerTotalResources> {
        let output = self
            .prj_ref()
            .command("pbsnodes")
            .args(pbsnodes::Status::<NodeAvailable>::ARGS)
            .output()
            .await
            .context("pbsnodes")?;
        if !output.status.success() {
            bail!(
                "pbsnodes terminated with an exception. Exit status: {}",
                output.status
            );
        }

        let status = pbsnodes::Status::<NodeAvailable>::new(&output.stdout)?;
        let resources = status.available();
        Ok(SchedulerTotalResources {
            memory: resources.mem.0,
            core_number: resources.ncpus,
            node_number: status.node_count(),
        })
    }

    async fn used(&self) -> anyhow::Result<SchedulerUsedResources> {
        let output = self
            .prj_ref()
            .command("pbsnodes")
            .args(pbsnodes::Status::<NodeAssigned>::ARGS)
            .output()
            .await
            .context("pbsnodes")?;
        if !output.status.success() {
            bail!(
                "pbsnodes terminated with an exception. Exit status: {}",
                output.status
            );
        }
        let nodes_status = pbsnodes::Status::<NodeAssigned>::new(&output.stdout)?;

        let output = self
            .prj_ref()
            .command("qstat")
            .args(qstat::Status::ARGS)
            .output()
            .await
            .context("qstat")?;
        if !output.status.success() {
            bail!(
                "qstat terminated with an exception. Exit status: {}",
                output.status
            );
        }
        let jobs_status = qstat::Status::new(&output.stdout)?;

        let (resources, used_node_count) = nodes_status.assigned();
        let (queuing_task_count, running_task_count) = jobs_status.qr_count();
        Ok(SchedulerUsedResources {
            allocated_memory: resources.mem.0,
            allocated_cpu_count: resources.ncpus,
            queuing_task_count,
            running_task_count,
            used_node_count,
        })
    }
}
