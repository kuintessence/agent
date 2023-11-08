use std::sync::Arc;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Arc<str>,
    pub name: String,
    pub owner: String,
    pub state: JobState,
    pub exit_status_code: i32,
    pub error_output: String,
    pub resource_used: JobResources,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub enum JobState {
    Queuing,
    Running,
    Suspended,
    Completing,
    Completed,
    Failed,
    #[default]
    Unknown,
}

/// 作业使用的资源
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResources {
    /// 核心数
    pub cpu: u64,
    /// 平均内存
    pub avg_memory: u64,
    /// 最大内存
    pub max_memory: u64,
    /// 存储空间
    pub storage: u64,
    /// 墙钟时间
    pub wall_time: u64,
    /// 核心时间
    pub cpu_time: u64,
    /// 节点数
    pub node: u64,
    /// 开始时间
    pub start_time: i64,
    /// 结束时间
    pub end_time: i64,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.owner == other.owner
            && self.state == other.state
    }
}

impl Default for Job {
    fn default() -> Self {
        Self {
            id: Arc::from(String::default()),
            name: String::default(),
            owner: String::default(),
            state: JobState::default(),
            exit_status_code: 0,
            error_output: String::default(),
            resource_used: JobResources::default(),
        }
    }
}
