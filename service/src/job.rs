use anyhow::Context;
use dashmap::DashMap;
use dep_inj::DepInj;
use domain::{
    model::{
        entity::{
            job::{Job, JobState},
            task::{
                deploy_software::{DeployerType, FacilityKind},
                execute_usecase::ExecuteUsecase,
                Task, TaskStatus,
            },
        },
        vo::job::ScriptInfo,
    },
    service::{
        JobResourcesReporter, JobScheduler, JobService, SelectSoftwareDeployer, TaskService,
    },
};
use uuid::Uuid;

#[derive(DepInj, Default)]
#[target(JobServiceImpl)]
pub struct JobServiceState {
    repo: DashMap<Uuid, Job>,
    spack: bool,
    apptainer: bool,
}

impl JobServiceState {
    pub fn new(spack: bool, apptainer: bool) -> Self {
        Self {
            spack,
            apptainer,
            ..Default::default()
        }
    }
}

#[async_trait::async_trait]
impl<Deps> TaskService for JobServiceImpl<Deps>
where
    Deps: AsRef<JobServiceState>
        + JobResourcesReporter
        + SelectSoftwareDeployer
        + JobScheduler
        + Send
        + Sync,
{
    type Body = ExecuteUsecase;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()> {
        let ExecuteUsecase {
            name,
            facility_kind,
            arguments,
            environments,
            std_in,
            requirements,
        } = task.body;

        let mut load_software: String = "".to_string();

        match facility_kind {
            FacilityKind::Spack {
                name,
                argument_list,
            } => {
                if self.spack {
                    let deployer = self.prj_ref().select(DeployerType::Spack);
                    if let Some(hash) = deployer.find_installed_hash(&name, &argument_list).await? {
                        load_software = deployer.gen_load_script(&hash);
                    }
                }
            }
            FacilityKind::Singularity { image, tag } => {
                if self.apptainer {
                    let deployer = self.prj_ref().select(DeployerType::Apptainer);
                    if let Some(hash) = deployer.find_installed_hash(&image, &[tag]).await? {
                        load_software = deployer.gen_load_script(&hash);
                    }
                }
            }
        }

        if (self.spack || self.apptainer) && load_software.is_empty() {
            tracing::error!(task_id = %task.id, "Execute usecase: software not found");
            return self
                .prj_ref()
                .report_msg(task.id, TaskStatus::Failed, "Software not found")
                .await;
        }

        let info = ScriptInfo {
            id: task.id.to_string(),
            parent_id: task.node_id.to_string(),
            name,
            load_software,
            path: format!("{}/run.sh", task.node_id),
            arguments,
            environments,
            std_in,
            requirements,
        };
        let job_id = self.prj_ref().submit_job_script(info).await?;
        let job = self.prj_ref().get_job(&job_id).await?;

        let status = match job.state {
            JobState::Queuing => {
                self.repo.insert(task.id, job);
                TaskStatus::Queued
            }
            JobState::Running | JobState::Completing => {
                self.repo.insert(task.id, job);
                TaskStatus::Started
            }
            JobState::Failed | JobState::Unknown => {
                return self.report_failure(task.id, &job).await
            }
            /* The following arms shouldn't be met at the beginning */
            JobState::Suspended => {
                self.repo.insert(task.id, job);
                TaskStatus::Paused
            }
            JobState::Completed => TaskStatus::Completed,
        };

        self.prj_ref().report(task.id, status).await
    }

    async fn pause(&self, id: Uuid) -> anyhow::Result<()> {
        let job_id = self.repo.get(&id).map(|job| job.id.clone()).context("Job not found")?;
        self.prj_ref().pause_job(&job_id).await?;
        self.refresh(id).await
    }

    async fn resume(&self, id: Uuid) -> anyhow::Result<()> {
        let job_id = self.repo.get(&id).map(|job| job.id.clone()).context("Job not found")?;
        self.prj_ref().continue_job(&job_id).await?;
        self.refresh(id).await
    }

    async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let job = self.repo.remove(&id).context("Job not found")?.1;
        self.prj_ref().delete_job(&job.id).await?;
        self.prj_ref().report(id, TaskStatus::Cancelled).await
    }
}

#[async_trait::async_trait]
impl<Deps> JobService for JobServiceImpl<Deps>
where
    Deps: AsRef<JobServiceState> + JobResourcesReporter + JobScheduler + Send + Sync,
{
    async fn refresh_all(&self) {
        tracing::info!("Refreshing state of jobs");
        let ids: Vec<Uuid> = self.repo.iter().map(|entry| *entry.key()).collect();
        for id in ids {
            if let Err(e) = self.refresh(id).await {
                tracing::error!(task_id = %id, "Execute usecase: {e}");
            }
        }
    }
}

impl<Deps> JobServiceImpl<Deps> {
    async fn refresh(&self, id: Uuid) -> anyhow::Result<()>
    where
        Deps: AsRef<JobServiceState> + JobResourcesReporter + JobScheduler + Send + Sync,
    {
        let (job_id, pre_state) = self
            .repo
            .get(&id)
            .map(|job| (job.id.clone(), job.state))
            .context("Job not found")?;
        let job = self.prj_ref().get_job(&job_id).await?;

        match job.state {
            JobState::Queuing => (),
            JobState::Running | JobState::Completing => match pre_state {
                JobState::Queuing => {
                    self.repo.insert(id, job);
                    self.prj_ref().report(id, TaskStatus::Started).await?;
                }
                JobState::Suspended => {
                    self.repo.insert(id, job);
                    self.prj_ref().report(id, TaskStatus::Resumed).await?;
                }
                _ => (),
            },
            JobState::Suspended => {
                if pre_state != JobState::Suspended {
                    self.repo.insert(id, job);
                    self.prj_ref().report(id, TaskStatus::Paused).await?;
                }
            }
            JobState::Failed | JobState::Unknown => {
                self.repo.remove(&id);
                tracing::info!(job_id = %job.id, "Job failed");
                self.report_failure(id, &job).await?;
            }
            JobState::Completed => {
                self.repo.remove(&id);
                tracing::info!(job_id = %job.id, "Job completed");
                self.prj_ref()
                    .report_resources(id, TaskStatus::Completed, job.resource_used)
                    .await?;
            }
        }

        Ok(())
    }

    async fn report_failure(&self, id: Uuid, job: &Job) -> anyhow::Result<()>
    where
        Deps: JobResourcesReporter + Send + Sync,
    {
        self.prj_ref()
            .report_msg(
                id,
                TaskStatus::Failed,
                &format!(
                    "Job exit with {}\nError Output:\n{}",
                    job.exit_status_code, job.error_output
                ),
            )
            .await
    }
}
