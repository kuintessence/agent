use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use domain::{
    model::entity::task::{CollectFrom, CollectTo},
    service::FileLoadService,
};
use reqwest::multipart::{Form, Part};
use reqwest_middleware::ClientWithMiddleware;
use retry_policies::policies::ExponentialBackoff;
use tokio::fs;
use typed_builder::TypedBuilder;
use url::Url;
use uuid::Uuid;

use crate::{
    dto::{
        text_storage::TextStorageUploaded, PreparePartialUploadFromNodeInstanceRequest,
        PreparePartialUploadResponse, PreparePartialUploadResponseResult, Reply,
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
pub struct FileLoadServiceImpl {
    base_path: PathBuf,
    http_client: Arc<ClientWithMiddleware>,
    stream_client: ClientWithMiddleware,
    #[builder(setter(transform = |retries: u32| {
        let policy = ExponentialBackoff::builder().build_with_max_retries(retries);
        RetryStreamRequest::new(policy, RetryOnError)
    }))]
    retry_stream_req: RetryStreamRequest<ExponentialBackoff, RetryOnError>,
    base_url: Url,
    scp: Option<Arc<Scp>>,
}

#[async_trait::async_trait]
impl FileLoadService for FileLoadServiceImpl {
    async fn load_file(&self, parent_id: &str, from: &CollectFrom) -> anyhow::Result<String> {
        let Some(scp) = &self.scp else {
            let mut p = self.base_path.join(parent_id);
            return match from {
                CollectFrom::FileOut { path } => {
                    p.push(path);
                    tracing::trace!("Load file from {}", p.to_string_lossy());
                    Ok(fs::read_to_string(p).await?)
                }
                CollectFrom::Stdout => {
                    p.push("STDOUT");
                    Ok(fs::read_to_string(p).await?)
                }
                CollectFrom::Stderr => {
                    p.push("STDERR");
                    Ok(fs::read_to_string(p).await?)
                }
            };
        };

        let ssh = scp.config();
        let mut p = PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, parent_id]);
        let mut p_local = self.base_path.join(parent_id);
        match from {
            CollectFrom::FileOut { path } => {
                p.push(path);
                p_local.push(path);
                tracing::trace!("Load file from {}", p.to_string_lossy());
                scp.command().rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
            CollectFrom::Stdout => {
                p.push("STDOUT");
                p_local.push("STDOUT");
                scp.command().rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
            CollectFrom::Stderr => {
                p.push("STDERR");
                p_local.push("STDERR");
                scp.command().rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
        }
    }

    async fn save_file(&self, parent_id: Uuid, output: &str, to: &CollectTo) -> anyhow::Result<()> {
        let task_id = parent_id.to_string();
        match to {
            CollectTo::File { id, path } => {
                let file = output.as_bytes().to_vec();
                let file_hash = blake3::hash(&file).to_hex().to_uppercase();
                let json = PreparePartialUploadFromNodeInstanceRequest {
                    file_name: path.clone(),
                    hash_algorithm: "blake3".to_string(),
                    hash: file_hash.clone(),
                    size: file.len() as u64,
                    count: 1,
                    node_instance_uuid: parent_id,
                    file_metadata_id: Some(*id),
                };

                let url = self
                    .base_url
                    .join("file-storage/PreparePartialUploadFromNodeInstance")
                    .unwrap();
                let resp: PreparePartialUploadResponse = self
                    .http_client
                    .post(url)
                    .header(TASK_ID, &task_id)
                    .json(&json)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<Reply<_>>()
                    .await?
                    .ok()?;
                if resp.result == PreparePartialUploadResponseResult::FlashUpload {
                    return Ok(());
                }

                let url = self.base_url.join("file-storage/PartialUpload").unwrap();
                self.retry_stream_req
                    .execute(|| async {
                        let part = Part::bytes(file.clone())
                            .file_name(path.clone())
                            .mime_str("application/octet-stream")
                            .context("Unable to upload")?;
                        let form = Form::new()
                            .text("nth", "0")
                            .text("file_metadata_id", resp.id.to_string())
                            .part("bin", part);

                        self.stream_client
                            .post(url.as_str())
                            .header(TASK_ID, &task_id)
                            .multipart(form)
                            .send()
                            .await
                    })
                    .await?
                    .error_for_status()?;
            }
            CollectTo::Text { id } => {
                let url = self.base_url.join("text-storage/upload").unwrap();
                self.http_client
                    .post(url)
                    .header(TASK_ID, &task_id)
                    .json(&TextStorageUploaded {
                        key: id,
                        value: output,
                    })
                    .send()
                    .await?
                    .error_for_status()
                    .context("Unable to upload")?;
            }
        }
        Ok(())
    }
}
