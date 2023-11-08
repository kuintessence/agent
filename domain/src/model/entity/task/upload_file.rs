use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
    pub file_id: Uuid,
    pub path: String,
    /// 是否打包上传
    pub is_package: bool,
    /// 上传前验证文件内容
    #[serde(flatten)]
    pub validator: Option<OutValidator>,
    /// 该文件对于节点而言是否可有可无
    pub optional: bool,
}

/// 输出校验器
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutValidator {
    /// 校验规则
    pub validate_rule: ValidateRule,
    /// 校验成功时的操作 (需逻辑检查与 failure_operation 至少有一个)
    pub pass_operation: ValidatedOperation,
    /// 校验失败时的操作
    pub failure_operation: ValidatedOperation,
}

/// 校验规则
#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum ValidateRule {
    /// 匹配正则
    Regex(String),
    /// 是否为空
    IsEmpty(bool),
}

/// 验证过后的操作
#[derive(Debug, Deserialize)]
pub enum ValidatedOperation {
    /// 报告成功
    ReportSuccess,
    /// 报告失败
    ReportFailure,
}
