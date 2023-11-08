pub mod collect_output;
pub mod deploy_software;
pub mod job;

pub mod prelude {
    #[rustfmt::skip]
    pub use super::{
        collect_output::{CollectOutputService, CollectOutputState},
        deploy_software::{DeploySoftwareService, DeploySoftwareState},
        job::{JobServiceImpl, JobServiceState},
    };
}
