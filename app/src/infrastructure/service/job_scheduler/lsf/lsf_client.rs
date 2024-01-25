use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use dep_inj::DepInj;
use domain::{
    model::{
        entity::{job::JobState, task::execute_usecase::StdInKind, Job},
        vo::job::ScriptInfo,
    },
    service::JobScheduler,
};
use indoc::formatdoc;
use tokio::{fs, process::Command};

use crate::infrastructure::{
    command::{MaybeSsh, Scp},
    service::job_scheduler::{LsfJob, LsfJobs},
};

#[derive(DepInj)]
#[target(LsfClient)]
pub struct LsfClientState {
    base_path: String,
    include_env: String,
    queue: String,
}

impl LsfClientState {
    pub fn new(base_path: String, include_env: String, queue: String) -> Self {
        Self {
            base_path,
            include_env,
            queue,
        }
    }
}

#[async_trait::async_trait]
impl<Deps> JobScheduler for LsfClient<Deps>
where
    Deps: AsRef<LsfClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_jobs(&self) -> anyhow::Result<Vec<Job>> {
        self.get_lsf_jobs().await
    }

    async fn get_job(&self, id: &str) -> anyhow::Result<Job> {
        self.get_lsf_job(id).await
    }

    async fn submit_job_script(&self, script_info: ScriptInfo) -> anyhow::Result<String> {
        let mut path = PathBuf::new();
        path.push(self.base_path.as_str());
        if !path.exists() {
            fs::create_dir_all(path.as_path()).await?;
        }
        path.push(script_info.path.as_str());
        let base_path = match self.prj_ref().scp() {
            Some((_, ssh)) => format!("{}/{}", ssh.home_dir, ssh.save_dir),

            None => self.base_path.to_owned(),
        };
        let script = self.gen_script(&base_path, &self.include_env, script_info.clone());
        fs::write(path, script).await?;
        self.submit_job(script_info.path.as_str()).await
    }

    async fn submit_job(&self, script_path: &str) -> anyhow::Result<String> {
        let out = 'block: {
            let path = PathBuf::from_iter([&self.base_path, script_path]);

            let Some((mut scp, ssh)) = self.prj_ref().scp() else {
                let out = Command::new(&path).current_dir(path.parent().unwrap()).output().await?;
                if !out.status.success() {
                    anyhow::bail!(
                        "Exit Status not 0 for submit_job. real: {}, stderr: {}",
                        out.status,
                        String::from_utf8(out.stderr)?
                    )
                }
                break 'block out;
            };

            let remote_path = PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, script_path]);
            let out = self
                .prj_ref()
                .command("mkdir")
                .arg("-p")
                .arg(remote_path.parent().unwrap())
                .output()
                .await;
            match out {
                Ok(out) => {
                    if !out.status.success() {
                        tracing::error!(
                            "Unable to create directory {} on for pbs script.",
                            remote_path.parent().unwrap().to_string_lossy(),
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("{e}");
                }
            }
            let _ = scp.local_path(&path).remote_path(&remote_path).output().await?;
            let out = self
                .prj_ref()
                .command("cd")
                .arg(remote_path.parent().unwrap())
                .arg(";")
                .args(["chmod", "+x"])
                .arg(&remote_path)
                .arg(";")
                .arg(remote_path)
                .output()
                .await?;
            if !out.status.success() {
                anyhow::bail!(
                    "Exit Status not 0 for submit_job. real: {}, stderr: {}",
                    out.status,
                    String::from_utf8(out.stderr)?
                )
            }
            out
        };

        Ok(LsfJob::parse_job_id(
            String::from_utf8_lossy(&out.stdout).as_ref(),
        )?)
    }

    async fn delete_job(&self, _job_id: &str) -> anyhow::Result<()> {
        anyhow::bail!("unimplemented!")
    }

    async fn pause_job(&self, _job_id: &str) -> anyhow::Result<()> {
        anyhow::bail!("unimplemented!")
    }

    async fn continue_job(&self, _job_id: &str) -> anyhow::Result<()> {
        anyhow::bail!("unimplemented!")
    }
}

impl<Deps> LsfClient<Deps>
where
    Deps: AsRef<LsfClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_lsf_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let out = self.prj_ref().command("bjobs").arg("-a").output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit status not 0 for get_pbs_jobs. real: {}", out.status)
        }
        let result = LsfJobs::new(&out.stdout)?;

        Ok(result
            .jobs
            .into_iter()
            .map(|item| Job {
                id: Arc::from(item.id),
                name: item.job_name,
                owner: item.user,
                state: match item.state.as_str() {
                    "EXIT" => JobState::Failed,
                    "DONE" => JobState::Completed,
                    "RUN|EXITING" => JobState::Running,
                    _ => JobState::Unknown,
                },
                ..Default::default()
            })
            .collect::<Vec<_>>())
    }

    async fn get_lsf_job(&self, id: &str) -> anyhow::Result<Job> {
        let out = self.prj_ref().command("bjobs").args(["-a", id]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit status not 0 for get_pbs_job. real: {}", out.status)
        }
        let result = LsfJobs::new(&out.stdout)?;
        let item = result.jobs.into_iter().next().context("Job not found")?;
        Ok(Job {
            id: Arc::from(item.id),
            name: item.job_name,
            owner: item.user,
            state: match item.state.as_str() {
                "EXIT" => JobState::Failed,
                "DONE" => JobState::Completed,
                "RUN|EXITING" => JobState::Running,
                _ => JobState::Unknown,
            },
            ..Default::default()
        })
    }
}

impl<Deps> LsfClient<Deps>
where
    Deps: AsRef<LsfClientState>,
{
    fn gen_script(&self, base_path: &str, include_env: &str, script_info: ScriptInfo) -> String {
        let header = "#!/bin/bash";
        let id = script_info.parent_id.clone();
        let env: Vec<String> = script_info
            .environments
            .iter()
            .map(|(k, v)| format!("export {k}={v}"))
            .collect();

        let env_string = env.join("\n");
        // let touch = format!("echo -n \"{}\" > $PBS_O_WORKDIR/.co.sig", script_info.id);
        let script = format!(
            "bsub -q {} -o {base_path}/{id}/STDOUT -host_stack 1024 -share_size 15000 -cgsp 64 {} {}",
            self.queue,
            script_info.name,
            script_info.arguments.join(" ")
        );
        let script = match script_info.std_in {
            Some(StdInKind::Text { text }) => {
                format!("{script} << EOF\n{text}\nEOF")
            }
            Some(StdInKind::File { path }) => {
                format!("{script} < {path}")
            }
            None => script,
        };

        formatdoc! {r#"
            {header}
            {env_string}
            {include_env}
            {script}
        "#}
    }
}
