use std::collections::HashMap;

use anyhow::Context;
use dep_inj::DepInj;
use domain::{
    model::entity::task::{collect_output::*, Task, TaskStatus},
    service::{FileLoadService, TaskService, TaskStatusReporter},
};
use regex::Regex;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(DepInj, Default)]
#[target(CollectOutputService)]
pub struct CollectOutputState {
    cancel_map: Mutex<HashMap<Uuid, CancellationToken>>,
}

#[async_trait::async_trait]
impl<Deps> TaskService for CollectOutputService<Deps>
where
    Deps: AsRef<CollectOutputState>
        + TaskStatusReporter<CollectOutput>
        + FileLoadService
        + Send
        + Sync,
{
    type Body = CollectOutput;

    async fn start(&self, task: Task<Self::Body>) -> anyhow::Result<()> {
        let task_id = task.id;
        self.prj_ref().report(task_id, TaskStatus::Started).await?;

        let cancel_token = CancellationToken::new();
        self.cancel_map.lock().await.insert(task_id, cancel_token.clone());

        tokio::select! {
            res = self.run(task) => {
                self.cancel_map.lock().await.remove(&task_id);
                let status = if let Err(e) = res {
                    tracing::error!(%task_id, "Collect output: {e}");
                    TaskStatus::Failed
                } else {
                    TaskStatus::Completed
                };
                self.prj_ref().report(task_id, status).await?;
            },
            _ = cancel_token.cancelled() => (),
        }

        Ok(())
    }

    async fn pause(&self, _id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }

    async fn resume(&self, _id: Uuid) -> anyhow::Result<()> {
        Ok(())
    }

    async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let cancel_token = self.cancel_map.lock().await.remove(&id).context("Task not found")?;
        cancel_token.cancel();

        self.prj_ref().report(id, TaskStatus::Cancelled).await
    }
}

impl<Deps> CollectOutputService<Deps>
where
    Deps: AsRef<CollectOutputState>
        + TaskStatusReporter<CollectOutput>
        + FileLoadService
        + Send
        + Sync,
{
    async fn run(&self, task: Task<CollectOutput>) -> anyhow::Result<()> {
        let CollectOutput {
            from,
            rule,
            to,
            optional,
        } = task.body;

        let input = match self.prj_ref().load_file(task.node_id, &from).await {
            Ok(s) => s,
            Err(e) => {
                if optional {
                    return Ok(());
                } else {
                    return Err(e).context("File not found");
                }
            }
        };
        let output = match rule {
            CollectRule::Regex(exp) => Regex::new(&exp)?
                .captures_iter(&input)
                .filter_map(|x| x.get(0).map(|x| x.as_str()))
                .collect::<Vec<&str>>()
                .join("\n"),
            CollectRule::BottomLines(n) => {
                let lines = input.lines().count();
                input.lines().skip(lines - n).collect::<Vec<&str>>().join("\n")
            }
            CollectRule::TopLines(n) => input.lines().take(n).collect::<Vec<&str>>().join("\n"),
        };
        self.prj_ref().save_file(task.id, task.node_id, &to, &output).await?;

        Ok(())
    }
}
