use domain::model::entity::{job::JobResources, task::*};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 任务
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub command: TaskCommand,
}

/// 任务目标状态
#[derive(Debug, Deserialize)]
pub enum TaskCommand {
    Start,
    Pause,
    Resume,
    Cancel,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStart {
    pub node_id: Uuid,
    #[serde(flatten)]
    pub body: StartTaskBody,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "body")]
pub enum StartTaskBody {
    /// 软件部署
    DeploySoftware(DeploySoftware),
    /// 文件下载
    DownloadFile(DownloadFile),
    /// 用例执行
    ExecuteUsecase(ExecuteUsecase),
    /// 文件上传
    UploadFile(UploadFile),
    /// 输出收集
    CollectOuput(CollectOutput),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum TaskType {
    /// 软件部署
    DeploySoftware,
    /// 文件下载
    DownloadFile,
    /// 用例执行
    ExecuteUsecase,
    /// 文件上传
    UploadFile,
    /// 输出收集
    CollectOuput,
}

/// 任务结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult<'a> {
    /// 任务 id
    pub id: Uuid,
    /// 任务结果状态
    pub status: TaskStatus,
    /// 输出信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'a str>,
}

/// 附带作业使用资源的任务结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResultWithResource {
    /// 任务 id
    pub id: Uuid,
    /// 任务结果状态
    pub status: TaskStatus,
    /// 作业使用的资源
    pub used_resources: JobResources,
}

impl StartTaskBody {
    pub fn r#type(&self) -> TaskType {
        match self {
            Self::DeploySoftware(_) => TaskType::DeploySoftware,
            Self::DownloadFile(_) => TaskType::DownloadFile,
            Self::ExecuteUsecase(_) => TaskType::ExecuteUsecase,
            Self::UploadFile(_) => TaskType::UploadFile,
            Self::CollectOuput(_) => TaskType::CollectOuput,
        }
    }
}

impl TaskType {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::DeploySoftware => "deploy software",
            Self::DownloadFile => "download file",
            Self::ExecuteUsecase => "execute usecase",
            Self::UploadFile => "upload file",
            Self::CollectOuput => "collect output",
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::{Task, TaskStart, TaskType};

    #[test]
    fn test_start() {
        let s = indoc! {
            r#"{
                 "id": "46099d7c-a982-41a0-9370-cac6df35114e",
                 "command": "Start",
                 "nodeId": "10b712f0-5577-4f79-a582-330b51abbc13",
                 "type": "ExecuteUsecase",
                 "body": {
                   "name": "./coawstM",
                   "facilityKind": {
                     "type": "Spack",
                     "name": "tar",
                     "argumentList": []
                   },
                   "arguments": ["coawst.in"],
                   "environments": {},
                   "requirements": {
                     "cpuCores": 56
                   }
                 }
               }"#
        };
        serde_json::from_str::<Task>(s).unwrap();
        serde_json::from_str::<TaskStart>(s).unwrap();
    }

    #[test]
    fn test_other() {
        let s = indoc! {
            r#"{
                 "id": "40e83b08-fb1b-497c-8808-de72c8824143",
                 "command": "Pause",
                 "type": "ExecuteUsecase"
               }"#
        };
        serde_json::from_str::<Task>(s).unwrap();
        serde_json::from_str::<TaskType>(s).unwrap();
    }
}
