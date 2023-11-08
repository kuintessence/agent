use std::collections::HashMap;

use serde::Deserialize;

use super::deploy_software::FacilityKind;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteUsecase {
    /// 执行名称
    pub name: String,
    /// 任务软件环境技术
    pub facility_kind: FacilityKind,
    /// 参数列表
    /// 例如： ["-i a.txt","--debug"]
    pub arguments: Vec<String>,
    /// 环境变量列表，值为 None 时代表只设置键，值为空字符串
    pub environments: HashMap<String, String>,
    /// 标准输入
    pub std_in: Option<StdInKind>,
    /// 计算资源配置
    pub requirements: Option<Requirements>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum StdInKind {
    #[serde(rename_all = "camelCase")]
    Text { text: String },
    #[serde(rename_all = "camelCase")]
    File { path: String },
}

/// 节点使用资源需求
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    /// 核心数
    pub cpu_cores: Option<usize>,
    /// 节点数
    pub node_count: Option<isize>,
    /// 最长等待时间（s）
    pub max_wall_time: Option<usize>,
    /// 最大核时消耗 (s)
    pub max_cpu_time: Option<usize>,
    /// 定时终止 (utc 0 时区 时间戳)
    pub stop_time: Option<usize>,
}
