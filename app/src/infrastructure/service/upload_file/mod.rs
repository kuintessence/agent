mod supervisor;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use crossbeam_queue::ArrayQueue;
use dep_inj::DepInj;
use domain::service::TaskStatusReporter;
use domain::{
    model::entity::task::{upload_file::UploadFile, Task, TaskStatus},
    service::TaskService,
};
use reqwest_middleware::ClientWithMiddleware;
use retry_policies::policies::ExponentialBackoff;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use typed_builder::TypedBuilder;
use url::Url;
use uuid::Uuid;

use self::supervisor::UploadFileSupervisor;
use crate::{
    dto::{IncompleteOldUpload, PartialUploadInfo, StatusCode},
    infrastructure::http::{
        header::TASK_ID,
        middleware::{RetryOnError, RetryStreamClient},
    },
};
use crate::{
    dto::{ParseReply, PreparePartialUploadFromNodeInstance},
    infrastructure::command::Scp,
};

#[derive(TypedBuilder)]
pub struct RawUploadFileService {
    save_dir: String,
    base_url: Url,
    block_size: u64,
    client: ClientWithMiddleware,
    #[builder(setter(transform = |client: ClientWithMiddleware, retries: u32| {
        let policy = ExponentialBackoff::builder().build_with_max_retries(retries);
        RetryStreamClient::new(client, policy, RetryOnError)
    }))]
    stream_client: RetryStreamClient<ExponentialBackoff, RetryOnError>,
}

#[derive(DepInj)]
#[target(UploadFileService)]
pub struct UploadFileState {
    id2supervisor: Mutex<HashMap<Uuid, Arc<UploadFileSupervisor>>>,
    save_dir: String,
    prepare_upload_url: Url,
    upload_info_url: Url,
    client: ClientWithMiddleware,
    inner: Arc<UploadFileServiceInner>,
}

pub struct UploadFileServiceInner {
    block_size: u64,
    upload_url: Url,
    retry_stream_req: RetryStreamClient<ExponentialBackoff, RetryOnError>,
}

impl From<RawUploadFileService> for UploadFileState {
    fn from(raw: RawUploadFileService) -> Self {
        let RawUploadFileService {
            save_dir,
            base_url,
            client,
            block_size,
            stream_client: retry_stream_req,
        } = raw;

        Self {
            id2supervisor: Mutex::default(),
            save_dir,
            prepare_upload_url: base_url
                .join("file-storage/PreparePartialUploadFromNodeInstance")
                .unwrap(),
            upload_info_url: base_url.join("file-storage/PartialUploadInfo/").unwrap(),
            client,
            inner: Arc::new(UploadFileServiceInner {
                block_size,
                upload_url: base_url.join("file-storage/PartialUpload").unwrap(),
                retry_stream_req,
            }),
        }
    }
}

#[async_trait::async_trait]
impl<Deps> TaskService for UploadFileService<Deps>
where
    Deps: AsRef<UploadFileState> + TaskStatusReporter<UploadFile> + Scp + Send + Sync,
{
    type Body = UploadFile;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()> {
        let id = task.id;
        let result = self.run(task).await;
        // Try to remove supervisor as cancel may remove it before
        self.id2supervisor.lock().await.remove(&id);

        match result {
            Ok("") => self.prj_ref().report(id, TaskStatus::Completed).await,
            Ok("cancel") => Ok(()),
            Ok(msg) => self.prj_ref().report_msg(id, TaskStatus::Completed, msg).await,
            Err(e) => self.prj_ref().report_msg(id, TaskStatus::Failed, &e.to_string()).await,
        }
    }

    async fn pause(&self, id: Uuid) -> anyhow::Result<()> {
        {
            let map = self.id2supervisor.lock().await;
            let supervisor = map.get(&id).context("Upload file supervisor not found")?;
            supervisor.pause_token.pause();
            let mut cancel_workers = supervisor.cancel_workers.lock().await;
            cancel_workers.cancel();
            *cancel_workers = CancellationToken::new();
        }

        self.prj_ref().report(id, TaskStatus::Paused).await
    }

    async fn resume(&self, id: Uuid) -> anyhow::Result<()> {
        self.id2supervisor
            .lock()
            .await
            .get(&id)
            .context("Upload file supervisor not found")?
            .pause_token
            .resume();

        self.prj_ref().report(id, TaskStatus::Resumed).await
    }

    async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let supervisor = self
            .id2supervisor
            .lock()
            .await
            .remove(&id)
            .context("Upload file supervisor not found")?;
        supervisor.cancel_upload.cancel();

        self.prj_ref().report(id, TaskStatus::Cancelled).await
    }
}

