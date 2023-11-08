use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePartialUploadFromNodeInstance {
    pub file_name: String,
    pub hash: String,
    pub hash_algorithm: String,
    pub size: u64,
    pub count: u64,
    pub node_instance_uuid: Uuid,
    pub file_metadata_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialUploadInfo {
    pub shards: Vec<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncompleteOldUpload {
    pub meta_id: Uuid,
}
