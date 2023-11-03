use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;

use alice_architecture::background_service::BackgroundService;
use domain::{
    command::FileTransferCommand, model::vo::FileTransferStatus, sender::IUploadSender,
    service::RunJobService,
};
use reqwest::multipart::*;
use reqwest_middleware::ClientWithMiddleware;
use retry_policies::policies::ExponentialBackoff;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
    sync::Semaphore,
    time::Duration,
};
use tracing::Instrument;
use typed_builder::TypedBuilder;
use url::Url;

use crate::{
    dto::{
        PreparePartialUploadFromNodeInstanceRequest, PreparePartialUploadResponse,
        PreparePartialUploadResponseResult, Reply,
    },
    infrastructure::{
        command::Scp,
        http::{
            header::TASK_ID,
            middleware::{RetryOnError, RetryStreamRequest},
        },
    },
};

#[derive(TypedBuilder)]
pub struct FileUploadRunner {
    save_dir: String,
    receiver: flume::Receiver<FileTransferCommand>,
    run_task: Arc<dyn RunJobService>,
    base_url: Url,
    #[builder(setter(transform = |client: ClientWithMiddleware| Arc::new(client)))]
    client: Arc<ClientWithMiddleware>,
    #[builder(setter(transform = |client: ClientWithMiddleware| Arc::new(client)))]
    stream_client: Arc<ClientWithMiddleware>,
    #[builder(setter(transform = |retries: u32| {
        let policy = ExponentialBackoff::builder().build_with_max_retries(retries);
        Arc::new(RetryStreamRequest::new(policy, RetryOnError))
    }))]
    retry_stream_req: Arc<RetryStreamRequest<ExponentialBackoff, RetryOnError>>,
    /// 512KiB
    #[builder(default = 1024 * 512)]
    block_size: u64,
    scp: Option<Arc<Scp>>,
}

struct StartRoutine {
    save_dir: String,
    run_task: Arc<dyn RunJobService>,
    base_url: Url,
    client: Arc<ClientWithMiddleware>,
    stream_client: Arc<ClientWithMiddleware>,
    retry_stream_req: Arc<RetryStreamRequest<ExponentialBackoff, RetryOnError>>,
    max_block_length: u64,
    scp: Option<Arc<Scp>>,
    command: FileTransferCommand,
}

#[async_trait::async_trait]
impl BackgroundService for FileUploadRunner {
    async fn run(&self) {
        loop {
            match self.receiver.recv_async().await {
                Ok(command) => {
                    if let FileTransferStatus::Start = command.status {
                        let start_routine = StartRoutine::new(self, command);
                        tokio::spawn(
                            async move { start_routine.run() }
                                .instrument(tracing::trace_span!("file_upload_runner")),
                        );
                    };
                }
                Err(e) => tracing::error!("{e}"),
            }
        }
    }
}

pub struct UploadSender {
    sender: Arc<flume::Sender<FileTransferCommand>>,
    receiver: flume::Receiver<FileTransferCommand>,
}

#[async_trait::async_trait]
impl IUploadSender for UploadSender {
    async fn send(&self, command: FileTransferCommand) -> anyhow::Result<()> {
        Ok(self.sender.send(command)?)
    }
}

impl UploadSender {
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
    fn new(upload_runner: &FileUploadRunner, command: FileTransferCommand) -> Self {
        Self {
            save_dir: upload_runner.save_dir.clone(),
            run_task: upload_runner.run_task.clone(),
            base_url: upload_runner.base_url.clone(),
            client: upload_runner.client.clone(),
            stream_client: upload_runner.stream_client.clone(),
            retry_stream_req: upload_runner.retry_stream_req.clone(),
            max_block_length: upload_runner.block_size,
            scp: upload_runner.scp.clone(),
            command,
        }
    }

