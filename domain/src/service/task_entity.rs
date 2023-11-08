use crate::model::entity::task::{
    CollectOutput, DeploySoftware, DownloadFile, ExecuteUsecase, UploadFile,
};

pub trait TaskEntity {
    const TYPE: &'static str;
}

impl TaskEntity for DeploySoftware {
    const TYPE: &'static str = "Deploy Software";
}

impl TaskEntity for DownloadFile {
    const TYPE: &'static str = "Download File";
}

impl TaskEntity for ExecuteUsecase {
    const TYPE: &'static str = "Execute Usecase";
}

impl TaskEntity for CollectOutput {
    const TYPE: &'static str = "Collect Output";
}

impl TaskEntity for UploadFile {
    const TYPE: &'static str = "Upload File";
}
