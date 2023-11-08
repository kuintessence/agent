use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFile {
    pub path: String,
    #[serde(flatten)]
    pub kind: FileTransmitKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum FileTransmitKind {
    /// 从中心下载
    #[serde(rename_all = "camelCase")]
    Center { file_id: Uuid, is_packaged: bool },
    /// P2P 下载
    #[serde(rename_all = "camelCase")]
    P2P { file_id: Uuid, is_packaged: bool },
    /// 直接读取文字
    Text { content: String },
}
