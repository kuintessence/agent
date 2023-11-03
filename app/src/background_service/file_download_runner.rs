use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alice_architecture::background_service::BackgroundService;
use anyhow::Context;
use domain::{
    command::FileTransferCommand, model::vo::FileTransferStatus, sender::IDownloadSender,
    service::RunJobService,
};
use reqwest_middleware::ClientWithMiddleware;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::Instrument;
use typed_builder::TypedBuilder;
use url::Url;

use crate::infrastructure::{
    command::{Scp, SshProxy},
    http::header::TASK_ID,
};

#[derive(TypedBuilder)]
pub struct FileDownloadRunner {
    save_dir: String,
    receiver: flume::Receiver<FileTransferCommand>,
    #[builder(setter(transform = |cw: ClientWithMiddleware| Arc::new(cw)))]
    http_client: Arc<ClientWithMiddleware>,
    #[builder(setter(transform = |cw: ClientWithMiddleware| Arc::new(cw)))]
    download_client: Arc<ClientWithMiddleware>,
    base_url: Url,
    run_task: Arc<dyn RunJobService>,
    /// 16MiB
    #[builder(default = 16 * 1024 * 1024)]
    block_size: u64,
    ssh_proxy: Arc<SshProxy>,
    scp: Option<Arc<Scp>>,
}

pub struct StartRoutine {
    save_dir: String,
    http_client: Arc<ClientWithMiddleware>,
    download_client: Arc<ClientWithMiddleware>,
    base_url: Url,
    run_task: Arc<dyn RunJobService>,
    max_block_length: u64,
    ssh_proxy: Arc<SshProxy>,
    scp: Option<Arc<Scp>>,
    command: FileTransferCommand,
}

#[async_trait::async_trait]
impl BackgroundService for FileDownloadRunner {
    async fn run(&self) {
        loop {
            match self.receiver.recv_async().await {
                Ok(command) => {
                    if let FileTransferStatus::Start = command.status {
                        let start_routine = StartRoutine::new(self, command);
                        tokio::spawn(
                            async move { start_routine.run().await }
                                .instrument(tracing::trace_span!("file_download_runner")),
                        );
                    }
                }
                Err(e) => tracing::error!("{e} 2"),
            }
        }
    }
}

pub struct DownloadSender {
    sender: Arc<flume::Sender<FileTransferCommand>>,
    receiver: flume::Receiver<FileTransferCommand>,
}

#[async_trait::async_trait]
impl IDownloadSender for DownloadSender {
    async fn send(&self, command: FileTransferCommand) -> anyhow::Result<()> {
        Ok(self.sender.send(command)?)
    }
}

impl DownloadSender {
    pub fn new() -> Self {
        let (sender, receiver): (
            flume::Sender<FileTransferCommand>,
            flume::Receiver<FileTransferCommand>,
        ) = flume::unbounded();
        Self {
            sender: Arc::from(sender),
            receiver,
        }
    }

    pub fn get_receiver(&self) -> flume::Receiver<FileTransferCommand> {
        self.receiver.clone()
    }
}

impl StartRoutine {
    fn new(file_download_runner: &FileDownloadRunner, command: FileTransferCommand) -> Self {
        Self {
            save_dir: file_download_runner.save_dir.clone(),
            http_client: file_download_runner.http_client.clone(),
            download_client: file_download_runner.download_client.clone(),
            base_url: file_download_runner.base_url.clone(),
            run_task: file_download_runner.run_task.clone(),
            max_block_length: file_download_runner.block_size,
            ssh_proxy: file_download_runner.ssh_proxy.clone(),
            scp: file_download_runner.scp.clone(),
            command,
        }
    }

