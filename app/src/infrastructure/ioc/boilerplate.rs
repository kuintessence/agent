use domain::{
    model::{
        entity::{
            job::JobResources,
            task::{collect_output::*, deploy_software::DeployerType, TaskStatus},
            Job, Task,
        },
        vo::job::ScriptInfo,
    },
    service::{
        FileLoadService, JobResourcesReporter, JobScheduler, JobService, SelectSoftwareDeployer,
        SoftwareDeployer, TaskEntity, TaskService, TaskStatusReporter,
    },
};
use service::{
    collect_output::CollectOutputService, deploy_software::DeploySoftwareService,
    job::JobServiceImpl,
};
use uuid::Uuid;

use super::container::JobSchedulerState;
use super::Container;
use crate::{
    dto::{StartTaskBody, TaskType},
    infrastructure::service::{
        download_file::DownloadFileService,
        file_load::FileLoadServiceImpl,
        job_scheduler::{LsfClient, PbsClient, SlurmClient},
        resource_stat::{
            Pbs, ResourceStat, ResourceStatImpl, SchedulerStat, SchedulerTotalResources,
            SchedulerUsedResources, Slurm, TotalResources, UsedResources,
        },
        software_deployer::{ApptainerDeployer, SpackDeployer},
        task_status_reporter::TaskStatusReporterImpl,
        upload_file::UploadFileService,
        SelectTaskService,
    },
};

#[async_trait::async_trait]
impl FileLoadService for Container {
    async fn load_file(&self, node_id: Uuid, from: &CollectFrom) -> anyhow::Result<String> {
        FileLoadServiceImpl::inj_ref(self).load_file(node_id, from).await
    }

    async fn save_file(
        &self,
        task_id: Uuid,
        node_id: Uuid,
        to: &CollectTo,
        output: &str,
    ) -> anyhow::Result<()> {
        FileLoadServiceImpl::inj_ref(self).save_file(task_id, node_id, to, output).await
    }
}

#[async_trait::async_trait]
impl<T: TaskEntity> TaskStatusReporter<T> for Container {
    async fn report(&self, id: Uuid, status: TaskStatus) -> anyhow::Result<()> {
        TaskStatusReporter::<T>::report(TaskStatusReporterImpl::inj_ref(self), id, status).await
    }

    async fn report_msg(&self, id: Uuid, status: TaskStatus, message: &str) -> anyhow::Result<()> {
        TaskStatusReporter::<T>::report_msg(
            TaskStatusReporterImpl::inj_ref(self),
            id,
            status,
            message,
        )
        .await
    }
}

#[async_trait::async_trait]
impl JobResourcesReporter for Container {
    async fn report_resources(
        &self,
        id: Uuid,
        status: TaskStatus,
        resources: JobResources,
    ) -> anyhow::Result<()> {
        TaskStatusReporterImpl::inj_ref(self)
            .report_resources(id, status, resources)
            .await
    }
}

impl SelectSoftwareDeployer for Container {
    fn select(&self, r#type: DeployerType) -> &(dyn SoftwareDeployer + Send + Sync) {
        match r#type {
            DeployerType::Spack => SpackDeployer::inj_ref(self),
            DeployerType::Apptainer => ApptainerDeployer::inj_ref(self),
        }
    }
}

#[async_trait::async_trait]
impl JobScheduler for Container {
    async fn get_jobs(&self) -> anyhow::Result<Vec<Job>> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).get_jobs().await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).get_jobs().await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).get_jobs().await,
        }
    }

    async fn get_job(&self, id: &str) -> anyhow::Result<Job> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).get_job(id).await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).get_job(id).await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).get_job(id).await,
        }
    }

    async fn submit_job_script(&self, script_info: ScriptInfo) -> anyhow::Result<String> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => {
                PbsClient::inj_ref(self).submit_job_script(script_info).await
            }
            JobSchedulerState::Slurm(_) => {
                SlurmClient::inj_ref(self).submit_job_script(script_info).await
            }
            JobSchedulerState::Lsf(_) => {
                LsfClient::inj_ref(self).submit_job_script(script_info).await
            }
        }
    }

    async fn submit_job(&self, script_path: &str) -> anyhow::Result<String> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).submit_job(script_path).await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).submit_job(script_path).await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).submit_job(script_path).await,
        }
    }

    async fn delete_job(&self, job_id: &str) -> anyhow::Result<()> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).delete_job(job_id).await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).delete_job(job_id).await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).delete_job(job_id).await,
        }
    }

    async fn pause_job(&self, job_id: &str) -> anyhow::Result<()> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).pause_job(job_id).await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).pause_job(job_id).await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).pause_job(job_id).await,
        }
    }

    async fn continue_job(&self, job_id: &str) -> anyhow::Result<()> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => PbsClient::inj_ref(self).continue_job(job_id).await,
            JobSchedulerState::Slurm(_) => SlurmClient::inj_ref(self).continue_job(job_id).await,
            JobSchedulerState::Lsf(_) => LsfClient::inj_ref(self).continue_job(job_id).await,
        }
    }
}

