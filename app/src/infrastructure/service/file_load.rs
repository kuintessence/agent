use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use dep_inj::DepInj;
use domain::{
    model::entity::task::collect_output::{CollectFrom, CollectTo},
    service::FileLoadService,
};
use reqwest_middleware::ClientWithMiddleware;
use tokio::fs;
use url::Url;
use uuid::Uuid;

use crate::{
    dto::text_storage::TextStorage,
    infrastructure::{command::Scp, http::header::TASK_ID},
};

#[derive(DepInj)]
#[target(FileLoadServiceImpl)]
pub struct FileLoadState {
    base_path: PathBuf,
    upload_text_url: Url,
    http_client: Arc<ClientWithMiddleware>,
}

impl FileLoadState {
    pub fn new(
        base_path: impl Into<PathBuf>,
        base_url: &Url,
        http_client: Arc<ClientWithMiddleware>,
    ) -> Self {
        Self {
            base_path: base_path.into(),
            upload_text_url: base_url.join("text-storage/upload").unwrap(),
            http_client,
        }
    }
}

#[async_trait::async_trait]
impl<Deps> FileLoadService for FileLoadServiceImpl<Deps>
where
    Deps: AsRef<FileLoadState> + Scp + Send + Sync,
{
    async fn load_file(&self, node_id: Uuid, from: &CollectFrom) -> anyhow::Result<String> {
        let Some((mut scp, ssh)) = self.prj_ref().scp() else {
            let mut p = self.base_path.join(node_id.to_string());
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

        let mut p = PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, &node_id.to_string()]);
        let mut p_local = self.base_path.join(node_id.to_string());
        match from {
            CollectFrom::FileOut { path } => {
                p.push(path);
                p_local.push(path);
                tracing::trace!("Load file from {}", p.to_string_lossy());
                scp.rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
            CollectFrom::Stdout => {
                p.push("STDOUT");
                p_local.push("STDOUT");
                scp.rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
            CollectFrom::Stderr => {
                p.push("STDERR");
                p_local.push("STDERR");
                scp.rec().remote_path(p).local_path(&p_local).spawn()?.wait().await?;
                Ok(fs::read_to_string(p_local).await?)
            }
        }
    }

    async fn save_file(
        &self,
        task_id: Uuid,
        node_id: Uuid,
        to: &CollectTo,
        output: &str,
    ) -> anyhow::Result<()> {
        match to {
            CollectTo::File { path } => {
                let mut file_pos = self.base_path.clone();
                file_pos.extend([&node_id.to_string(), &path]);
                fs::write(&file_pos, output).await?;
            }
            CollectTo::Text { id } => {
                self.http_client
                    .post(self.upload_text_url.clone())
                    .header(TASK_ID, task_id.to_string())
                    .json(&TextStorage {
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
