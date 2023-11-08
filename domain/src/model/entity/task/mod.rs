pub mod collect_output;
pub mod deploy_software;
pub mod download_file;
pub mod execute_usecase;
pub mod upload_file;

#[rustfmt::skip]
pub use self::{
    collect_output::CollectOutput,
    deploy_software::DeploySoftware,
    download_file::DownloadFile,
    execute_usecase::ExecuteUsecase,
    upload_file::UploadFile,
};

use serde::Serialize;
use uuid::Uuid;

#[derive(Debug)]
pub struct Task<T> {
    pub id: Uuid,
    pub node_id: Uuid,
    pub body: T,
}

/// 任务执行完的状态
#[derive(Debug, Clone, Serialize, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum TaskStatus {
    /// 任务暂不能执行，在agent上进入等待队列
    Queued,
    /// 任务开始运行
    Started,
    /// 成功执行
    Completed,
    /// 失败
    Failed,
    /// 暂停
    Paused,
    /// 继续
    Resumed,
    /// 删除
    Cancelled,
}

impl<T> Task<T> {
    pub fn new(id: Uuid, node_id: Uuid, body: T) -> Self {
        Self { id, node_id, body }
    }
}
