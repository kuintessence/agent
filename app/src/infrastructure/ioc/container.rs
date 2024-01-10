use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;
use service::{
    collect_output::CollectOutputState, deploy_software::DeploySoftwareState, job::JobServiceState,
};
use typed_builder::TypedBuilder;

use crate::infrastructure::{
    command::SshConfig,
    service::{
        download_file::DownloadFileState,
        file_load::FileLoadState,
        job_scheduler::{LsfClientState, PBSClientState, SlurmClientState},
        software_deployer::{ApptainerDeployerState, SpackDeployerState},
        task_status_reporter::TaskStatusReporterState,
        upload_file::UploadFileState,
    },
};

#[derive(derive_more::AsRef, TypedBuilder)]
pub struct Container {
    #[as_ref]
    pub(super) ssh_config: Option<SshConfig>,

    pub default_http_client: Arc<ClientWithMiddleware>,

    #[as_ref]
    pub(super) file_load: FileLoadState,

    #[as_ref]
    pub(super) task_status_reporter: TaskStatusReporterState,

    #[as_ref]
    pub(super) spack: SpackDeployerState,

    #[as_ref]
    pub(super) apptainer: ApptainerDeployerState,

    pub(super) job_scheduler: JobSchedulerState,

    #[as_ref]
    pub(super) deploy_software: DeploySoftwareState,

    #[as_ref]
    pub(super) download_file: DownloadFileState,

    #[as_ref]
    pub(super) job: JobServiceState,

    #[as_ref]
    pub(super) collect_output: CollectOutputState,

    #[as_ref]
    pub(super) upload_file: UploadFileState,
}

pub(super) enum JobSchedulerState {
    Pbs(PBSClientState),
    Slurm(SlurmClientState),
    Lsf(LsfClientState),
}

impl AsRef<PBSClientState> for Container {
    fn as_ref(&self) -> &PBSClientState {
        match &self.job_scheduler {
            JobSchedulerState::Pbs(client) => client,
            _ => panic!("Agent isn't usnig PBS"),
        }
    }
}

impl AsRef<SlurmClientState> for Container {
    fn as_ref(&self) -> &SlurmClientState {
        match &self.job_scheduler {
            JobSchedulerState::Slurm(client) => client,
            _ => panic!("Agent isn't usnig Slurm"),
        }
    }
}

impl AsRef<LsfClientState> for Container {
    fn as_ref(&self) -> &LsfClientState {
        match &self.job_scheduler {
            JobSchedulerState::Lsf(client) => client,
            _ => panic!("Agent isn't using Lsf"),
        }
    }
}
