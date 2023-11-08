mod supervisor;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use dep_inj::DepInj;
use domain::{
    model::entity::task::{
        download_file::{DownloadFile, FileTransmitKind},
        Task, TaskStatus,
    },
    service::{TaskService, TaskStatusReporter},
};
use reqwest_middleware::ClientWithMiddleware;
use tokio::fs;
use tokio::fs::File;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use typed_builder::TypedBuilder;
use url::Url;
use uuid::Uuid;

use self::supervisor::DownloadFileSupervisor;
use crate::infrastructure::{
    command::{MaybeSsh, Scp},
    http::header::TASK_ID,
};

#[derive(TypedBuilder)]
pub struct RawDownloadFileService {
    save_dir: String,
    base_url: Url,
    block_size: u64,
    http_client: ClientWithMiddleware,
    download_client: ClientWithMiddleware,
}

#[derive(DepInj)]
#[target(DownloadFileService)]
pub struct DownloadFileState {
    id2supervisor: Mutex<HashMap<Uuid, Arc<DownloadFileSupervisor>>>,
    save_dir: String,
    download_url: Url,
    inner: Arc<DownloadFileServiceInner>,
}

pub struct DownloadFileServiceInner {
    block_size: u64,
    http_client: ClientWithMiddleware,
    download_client: ClientWithMiddleware,
}

impl From<RawDownloadFileService> for DownloadFileState {
    fn from(raw: RawDownloadFileService) -> Self {
        let RawDownloadFileService {
            save_dir,
            base_url,
            block_size,
            http_client,
            download_client,
        } = raw;

        Self {
            id2supervisor: Mutex::default(),
            save_dir,
            download_url: base_url.join("file-storage/RangelyDownloadFile/").unwrap(),
            inner: Arc::new(DownloadFileServiceInner {
                block_size,
                http_client,
                download_client,
            }),
        }
    }
}

#[async_trait::async_trait]
impl<Deps> TaskService for DownloadFileService<Deps>
where
    Deps:
        AsRef<DownloadFileState> + MaybeSsh + Scp + TaskStatusReporter<DownloadFile> + Send + Sync,
{
    type Body = DownloadFile;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()> {
        let id = task.id;
        let result = self.run(task).await;
        // Try to remove supervisor as cancel may remove it before
        self.id2supervisor.lock().await.remove(&id);

        match result {
            Ok("cancel") => Ok(()),
            Ok(_) => self.prj_ref().report(id, TaskStatus::Completed).await,
            Err(e) => self.prj_ref().report_msg(id, TaskStatus::Failed, &e.to_string()).await,
        }
    }

    async fn pause(&self, id: Uuid) -> anyhow::Result<()> {
        {
            let map = self.id2supervisor.lock().await;
            let supervisor = map.get(&id).context("Download file supervisor not found")?;
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
            .context("Download file supervisor not found")?
            .pause_token
            .resume();

        self.prj_ref().report(id, TaskStatus::Resumed).await
    }

    async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let cancel_download = self
            .id2supervisor
            .lock()
            .await
            .remove(&id)
            .context("Download file supervisor not found")?
            .cancel_download
            .clone();
        cancel_download.cancel();

        self.prj_ref().report(id, TaskStatus::Cancelled).await
    }
}

impl<Deps> DownloadFileService<Deps>
where
    Deps:
        AsRef<DownloadFileState> + MaybeSsh + Scp + TaskStatusReporter<DownloadFile> + Send + Sync,
{
    async fn run(&self, task: Task<<Self as TaskService>::Body>) -> anyhow::Result<&'static str> {
        let task_file = task.body;
        let file_pos =
            PathBuf::from_iter([&self.save_dir, &task.node_id.to_string(), &task_file.path]);
        if !file_pos.exists() {
            fs::create_dir_all(file_pos.parent().unwrap()).await?;
        }

        // TODO: 压缩目录
        let (file_id, _is_packaged) = match task_file.kind {
            FileTransmitKind::Center {
                file_id,
                is_packaged,
            }
            | FileTransmitKind::P2P {
                file_id,
                is_packaged,
            } => (file_id, is_packaged),
            FileTransmitKind::Text { content } => {
                fs::write(&file_pos, &content).await?;
                return Ok("");
            }
        };
        self.prj_ref()
            .report_msg(
                task.id,
                TaskStatus::Started,
                &format!("download file {file_id} to {}", task_file.path),
            )
            .await?;

        let url = self.download_url.join(&file_id.to_string()).unwrap();
        let task_id = task.id.to_string();
        let node_id = task.node_id.to_string();
        tracing::debug!("File is downloading from {url}");
        let file_size: u64 = self
            .inner
            .http_client
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

        if file_size == 0 {
            // File size is unknown, download it all at once.
            let bytes = self
                .inner
                .http_client
                .get(url)
                .header(TASK_ID, &task_id)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            fs::write(&file_pos, &bytes).await?;
        } else {
            let file = File::create(&file_pos).await?;
            file.set_len(file_size).await?;
            let block_count = file_size.div_ceil(self.inner.block_size);
            let supervisor = Arc::new(DownloadFileSupervisor::new(
                &self.inner,
                task.id,
                file,
                file_size,
                block_count,
                url,
            ));
            if supervisor.run().await {
                // It was canceled
                let _ = fs::remove_file(&file_pos).await;
                return Ok("cancel");
            }
        }
        tracing::debug!(%file_id, "File download finished");

        if let Some((mut scp, ssh)) = self.prj_ref().scp() {
            let remote_path =
                PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, &node_id, &task_file.path]);
            let path = PathBuf::from_iter([&self.save_dir, &node_id, &task_file.path]);

            self.prj_ref()
                .command("mkdir")
                .arg("-p")
                .arg(remote_path.parent().unwrap())
                .output()
                .await?;
            let output = scp.local_path(&path).remote_path(&remote_path).output().await?;
            if !output.status.success() {
                anyhow::bail!("Failed to download file ID={file_id}: SSH transport not success");
            }
        }

        Ok("")
    }
}
