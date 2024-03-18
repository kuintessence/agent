mod boilerplate;
mod container;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use service::prelude::*;

const REQ_TIMEOUT: Duration = Duration::from_secs(20);

use crate::{
    config::AgentConfig,
    infrastructure::{
        command::SshConfig,
        http::middleware::{AuthMiddleware, MiddlewareMenu},
        ioc::container::JobSchedulerState,
        service::{
            download_file::{DownloadFileState, RawDownloadFileService},
            file_load::FileLoadState,
            job_scheduler::{PBSClientState, SlurmClientState},
            software_deployer::{ApptainerDeployerState, SpackDeployerState},
            upload_file::{RawUploadFileService, UploadFileState},
        },
    },
};

pub use self::container::Container;

use super::service::{
    job_scheduler::LsfClientState, task_status_reporter::TaskStatusReporterState,
};

impl Container {
    pub async fn new(
        config: &AgentConfig,
        access_token: &str,
        refresh_token: String,
    ) -> anyhow::Result<Self> {
        let ssh_config = config.ssh_proxy.as_ref().map(SshConfig::new);

        let auth_middleware = Arc::new(AuthMiddleware::new(
            config.oidc_server.clone().join("token").unwrap(),
            &config.client_id,
            access_token,
            refresh_token,
            REQ_TIMEOUT,
        ));
        let default_http_client = Arc::new(
            MiddlewareMenu::builder()
                .retries(5)
                .auth(auth_middleware.clone())
                .timeout(REQ_TIMEOUT)
                .build()
                .make(),
        );

        let file_load = FileLoadState::new(
            &config.save_path,
            &config.server,
            default_http_client.clone(),
        );

        let task_status_reporter = TaskStatusReporterState::new(
            config.server.clone(),
            MiddlewareMenu::builder()
                .retries(3)
                .auth(auth_middleware.clone())
                .timeout(REQ_TIMEOUT)
                .build()
                .make(),
        );

        let apptainer = ApptainerDeployerState::new(
            "apptainer".to_string(),
            config.container_save_path.clone(),
            None,
        );

        let download_file: DownloadFileState = RawDownloadFileService::builder()
            .save_dir(config.save_path.clone())
            .base_url(config.server.clone())
            .block_size(config.download_part_size.0)
            .http_client(
                MiddlewareMenu::builder()
                    .retries(3)
                    .auth(auth_middleware.clone())
                    .timeout(REQ_TIMEOUT)
                    .build()
                    .make(),
            )
            .download_client(
                MiddlewareMenu::builder()
                    .retries(4)
                    .auth(auth_middleware.clone())
                    .timeout(Duration::from_secs(30))
                    .build()
                    .make(),
            )
            .build()
            .into();

        let upload_file: UploadFileState = RawUploadFileService::builder()
            .save_dir(config.save_path.clone())
            .base_url(config.server.clone())
            .block_size(config.upload_part_size.0)
            .client(
                MiddlewareMenu::builder()
                    .retries(10)
                    .auth(auth_middleware.clone())
                    .timeout(REQ_TIMEOUT)
                    .build()
                    .make(),
            )
            .stream_client(
                MiddlewareMenu::builder()
                    .auth(auth_middleware.clone())
                    .timeout(Duration::from_secs(30))
                    .build()
                    .make(),
                5,
            )
            .build()
            .into();

        let path = Path::new(config.include_env_script_path.as_str());
        let include_env = if path.is_file() {
            tokio::fs::read_to_string(path).await.unwrap_or_default()
        } else {
            config.include_env_script.clone()
        };
        let job_scheduler = match config.scheduler.r#type.to_lowercase().as_str() {
            "pbs" => {
                JobSchedulerState::Pbs(PBSClientState::new(config.save_path.clone(), include_env))
            }
            "slurm" => JobSchedulerState::Slurm(SlurmClientState::new(
                config.save_path.clone(),
                include_env,
                config.mpi,
            )),
            "lsf" => JobSchedulerState::Lsf(LsfClientState::new(
                config.save_path.clone(),
                include_env,
                config.scheduler.queue.clone().context("Lsf need to specify queue.")?,
            )),
            "lsf" => JobSchedulerState::Lsf(LsfClientState::new(
                config.save_path.clone(),
                include_env,
                config.scheduler.queue.clone().context("Lsf need to specify queue.")?,
            )),
            t => {
                anyhow::bail!("Unsupported `job.scheduler.type`: {t}");
            }
        };

        let container = Container::builder()
            .ssh_config(ssh_config)
            .default_http_client(default_http_client)
            .file_load(file_load)
            .task_status_reporter(task_status_reporter)
            .spack(SpackDeployerState::new())
            .apptainer(apptainer)
            .job_scheduler(job_scheduler)
            .deploy_software(DeploySoftwareState::new(5))
            .download_file(download_file)
            .job(JobServiceState::new(config.spack, config.apptainer))
            .collect_output(CollectOutputState::default())
            .upload_file(upload_file)
            .build();

        Ok(container)
    }
}
