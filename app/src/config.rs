use alice_infrastructure::config::CommonConfig;
use bytesize::ByteSize;
use serde::*;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default, flatten)]
    pub common: CommonConfig,

    pub server: Url,

    pub oidc_server: Url,

    #[serde(default = "AgentConfig::default_upload_part_size")]
    pub upload_part_size: ByteSize,

    #[serde(default = "AgentConfig::default_download_part_size")]
    pub download_part_size: ByteSize,

    #[serde(default = "AgentConfig::default_refresh_jobs_interval")]
    pub refresh_jobs_interval: u64,

    #[serde(default = "AgentConfig::default_save_path")]
    pub save_path: String,

    #[serde(default = "AgentConfig::default_save_path")]
    pub container_save_path: String,

    #[serde(default = "AgentConfig::default_include_env_script_path")]
    pub include_env_script_path: String,

    #[serde(default = "Default::default")]
    pub include_env_script: String,

    #[serde(default = "Default::default")]
    pub scheduler: SchedulerConfig,

    #[serde(default = "Default::default")]
    pub ssh_proxy: Option<SshProxyConfig>,

    #[serde(default = "AgentConfig::default_client_id")]
    pub client_id: String,

    #[serde(default = "AgentConfig::default_mpi")]
    pub mpi: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default = "SchedulerConfig::default_type")]
    pub r#type: String,

    #[serde(default = "Default::default")]
    pub queue: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SshProxyConfig {
    pub host: String,

    pub username: String,

    #[serde(default = "SshProxyConfig::default_port")]
    pub port: u16,

    #[serde(default = "SshProxyConfig::default_home_dir")]
    pub home_dir: String,

    #[serde(default = "SshProxyConfig::default_save_dir")]
    pub save_dir: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoginConfig {
    pub client_id: String,
}

impl AgentConfig {
    pub fn default_upload_part_size() -> ByteSize {
        ByteSize::kib(4)
    }

    pub fn default_download_part_size() -> ByteSize {
        ByteSize::mib(16)
    }

    pub fn default_save_path() -> String {
        ".".to_owned()
    }

    pub fn default_include_env_script_path() -> String {
        "included.sh".to_owned()
    }

    pub fn default_client_id() -> String {
        "device".to_owned()
    }

    pub fn default_refresh_jobs_interval() -> u64 {
        60
    }

    pub fn default_mpi() -> bool {
        true
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            r#type: Self::default_type(),
            queue: None,
        }
    }
}

impl SchedulerConfig {
    pub fn default_type() -> String {
        "slurm".to_owned()
    }
}

impl Default for SshProxyConfig {
    fn default() -> Self {
        Self {
            host: Default::default(),
            username: Default::default(),
            port: Self::default_port(),
            home_dir: Self::default_home_dir(),
            save_dir: Self::default_save_dir(),
        }
    }
}

impl SshProxyConfig {
    pub fn default_port() -> u16 {
        22
    }

    pub fn default_home_dir() -> String {
        "~".to_owned()
    }

    pub fn default_save_dir() -> String {
        "agent/tasks".to_owned()
    }
}
