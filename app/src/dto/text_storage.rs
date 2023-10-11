use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct TextStorageUploaded<'a> {
    pub key: &'a Uuid,
    pub value: &'a str,
}