impl<Deps> UploadFileService<Deps>
where
    Deps: AsRef<UploadFileState> + TaskStatusReporter<UploadFile> + Scp + Send + Sync,
{
    async fn run(&self, task: Task<<Self as TaskService>::Body>) -> anyhow::Result<&'static str> {
        let task_id = task.id.to_string();
        let mut task_file = task.body;

        let file_pos =
            PathBuf::from_iter([&self.save_dir, &task.node_id.to_string(), &task_file.path]);

        if let Some((mut scp, ssh)) = self.prj_ref().scp() {
            let remote_path =
                PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, &task_id, &task_file.path]);
            scp.rec()
                .remote_path(&remote_path)
                .local_path(&file_pos)
                .spawn()?
                .wait()
                .await?;
        }

        self.prj_ref()
            .report_msg(
                task.id,
                TaskStatus::Started,
                &format!("uploading file {} to {}", task_file.file_id, task_file.path),
            )
            .await?;

        let mut file = match File::open(&file_pos).await {
            Ok(fd) => fd,
            Err(e) if e.kind() == io::ErrorKind::NotFound && task_file.optional => {
                return Ok("File not found but it is optional");
            }
            Err(e) => return Err(e.into()),
        };

        let file_size = file.metadata().await?.len();
        let block_count = file_size.div_ceil(self.inner.block_size);
        let hash = hash_file(&mut file).await?;

        let json = PreparePartialUploadFromNodeInstance {
            file_name: task_file.path.clone(),
            hash_algorithm: "blake3".to_string(),
            hash,
            size: file_size,
            count: block_count,
            node_instance_uuid: task.node_id,
            file_metadata_id: Some(task_file.file_id),
        };
        let resp = self
            .client
            .post(self.prepare_upload_url.clone())
            .header(TASK_ID, &task_id)
            .json(&json)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        let incomplete_upload = match resp.status()? {
            StatusCode::OK => false,
            StatusCode::FLASH_UPLOAD => return Ok(""),
            StatusCode::INCOMPLETE_UPLOAD => true,
            StatusCode::INCOMPLETE_OLD_UPLOAD => {
                task_file.file_id = resp.content::<IncompleteOldUpload>()?.meta_id;
                true
            }
            _ => return Err(resp.error()?.into()),
        };
        let index_queue = if incomplete_upload {
            let upload_info_url =
                self.upload_info_url.join(&task_file.file_id.to_string()).unwrap();
            let upload_info: PartialUploadInfo = self
                .client
                .get(upload_info_url)
                .header(TASK_ID, &task_id)
                .send()
                .await?
                .bytes()
                .await?
                .content()?;
            let index_queue = ArrayQueue::new(upload_info.shards.len());
            for i in upload_info.shards {
                // safe because of reserved capacity
                index_queue.push(i).unwrap();
            }
            index_queue
        } else {
            let index_queue = ArrayQueue::new(block_count as usize);
            for i in 0..block_count {
                // safe because of reserved capacity
                index_queue.push(i).unwrap();
            }
            index_queue
        };

        let supervisor = Arc::new(UploadFileSupervisor::new(
            &self.inner,
            task.id,
            file,
            task_file.file_id,
            task_file.path,
            index_queue,
        ));
        self.id2supervisor.lock().await.insert(task.id, supervisor.clone());

        if supervisor.run().await {
            return Ok("cancel");
        }

        Ok("")
    }
}

async fn hash_file(file: &mut File) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    // 64KiB per read possibly
    let mut buf = [0; 2usize.pow(16)];
    loop {
        match file.read(&mut buf).await {
            Ok(0) => return Ok(hasher.finalize().to_hex().to_uppercase()),
            Ok(n) => {
                hasher.update(&buf[..n]);
                buf[..n].fill(0);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e),
        }
    }
}
