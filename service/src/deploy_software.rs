use std::collections::HashMap;

use anyhow::Context;
use dep_inj::DepInj;
use domain::{
    model::entity::task::{deploy_software::*, Task, TaskStatus},
    service::{SelectSoftwareDeployer, TaskService, TaskStatusReporter},
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(DepInj)]
#[target(DeploySoftwareService)]
pub struct DeploySoftwareState {
    start_guard: Semaphore,
    cancel_map: Mutex<HashMap<Uuid, CancellationToken>>,
}

impl DeploySoftwareState {
    pub fn new(permits: usize) -> Self {
        Self {
            start_guard: Semaphore::new(permits),
            cancel_map: Mutex::default(),
        }
    }
}

#[async_trait::async_trait]
impl<Deps> TaskService for DeploySoftwareService<Deps>
where
    Deps: AsRef<DeploySoftwareState>
        + TaskStatusReporter<DeploySoftware>
        + SelectSoftwareDeployer
        + Send
        + Sync,
{
    type Body = DeploySoftware;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()> {
        let id = task.id;
        self.prj_ref().report(id, TaskStatus::Queued).await?;

        let cancel_token = CancellationToken::new();
        self.cancel_map.lock().await.insert(id, cancel_token.clone());

        tokio::select! {
            res = self.run(task) => {
                self.cancel_map.lock().await.remove(&id);
                if let Err(e) = res {
                    tracing::error!(task_id = %id, "Deploy software: {e}");
                    self.prj_ref().report_msg(id, TaskStatus::Failed, &e.to_string()).await?;

                } else {
                    self.prj_ref().report(id, TaskStatus::Completed).await?;
                }
            },
            _ = cancel_token.cancelled() => (),
        }

        Ok(())
    }

    async fn pause(&self, _id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }

    async fn resume(&self, _id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }

    async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let cancel_token = self.cancel_map.lock().await.remove(&id).context("Task not found")?;
        cancel_token.cancel();

        self.prj_ref().report(id, TaskStatus::Cancelled).await
    }
}

impl<Deps> DeploySoftwareService<Deps>
where
    Deps: AsRef<DeploySoftwareState>
        + TaskStatusReporter<DeploySoftware>
        + SelectSoftwareDeployer
        + Send
        + Sync,
{
    async fn run(&self, task: Task<DeploySoftware>) -> anyhow::Result<()> {
        // controling the numbers of running task via semaphore
        let _running = self.start_guard.acquire().await?;
        self.prj_ref().report(task.id, TaskStatus::Started).await?;

        match task.body.facility_kind {
            FacilityKind::Spack {
                name,
                argument_list,
            } => {
                let deployer = self.prj_ref().select(DeployerType::Spack);

                if let Ok(Some(_)) = deployer.find_installed_hash(&name, &argument_list).await {
                    return Ok(());
                }

                deployer.install(&name, argument_list).await?;
            }
            FacilityKind::Singularity { image, tag } => {
                let deployer = self.prj_ref().select(DeployerType::Apptainer);

                let tag = vec![tag];
                if let Ok(Some(_)) = deployer.find_installed_hash(&image, &tag).await {
                    return Ok(());
                }

                deployer.install(&image, tag).await?;
            }
        };

        Ok(())
    }
}
