use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alice_architecture::hosting::IBackgroundService;
use alice_di::*;
use domain::{
    model::{entity::task::DeployerType, vo::TaskDisplayType},
    sender::*,
    service::*,
};
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;
use service::prelude::*;
use url::Url;

use crate::background_service::{
    file_download_runner::DownloadSender, file_upload_runner::UploadSender, prelude::*,
    resource_reporter::ResourceReporter, software_deployment_runner::SoftwareDeploymentSender,
    task_scheduler_runner::SubTaskReportService,
};
use crate::infrastructure::{
    command::{Scp, SshProxy},
    database::JsonDb,
    http::middleware::{AuthMiddleware, MiddlewareMenu, RetryOnError},
    service::{
        file_load_service::FileLoadServiceImpl,
        job_schedulers::{PBSClient, SlurmClient},
        resource_stat::ResourceStat,
        software_deployers::{apptainer::ApptainerDeployer, spack::SpackDeployer},
        task_report_service::TaskReportServiceImpl,
    },
};

build_container! {
    #[derive(Clone)]
    pub struct ServiceProvider;
    params(
        config: config::Config,
        ssh_proxy: Arc<SshProxy>,
        auth_middleware: Arc<AuthMiddleware>,
        topic: String,
        resource_stat: Arc<ResourceStat>
    )
    common_config: alice_infrastructure::config::CommonConfig {
        build {
            let common_config: alice_infrastructure::config::CommonConfig = config.get("common").unwrap_or_default();
            common_config
        }
    }
    agent_config: crate::config::AgentConfig {
        build {
            let agent_config: crate::config::AgentConfig = config.get("agent").unwrap_or_default();
            agent_config
        }
    }
    repository: Arc<JsonDb> {
        build async {
            Arc::new(JsonDb::new(common_config.db().url()).await?)
        }
    }
    scp: Option<Arc<Scp>> {
        build {
            ssh_proxy.config().map(|c| Arc::new(Scp::new(c.clone())))
        }
    }
    job_scheduler: Arc<dyn JobSchedulerService> {
        build async {
            let path = Path::new(agent_config.include_env_script_path.as_str());
            let include_env = if path.is_file() {
                tokio::fs::read_to_string(path).await.unwrap_or_default()
            } else {
                agent_config.include_env_script.clone()
            };
            let result: Arc<dyn JobSchedulerService> =
                match agent_config.scheduler.r#type.to_lowercase().as_str() {
                    "pbs" => Arc::new(PBSClient::new(
                        agent_config.save_path.clone(),
                        include_env,
                        ssh_proxy.clone(),
                        scp.clone(),
                    )),
                    "slurm" => Arc::new(SlurmClient::new(
                        agent_config.save_path.clone(),
                        include_env,
                        ssh_proxy.clone(),
                        scp.clone(),
                    )),
                    _ => {
                        anyhow::bail!("job.scheduler.type hasn't been configured.")
                    }
                };
            result
        }
    }
    download_sender: Arc<DownloadSender> {
        provide [Arc<dyn IDownloadSender + Send + Sync>]
        build {
            Arc::new(DownloadSender::new())
        }
    }
    upload_sender: Arc<UploadSender> {
        provide [Arc<dyn IUploadSender + Send + Sync>]
        build {
            Arc::new(UploadSender::new())
        }
    }
    deploy_sender: Arc<SoftwareDeploymentSender> {
        provide [Arc<dyn ISoftwareDeploymentSender + Send + Sync>]
        build {
            Arc::new(SoftwareDeploymentSender::new())
        }
    }
    retry_middleware: Arc<RetryTransientMiddleware<ExponentialBackoff, RetryOnError>> {
        build {
            let policy = ExponentialBackoff::builder().build_with_max_retries(5);
            Arc::new(RetryTransientMiddleware::new_with_policy_and_strategy(policy, RetryOnError))
        }
    }
    standard_http_client: Arc<ClientWithMiddleware> {
        build {
            Arc::new(MiddlewareMenu::builder()
                .retries(5)
                .auth(auth_middleware.clone())
                .timeout(Duration::from_secs(1))
                .build()
                .make()
            )
        }
    }
    sub_task_report_service: Arc<SubTaskReportService> {
        provide [Arc<dyn ISubTaskReportService>]
        build {
            Arc::new(SubTaskReportService::new())
        }
    }
    spack_deployer_service: Arc<SpackDeployer> {
        build {
            Arc::new(SpackDeployer::new(ssh_proxy.clone()))
        }
    }
    apptainer_deployer_service: Arc<ApptainerDeployer> {
        build {
            Arc::new(ApptainerDeployer::new(
                "apptainer".to_string(),
                agent_config.container_save_path.clone(),
                None,
            ))
        }
    }
    deployers: HashMap<DeployerType, Arc<dyn SoftwareDeployerService>> {
        build {
            let mut deployers: HashMap<DeployerType, Arc<dyn SoftwareDeployerService>> = HashMap::new();
            deployers.insert(spack_deployer_service.get_deployer_type(), spack_deployer_service.clone());
            deployers.insert(apptainer_deployer_service.get_deployer_type(), apptainer_deployer_service.clone());
            deployers
        }
    }
    run_task_service: Arc<dyn RunJobService> {
        build {
            Arc::new(RunJobServiceImpl::new(
                job_scheduler.clone(),
                repository.clone(),
                repository.clone(),
                download_sender.clone(),
                upload_sender.clone(),
                sub_task_report_service.clone(),
                deployers.clone(),
            ))
        }
    }
    deploy_software_service: Arc<dyn DeploySoftwareService> {
        build {
            Arc::new(DeploySoftwareServiceImpl::new(
                repository.clone(),
                sub_task_report_service.clone(),
                deploy_sender.clone(),
                deployers.clone(),
            ))
        }
    }
    software_deployment_runner: Arc<SoftwareDeploymentRunner> {
        build {
            Arc::new(SoftwareDeploymentRunner::new(
                deploy_sender.get_receiver(),
                deploy_software_service.clone(),
            ))
        }
    }
    file_load_service: Arc<dyn FileLoadService> {
        build {
            Arc::new(FileLoadServiceImpl::builder()
                .base_path(PathBuf::from(&agent_config.save_path))
                .http_client(standard_http_client.clone())
                .stream_client(MiddlewareMenu::builder()
                    .auth(auth_middleware.clone())
                    .timeout(Duration::from_secs(30))
                    .build()
                    .make()
                )
                .retry_stream_req(3)
                .base_url(agent_config.upload_base_url.parse()?)
                .scp(scp.clone())
                .build()
            )
        }
    }
    collection_task_service: Arc<CollectionTaskServiceImpl> {
        build {
            Arc::new(CollectionTaskServiceImpl::new(
                repository.clone(),
                sub_task_report_service.clone(),
                file_load_service.clone(),
            ))
        }
    }
    task_scheduler_service: Arc<dyn TaskSchedulerService> {
        build {
            let mut sub_task_services: HashMap<TaskDisplayType, Arc<dyn SubTaskService + Sync + Send>> = HashMap::new();
            sub_task_services.insert(run_task_service.get_task_type(), run_task_service.clone());
            sub_task_services.insert(deploy_software_service.get_task_type(), deploy_software_service.clone());
            sub_task_services.insert(collection_task_service.get_task_type(), collection_task_service.clone());

            Arc::new(TaskSchedulerServiceImpl::builder()
                .repo(repository.clone())
                .sub_repo(repository.clone())
                .report_service(Arc::new(TaskReportServiceImpl::builder()
                    .url(agent_config.report_url.parse::<Url>()?.join("workflow-engine/ReceiveNodeStatus").unwrap())
                    .base(MiddlewareMenu::builder()
                        .retries(3)
                        .auth(auth_middleware.clone())
                        .timeout(Duration::from_secs(1))
                        .build()
                        .make()
                    )
                    .repo(repository.clone())
                    .build()
                ))
                .sub_task_services(sub_task_services)
                .max_tasks_count(0)
                .build()
            )
        }
    }
    task_scheduler_runner: Arc<TaskSchedulerRunner> {
        build {
            Arc::new(TaskSchedulerRunner::new(sub_task_report_service.get_receiver(), task_scheduler_service.clone()))
        }
    }
    file_download_runner: Arc<FileDownloadRunner> {
        build {
            Arc::new(
                FileDownloadRunner::builder()
                    .save_dir(agent_config.save_path.clone())
                    .receiver(download_sender.get_receiver())
                    .http_client(
                        MiddlewareMenu::builder()
                            .retries(3)
                            .auth(auth_middleware.clone())
                            .timeout(Duration::from_secs(5))
                            .build()
                            .make()
                    )
                    .download_client(
                        MiddlewareMenu::builder()
                            .retries(4)
                            .auth(auth_middleware.clone())
                            .timeout(Duration::from_secs(30))
                            .build()
                            .make()
                    )
                    .base_url(agent_config.download_base_url.parse()?)
                    .run_task(run_task_service.clone())
                    .ssh_proxy(ssh_proxy.clone())
                    .scp(scp.clone())
                    .build()
            )
        }
    }
    file_upload_runner: Arc<FileUploadRunner> {
        build {
            Arc::new(FileUploadRunner::builder()
                .save_dir(agent_config.save_path.clone())
                .receiver(upload_sender.get_receiver())
                .run_task(run_task_service.clone())
                .base_url(agent_config.upload_base_url.parse()?)
                .client(MiddlewareMenu::builder()
                    .retries(10)
                    .auth(auth_middleware.clone())
                    .timeout(Duration::from_secs(5))
                    .build()
                    .make()
                )
                .stream_client(MiddlewareMenu::builder()
                    .auth(auth_middleware.clone())
                    .timeout(Duration::from_secs(30))
                    .build()
                    .make()
                )
                .retry_stream_req(5)
                .scp(scp.clone())
                .build()
            )
        }
    }
    file_system_watch_runner: Arc<FileSystemWatchRunner> {
        build {
            Arc::new(FileSystemWatchRunner::new(
                agent_config.save_path.clone(),
                run_task_service.clone(),
                agent_config.ssh_proxy.clone()
            ))
        }
    }
    interval_runner: Arc<IntervalRunner> {
        build {
            Arc::new(IntervalRunner::new(agent_config.watch_interval, task_scheduler_service.clone(), run_task_service.clone()))
        }
    }
    message_queue: Arc<KafkaMessageQueue> {
        build async {
            Arc::new(KafkaMessageQueue::new(
                common_config.mq(),
                [topic],
                task_scheduler_service.clone(),
            ).await?)
        }
    }
    resource_reporter: Arc<ResourceReporter> {
        build {
            Arc::new(ResourceReporter::builder()
                .update_url(agent_config.report_url.parse::<Url>()?.join("/agent/UpdateUsedResource")?)
                .http_client(standard_http_client.clone())
                .stat(resource_stat.clone())
                .build()
            )
        }
    }
    background_services: Vec<Arc<dyn IBackgroundService + Send + Sync>> {
        build {
            let result: Vec<Arc<dyn IBackgroundService + Send + Sync>> =
                vec![
                    file_download_runner.clone(),
                    file_upload_runner.clone(),
                    file_system_watch_runner.clone(),
                    interval_runner.clone(),
                    message_queue.clone(),
                    task_scheduler_runner.clone(),
                    software_deployment_runner.clone(),
                    resource_reporter.clone(),
                ];
            result
        }
    }
    outer config: config::Config {}
}
