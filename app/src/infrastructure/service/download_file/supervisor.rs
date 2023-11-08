use std::io::SeekFrom;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crossbeam_queue::ArrayQueue;
use infrastructure::sync::PauseToken;
use tokio::fs::File;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::DownloadFileServiceInner;
use crate::infrastructure::http::header::TASK_ID;

const MAX_WORKER_COUNT: usize = 16;

pub struct DownloadFileSupervisor {
    // Used in request
    task_id: Uuid,
    // File related information
    file: Mutex<File>,
    file_size: u64,
    index_queue: ArrayQueue<u64>,
    last_index: u64,
    // status control resources
    start_guard: Arc<Semaphore>,
    pub(super) pause_token: PauseToken,
    pub(super) cancel_workers: Mutex<CancellationToken>,
    pub(super) cancel_download: CancellationToken,
    // Net
    download_url: Url,
    // Tools
    service: Weak<DownloadFileServiceInner>,
}

struct DownloadFileWorker {
    block_index: u64,
    _permit: OwnedSemaphorePermit,
    cancel_token: CancellationToken,
    supervisor: Weak<DownloadFileSupervisor>,
}

impl DownloadFileSupervisor {
    /// Download file in multipart
    ///
    /// # return
    ///
    /// The routine is cancelled or not.
    pub async fn run(self: Arc<Self>) -> bool {
        let task_id = self.task_id;
        loop {
            tokio::select! {
                permit = self.pause_token.attach(self.start_guard.clone().acquire_owned()) => {
                    if let Some(block_index) = self.index_queue.pop() {
                        let worker = DownloadFileWorker {
                            block_index,
                            _permit: permit.unwrap(),
                            cancel_token: self.cancel_workers.lock().await.clone(),
                            supervisor: Arc::downgrade(&self),
                        };
                        tokio::spawn(async move {
                            if let Err(e) = worker.start().await {
                                tracing::error!(
                                    %task_id,
                                    %block_index,
                                    "Download part failed: {e}"
                                );
                            }
                        });
                    } else if self.start_guard.available_permits() + 1 < MAX_WORKER_COUNT {
                        // Plus the just acquired one is less than 16,
                        // means there are still some workers are running.
                        // Wait for a moment and then try again.
                        sleep(Duration::from_secs(1)).await;
                    } else {
                        break false;
                    };
                }
                _ = self.cancel_download.cancelled() => {
                    self.cancel_workers.lock().await.cancel();
                    break true;
                }
            }
        }
    }

    pub fn new(
        sv: &Arc<DownloadFileServiceInner>,
        task_id: Uuid,
        file: File,
        file_size: u64,
        block_count: u64,
        download_url: Url,
    ) -> Self {
        let index_queue = ArrayQueue::new(block_count as usize);
        for i in 0..block_count {
            // safe because of reserved capacity
            index_queue.push(i).unwrap();
        }

        Self {
            task_id,
            file: Mutex::new(file),
            file_size,
            index_queue,
            last_index: block_count - 1,
            start_guard: Arc::new(Semaphore::new(MAX_WORKER_COUNT)),
            pause_token: PauseToken::default(),
            cancel_workers: Mutex::default(),
            cancel_download: CancellationToken::new(),
            download_url,
            service: Arc::downgrade(sv),
        }
    }
}

impl DownloadFileWorker {
    async fn start(&self) -> anyhow::Result<()> {
        tokio::select! {
            Err(e) = self.run() => {
                self.revert_block_index().await;
                return Err(e);
            }
            _ = self.cancel_token.cancelled() => {
                self.revert_block_index().await;
            }
            else => (),
        }

        Ok(())
    }

    async fn run(&self) -> anyhow::Result<()> {
        let supervisor = self.supervisor.upgrade().unwrap();
        let sv = supervisor.service.upgrade().unwrap();

        let start = self.block_index * sv.block_size;
        let end = if self.block_index == supervisor.last_index {
            supervisor.file_size - 1
        } else {
            start + sv.block_size - 1
        };

        let bytes = sv
            .download_client
            .get(supervisor.download_url.clone())
            .header(TASK_ID, &supervisor.task_id.to_string())
            .header("Range", format!("bytes={start}-{end}")) // The right end is inclusive
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        let mut file = supervisor.file.lock().await;
        file.seek(SeekFrom::Start(start)).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;

        Ok(())
    }

    async fn revert_block_index(&self) {
        self.supervisor.upgrade().unwrap().index_queue.push(self.block_index).unwrap();
    }
}