    async fn run(self) {
        let Self {
            save_dir,
            run_task,
            base_url,
            client,
            stream_client,
            retry_stream_req,
            max_block_length,
            scp,
            command,
        } = self;

        let task_file = command.task_file.unwrap();
        let task = async {
            let mut file_path = PathBuf::new();
            let task_file = task_file.clone();
            tracing::debug!(
                "Starting upload file {} into {}.",
                task_file.metadata_id,
                task_file.file_name
            );
            tracing::trace!("Upload file info: {task_file:#?}");
            file_path.push(save_dir);
            file_path.push(command.parent_id.to_string());
            file_path.push(task_file.file_name.clone());
            tracing::trace!("File {} read from {}", task_file.id, task_file.file_name);

            if let Some(scp) = &scp {
                let ssh = scp.config();
                let remote_path = PathBuf::from_iter([
                    &ssh.home_dir,
                    &ssh.save_dir,
                    &command.parent_id.to_string(),
                    &task_file.file_name,
                ]);
                scp.command()
                    .rec()
                    .remote_path(&remote_path)
                    .local_path(&file_path)
                    .spawn()?
                    .wait()
                    .await?;
            }

            let mut file = match fs::File::open(&file_path).await {
                Ok(x) => x,
                Err(e) => {
                    if task_file.is_optional {
                        tracing::debug!(
                            "File {} is not existed, but is optional.",
                            task_file.metadata_id
                        );
                        return Ok(());
                    } else {
                        tracing::error!("{e}");
                        return Err(e.into());
                    }
                }
            };
            let file_hash = get_hash(&mut file).await.map_err(|e| {
                tracing::error!("{e}");
                e
            })?;
            let file_length = match file.metadata().await {
                Ok(x) => x.len(),
                Err(e) => {
                    tracing::error!("{e}");
                    return Err(e.into());
                }
            };
            let (block_length, block_count) = {
                if file_length > max_block_length {
                    if file_length % max_block_length != 0 {
                        (max_block_length, file_length / max_block_length + 1)
                    } else {
                        (max_block_length, file_length / max_block_length)
                    }
                } else {
                    (file_length, 1u64)
                }
            };
            let json = PreparePartialUploadFromNodeInstanceRequest {
                file_name: task_file.file_name.clone(),
                hash_algorithm: "blake3".to_string(),
                hash: file_hash.clone(),
                size: file_length,
                count: block_count,
                node_instance_uuid: command.parent_id,
                file_metadata_id: Some(task_file.metadata_id),
            };
            let url = base_url.join("file-storage/PreparePartialUploadFromNodeInstance").unwrap();

            let sleep_time = {
                let sleep_time = rand::random::<u8>() as u64 / 10;
                if sleep_time > 20 {
                    sleep_time - 10
                } else if sleep_time == 0 {
                    1
                } else {
                    sleep_time
                }
            };
            sleep(sleep_time).await;

            // Upload the file when server has no it.
            let resp: PreparePartialUploadResponse = match async {
                Ok(client
                    .post(url.as_str())
                    .header(TASK_ID, &json.node_instance_uuid.to_string())
                    .json(&json)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<Reply<_>>()
                    .await?
                    .ok()?)
            }
            .await
            {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!("{e}");
                    return Err(e);
                }
            };

            if resp.result != PreparePartialUploadResponseResult::FlashUpload {
                let upload_url = base_url.join("file-storage/PartialUpload").unwrap();
                let semaphore = Arc::new(Semaphore::new(16));
                let mut tasks = vec![];

                for i in 0..block_count {
                    let url = upload_url.clone();
                    let stream_client = stream_client.clone();
                    let retry_stream_req = retry_stream_req.clone();
                    let mut file = file.try_clone().await.unwrap();
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let file_name = task_file.file_name.clone();
                    let file_id = resp.id.to_string();
                    tasks.push(tokio::spawn(async move {
                        let buffer = {
                            let real_block_length = if i == (block_count - 1) {
                                file_length - i * block_length - 1
                            } else {
                                block_length - 1
                            };
                            file.seek(SeekFrom::Start(i * block_length)).await?;
                            let mut buffer = vec![0; real_block_length as usize];
                            file.read_exact(&mut buffer).await?;
                            buffer
                        };
                        let task_id = command.parent_id.to_string();
                        let resp = retry_stream_req
                            .execute(|| async {
                                let part = Part::bytes(buffer.to_vec())
                                    .file_name(file_name.to_owned())
                                    .mime_str("application/octet-stream")
                                    .unwrap();
                                let form = Form::new()
                                    .text("nth", i.to_string())
                                    .text("file_metadata_id", file_id.to_owned())
                                    .part("bin", part);
                                stream_client
                                    .post(url.clone())
                                    .header(TASK_ID, &task_id)
                                    .multipart(form)
                                    .send()
                                    .await
                            })
                            .await?
                            .error_for_status()?;

                        drop(permit);
                        Result::<_, anyhow::Error>::Ok(resp)
                    }));
                }
                for task in tasks {
                    if let Err(e) = task.await {
                        tracing::error!("{e}");
                        return Err(e.into());
                    }
                }
            }

            Result::<_, anyhow::Error>::Ok(())
        };

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
        sleep(sleep_time).await;
        match task.await {
            Ok(()) => {
                if let Err(e) = run_task.complete_job(command.id).await {
                    tracing::error!("{e}");
                }
            }
            Err(e) => {
                if let Err(e) = run_task
                    .fail_job(
                        task_file.related_task_body,
                        format!(
                            "Cannot upload File {} to Node {} when uploading block, because of {e}",
                            task_file.id, command.parent_id
                        )
                        .as_str(),
                    )
                    .await
                {
                    tracing::error!("{e}");
                }
            }
        }
    }
}

async fn get_hash(file: &mut fs::File) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let mut file = file.try_clone().await?;
    let mut buffer = [0; 2usize.pow(16)]; // 64KiB
    loop {
        match file.read(&mut buffer).await {
            Ok(0) => return Ok(hasher.finalize().to_hex().to_uppercase()),
            Ok(n) => {
                hasher.update(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e),
        }
    }
}

async fn sleep(time: u64) {
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
    } * time;
    tokio::time::sleep(Duration::from_millis(sleep_time)).await;
}
