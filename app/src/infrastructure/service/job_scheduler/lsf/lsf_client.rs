use dep_inj::DepInj;
use domain::{
    model::{entity::Job, vo::job::ScriptInfo},
    service::JobScheduler,
};

use crate::infrastructure::{
    command::{MaybeSsh, Scp},
    service::job_scheduler::LsfJobs,
};

#[derive(DepInj)]
#[target(LsfClient)]
pub struct LsfClientState {
    base_path: String,
    include_env: String,
}

impl LsfClientState {
    pub fn new(base_path: String, include_env: String) -> Self {
        Self {
            base_path,
            include_env,
        }
    }
}

#[async_trait::async_trait]
impl<Deps> JobScheduler for LsfClient<Deps>
where
    Deps: AsRef<LsfClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_jobs(&self) -> anyhow::Result<Vec<Job>> {
        // match self.get_lsf_jobs().await {
        //     Ok(x) => Ok(x),
        //     Err(_) => self.get_lsf_jobs_alternative().await,
        // }
        todo!()
    }
    async fn get_job(&self, id: &str) -> anyhow::Result<Job> {
        todo!()
    }
    async fn submit_job_script(&self, script_info: ScriptInfo) -> anyhow::Result<String> {
        todo!()
    }

    async fn submit_job(&self, script_path: &str) -> anyhow::Result<String> {
        todo!()
    }
    async fn delete_job(&self, job_id: &str) -> anyhow::Result<()> {
        todo!()
    }
    async fn pause_job(&self, job_id: &str) -> anyhow::Result<()> {
        todo!()
    }
    async fn continue_job(&self, job_id: &str) -> anyhow::Result<()> {
        todo!()
    }
}

impl<Deps> LsfClient<Deps>
where
    Deps: AsRef<LsfClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_lsf_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let out = self.prj_ref().command("bjobs").arg("-a").output().await?;
        if !out.status.success() {
            return Ok(vec![]);
        }
        let result = LsfJobs::new(&out.stdout)?;
        todo!()
    }
}