    async fn run(self) {
        let Self {
            save_dir,
            http_client,
            download_client,
            base_url,
            run_task,
            max_block_length,
            ssh_proxy,
            scp,
            mut command,
        } = self;
        let task_file = command.task_file.take().unwrap();

        let task = async {
            let mut path = PathBuf::new();
            path.push(save_dir.as_str());
            path.push(command.parent_id.to_string());
            path.push(task_file.file_name.as_str());
            if !path.exists() {
                tracing::trace!(
                    "File {} created directory {}.",
                    task_file.metadata_id,
                    path.to_string_lossy()
                );
                tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            }
            tracing::debug!(
                "Starting download file {} into {}.",
                task_file.metadata_id,
                task_file.file_name
            );
            tracing::trace!("Download file info: {task_file:#?}");
            if task_file.is_generated {
                tokio::fs::write(path, task_file.text.as_str()).await?;
                return Result::<(), anyhow::Error>::Ok(());
            }

            let url = base_url
                .join("file-storage/RangelyDownloadFile/")
                .unwrap()
                .join(&task_file.metadata_id.to_string())
                .unwrap();
            let task_id = command.parent_id.to_string();
            tracing::trace!("File {} is downloading from {}", task_file.metadata_id, url);
            let file_length = http_client
                .head(url.clone())
                .header(TASK_ID, &task_id)
                .send()
                .await
                .ok()
                .as_ref()
                .and_then(|resp| resp.headers().get("Content-Length"))
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default();

            if file_length == 0 {
                let bytes = http_client
                    .get(url)
                    .header(TASK_ID, &task_id)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;
                tracing::trace!("File {} download finished.", task_file.metadata_id);
                let mut file = tokio::fs::File::create(path).await?;
                tokio::io::copy(&mut bytes.as_ref(), &mut file).await?;
                tracing::debug!(
                    "File {} was written into {}",
                    task_file.metadata_id,
                    task_file.file_name
                );
            } else {
                let file = tokio::fs::File::create(path).await?;
                file.set_len(file_length).await?;
                let (block_length, block_count) = {
                    if file_length > max_block_length {
                        if file_length % max_block_length != 0 {
                            (max_block_length, file_length / max_block_length + 1)
                        } else {
                            (max_block_length, file_length / max_block_length)
                        }
                    } else {
                        (file_length, 1)
                    }
                };
                let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
                let mut tasks = vec![];
                for i in 0..block_count {
                    let url = url.clone();
                    let client = download_client.clone();
                    let writer = file.try_clone().await.unwrap();
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let task = tokio::spawn(async move {
                        let task = download_task(
                            block_count,
                            file_length,
                            i,
                            block_length,
                            client,
                            url,
                            writer,
                        )
                        .await;
                        drop(permit);
                        task
                    });
                    tasks.push(task);
                }
                for task in tasks {
                    task.await.with_context(|| {
                        format!(
                            "Cannot download File {} to Node {}",
                            task_file.metadata_id, command.parent_id
                        )
                    })??;
                }
            }

            Ok(())
        };

        if let Result::<(), anyhow::Error>::Err(e) = async {
            match task.await {
                Ok(_) => {
                    if let Some(scp) = scp {
                        let ssh = scp.config();
                        let remote_path = PathBuf::from_iter([
                            &ssh.home_dir,
                            &ssh.save_dir,
                            &command.parent_id.to_string(),
                            &task_file.file_name,
                        ]);
                        let path = PathBuf::from_iter([
                            &save_dir,
                            &command.parent_id.to_string(),
                            &task_file.file_name,
                        ]);

                        let mut cmd = scp.command();
                        let cmd = cmd.local_path(&path).remote_path(&remote_path);
                        let out = ssh_proxy
                            .command("mkdir")
                            .arg("-p")
                            .arg(remote_path.parent().unwrap())
                            .output()
                            .await?;
                        if !out.status.success() {
                            tracing::error!(
                                "Unable to create directory {:?} on remote for download.",
                                remote_path.parent().unwrap()
                            );
                        }
                        match cmd.spawn() {
                            Ok(mut x) => match x.wait().await {
                                Ok(x) => {
                                    if !x.success() {
                                        run_task
                                            .fail_job(
                                                task_file.related_task_body,
                                                &format!(
                                                    "Unable to download file {}, because of ssh transport not success.",
                                                    task_file.metadata_id
                                            ))
                                            .await?
                                    }
                                }
                                Err(e) => {
                                    run_task
                                        .fail_job(
                                            task_file.related_task_body,
                                            &format!(
                                                "Unable to download file {}, because of {e}",
                                                task_file.metadata_id
                                        ))
                                        .await?
                                }
                            },
                            Err(e) => {
                                run_task
                                    .fail_job(
                                        task_file.related_task_body,
                                        &format!(
                                            "Unable to download file {}, because of {e}",
                                            task_file.metadata_id
                                    ))
                                    .await?
                            }
                        }
                    }

                    let sleep_time = {
                        let sleep_time = rand::random::<u8>() as u64;
                        let jitter = rand::random::<u8>() as u64;
                        (if sleep_time > 0 && sleep_time <= 10 {
                            sleep_time * 1000
                        } else if sleep_time > 10 && sleep_time <= 100 {
                            sleep_time * 100
                        } else {
                            1000
                        }) + jitter
                    };
                    tokio::time::sleep(Duration::from_millis(sleep_time)).await;

                    run_task.run_job(task_file.id).await?;

                    Ok(())
                }
                Err(e) => {
                    tracing::error!("{e}");
                    run_task
                        .fail_job(
                            task_file.related_task_body,
                            &format!(
                                "Unable to download file {}, because of {e}",
                                task_file.metadata_id
                            ),
                        )
                        .await?;

                    Ok(())
                }
            }
        }
        .await
        {
            tracing::error!("{e}");
        };
    }
}

async fn download_task(
    block_count: u64,
    file_length: u64,
    block: u64,
    block_length: u64,
    client: Arc<ClientWithMiddleware>,
    url: url::Url,
    mut writer: File,
) -> anyhow::Result<()> {
    let real_block_length = if block == (block_count - 1) {
        file_length - block * block_length - 1
    } else {
        block_length - 1
    };
    let start = block * block_length;
    let end = block * block_length + real_block_length;
    let bytes = client
        .get(url.clone())
        .header("Range", format!("bytes={start}-{end}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    writer.seek(SeekFrom::Start(block * block_length)).await?;
    writer.write_all_buf(&mut bytes.as_ref()).await?;
    writer.flush().await?;

    Ok(())
}
