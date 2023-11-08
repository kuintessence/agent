use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploySoftware {
    pub facility_kind: FacilityKind,
}

/// 软件环境技术
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum FacilityKind {
    /// spack
    #[serde(rename_all = "camelCase")]
    Spack {
        /// 软件名称
        name: String,
        /// 安装参数
        argument_list: Vec<String>,
    },
    /// singularity
    #[serde(rename_all = "camelCase")]
    Singularity {
        /// 镜像名
        image: String,
        /// 镜像 tag
        tag: String,
    },
}

#[derive(Debug, enum_map::Enum)]
pub enum DeployerType {
    Spack,
    Apptainer,
}