#[async_trait::async_trait]
impl JobService for Container {
    async fn refresh_all(&self) {
        JobServiceImpl::inj_ref(self).refresh_all().await;
    }
}

#[async_trait::async_trait]
impl SelectTaskService for Container {
    async fn start(&self, id: Uuid, node_id: Uuid, body: StartTaskBody) {
        let task_type = body.r#type().to_str();
        if let Err(e) = match body {
            StartTaskBody::DeploySoftware(body) => {
                DeploySoftwareService::inj_ref(self).start(Task::new(id, node_id, body)).await
            }
            StartTaskBody::ExecuteUsecase(body) => {
                JobServiceImpl::inj_ref(self).start(Task::new(id, node_id, body)).await
            }
            StartTaskBody::CollectOuput(body) => {
                CollectOutputService::inj_ref(self).start(Task::new(id, node_id, body)).await
            }
            StartTaskBody::UploadFile(body) => {
                UploadFileService::inj_ref(self).start(Task::new(id, node_id, body)).await
            }
            StartTaskBody::DownloadFile(body) => {
                DownloadFileService::inj_ref(self).start(Task::new(id, node_id, body)).await
            }
        } {
            tracing::error!(task_id = %id, %task_type, "[run] {e}");
        }
    }

    async fn pause(&self, r#type: TaskType, id: Uuid) {
        if let Err(e) = match r#type {
            TaskType::DeploySoftware => DeploySoftwareService::inj_ref(self).pause(id).await,
            TaskType::ExecuteUsecase => JobServiceImpl::inj_ref(self).pause(id).await,
            TaskType::CollectOuput => CollectOutputService::inj_ref(self).pause(id).await,
            TaskType::UploadFile => UploadFileService::inj_ref(self).pause(id).await,
            TaskType::DownloadFile => DownloadFileService::inj_ref(self).pause(id).await,
        } {
            let task_type = r#type.to_str();
            tracing::error!(task_id = %id, %task_type, "[pause] {e}");
        }
    }

    async fn resume(&self, r#type: TaskType, id: Uuid) {
        if let Err(e) = match r#type {
            TaskType::DeploySoftware => DeploySoftwareService::inj_ref(self).resume(id).await,
            TaskType::ExecuteUsecase => JobServiceImpl::inj_ref(self).resume(id).await,
            TaskType::CollectOuput => CollectOutputService::inj_ref(self).resume(id).await,
            TaskType::UploadFile => UploadFileService::inj_ref(self).resume(id).await,
            TaskType::DownloadFile => DownloadFileService::inj_ref(self).resume(id).await,
        } {
            let task_type = r#type.to_str();
            tracing::error!(task_id = %id, %task_type, "[continue] {e}");
        }
    }

    async fn cancel(&self, r#type: TaskType, id: Uuid) {
        if let Err(e) = match r#type {
            TaskType::DeploySoftware => DeploySoftwareService::inj_ref(self).cancel(id).await,
            TaskType::ExecuteUsecase => JobServiceImpl::inj_ref(self).cancel(id).await,
            TaskType::CollectOuput => CollectOutputService::inj_ref(self).cancel(id).await,
            TaskType::UploadFile => UploadFileService::inj_ref(self).cancel(id).await,
            TaskType::DownloadFile => DownloadFileService::inj_ref(self).cancel(id).await,
        } {
            let task_type = r#type.to_str();
            tracing::error!(task_id = %id, %task_type, "[cancel] {e}");
        }
    }
}

#[async_trait::async_trait]
impl SchedulerStat for Container {
    async fn total(&self) -> anyhow::Result<SchedulerTotalResources> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => Pbs::inj_ref(self).total().await,
            JobSchedulerState::Slurm(_) => Slurm::inj_ref(self).total().await,
            JobSchedulerState::Lsf(_) => anyhow::bail!("Lsf doesn't implement resource_stat"),
        }
    }

    async fn used(&self) -> anyhow::Result<SchedulerUsedResources> {
        match self.job_scheduler {
            JobSchedulerState::Pbs(_) => Pbs::inj_ref(self).used().await,
            JobSchedulerState::Slurm(_) => Slurm::inj_ref(self).used().await,
            JobSchedulerState::Lsf(_) => anyhow::bail!("Lsf doesn't implement resource_stat"),
        }
    }
}

#[async_trait::async_trait]
impl ResourceStat for Container {
    async fn total(&self) -> anyhow::Result<TotalResources> {
        ResourceStatImpl::inj_ref(self).total().await
    }

    async fn used(&self) -> anyhow::Result<UsedResources> {
        ResourceStatImpl::inj_ref(self).used().await
    }
}
