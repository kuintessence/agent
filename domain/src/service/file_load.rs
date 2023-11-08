use uuid::Uuid;

use crate::model::entity::task::collect_output::{CollectFrom, CollectTo};

#[async_trait::async_trait]
pub trait FileLoadService {
    async fn load_file(&self, node_id: Uuid, from: &CollectFrom) -> anyhow::Result<String>;
    async fn save_file(
        &self,
        task_id: Uuid,
        node_id: Uuid,
        to: &CollectTo,
        output: &str,
    ) -> anyhow::Result<()>;
}
