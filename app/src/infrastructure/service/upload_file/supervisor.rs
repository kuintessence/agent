use std::io;
use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use crossbeam_queue::ArrayQueue;
use infrastructure::sync::PauseToken;
use reqwest::multipart::Form;
use reqwest::multipart::Part;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::UploadFileServiceInner;
use crate::infrastructure::http::header::TASK_ID;

const MAX_WORKER_COUNT: usize = 16;

pub struct UploadFileSupervisor {
    // Used in request
    task_id: Uuid,
    // File related information
    file: Mutex<File>,
    file_id: Uuid,
    file_path: String,
    index_queue: ArrayQueue<u64>,
    // status control resources
    start_guard: Arc<Semaphore>,
    pub(super) pause_token: PauseToken,
    pub(super) cancel_workers: Mutex<CancellationToken>,
    pub(super) cancel_upload: CancellationToken,
    // The tools we need
    service: Weak<UploadFileServiceInner>,
}

struct UploadFileWorker {
    block_index: u64,
    _permit: OwnedSemaphorePermit,
    cancel_token: CancellationToken,
    supervisor: Weak<UploadFileSupervisor>,
}

impl UploadFileSupervisor {
    /// Upload file in multipart
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
                        let worker = UploadFileWorker {
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
                                    "Upload part failed: {e}"
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
                _ = self.cancel_upload.cancelled() => {
                    self.cancel_workers.lock().await.cancel();
                    break true;
                }
            }
        }
    }

    pub fn new(
        sv: &Arc<UploadFileServiceInner>,
        task_id: Uuid,
        file: File,
        file_id: Uuid,
        file_path: String,
        index_queue: ArrayQueue<u64>,
    ) -> Self {
        Self {
            task_id,
            file: Mutex::new(file),
            file_id,
            file_path,
            index_queue,
            start_guard: Arc::new(Semaphore::new(MAX_WORKER_COUNT)),
            pause_token: PauseToken::default(),
            cancel_workers: Mutex::default(),
            cancel_upload: CancellationToken::default(),
            service: Arc::downgrade(sv),
        }
    }
}

impl UploadFileWorker {
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
        let supervisor_sv = self.supervisor.upgrade().unwrap();
        let upload_file_sv = supervisor_sv.service.upgrade().unwrap();

        let mut buf = vec![0u8; upload_file_sv.block_size as usize];

        {
            let mut file = supervisor_sv.file.lock().await;
            file.seek(SeekFrom::Start(
                self.block_index * upload_file_sv.block_size,
            ))
            .await?;
            if let Err(e) = file.read_exact(&mut buf).await {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    // Reseek will never fail because we succeed before
                    file.seek(SeekFrom::Start(
                        self.block_index * upload_file_sv.block_size,
                    ))
                    .await
                    .unwrap();
                    let mut smaller_buf = Vec::new();
                    file.read_to_end(&mut smaller_buf).await?;
                    buf = smaller_buf;
                } else {
                    return Err(e.into());
                }
            }
        }

        upload_file_sv
            .retry_stream_req
            .execute(|client| async {
                let part = Part::bytes(buf.clone())
                    .file_name(supervisor_sv.file_path.clone())
                    .mime_str("application/octet-stream")
                    .unwrap();
                let form = Form::new()
                    .text("nth", self.block_index.to_string())
                    .text("file_metadata_id", supervisor_sv.file_id.to_string())
                    .part("bin", part);

                client
                    .post(upload_file_sv.upload_url.clone())
                    .header(TASK_ID, supervisor_sv.task_id.to_string())
                    .multipart(form)
                    .send()
                    .await
            })
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn revert_block_index(&self) {
        self.supervisor.upgrade().unwrap().index_queue.push(self.block_index).unwrap();
    }
}
