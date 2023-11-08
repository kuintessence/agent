use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectOutput {
    /// 从哪收集
    pub from: CollectFrom,
    /// 收集规则
    pub rule: CollectRule,
    /// 收集到哪里
    pub to: CollectTo,
    /// 如果收集不到是否报错（true 时不报错）
    pub optional: bool,
}

/// 从哪里收集
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum CollectFrom {
    /// 收集文件输出
    #[serde(rename_all = "camelCase")]
    FileOut { path: String },
    /// 收集标准输出
    Stdout,
    /// 收集标准错误输出
    Stderr,
}

/// 结果输出形式
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum CollectTo {
    /// 输出为文件
    #[serde(rename_all = "camelCase")]
    File { path: String },
    /// 输出为文字
    #[serde(rename_all = "camelCase")]
    Text { id: Uuid },
}

/// 收集规则
#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum CollectRule {
    /// 正则匹配
    Regex(String),
    /// 最后几行
    BottomLines(usize),
    /// 前几行
    TopLines(usize),
}
