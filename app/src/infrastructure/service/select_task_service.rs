use uuid::Uuid;

use crate::dto::{StartTaskBody, TaskType};

#[async_trait::async_trait]
pub trait SelectTaskService {
    async fn start(&self, id: Uuid, node_id: Uuid, body: StartTaskBody);
    async fn pause(&self, r#type: TaskType, id: Uuid);
    async fn resume(&self, r#type: TaskType, id: Uuid);
    async fn cancel(&self, r#type: TaskType, id: Uuid);
}
