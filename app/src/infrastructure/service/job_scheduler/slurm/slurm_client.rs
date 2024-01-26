use std::path::PathBuf;
use std::sync::Arc;

use dep_inj::DepInj;
use domain::{
    model::{
        entity::{
            job::{JobResources, JobState},
            task::execute_usecase::StdInKind,
            Job,
        },
        vo::job::ScriptInfo,
    },
    service::JobScheduler,
};
use indoc::formatdoc;
use tokio::process::Command;

use super::SlurmJob;
use crate::infrastructure::command::{MaybeSsh, Scp};

#[derive(DepInj)]
#[target(SlurmClient)]
pub struct SlurmClientState {
    base_path: String,
    include_env: String,
    mpi: bool,
}

impl SlurmClientState {
    pub fn new(base_path: String, include_env: String, mpi: bool) -> Self {
        Self {
            base_path,
            include_env,
            mpi,
        }
    }
}

#[async_trait::async_trait]
impl<Deps> JobScheduler for SlurmClient<Deps>
where
    Deps: AsRef<SlurmClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_job(&self, id: &str) -> anyhow::Result<Job> {
        tracing::debug!("getting job id: {id}");
        let out = self.prj_ref().command("sacct")
            .args([
                "-PXo",
                "JobID,JobName,User,State,ExitCode,WorkDir,CPUTimeRaw,ElapsedRaw,NCPUS,AveRSS,MaxRSS,NNodes,Start,End",
                "-j",
                id,
            ])
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for get_job. real: {}", out.status)
        }
        let stdout = out.stdout.iter().cloned().filter(|c| *c != b'\'').collect::<Vec<_>>();
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(b'|')
            .quoting(false)
            .from_reader(stdout.as_slice());
        let mut jobs = Vec::<Job>::new();
        for record in csv_reader.deserialize() {
            let record: SlurmJob = record?;
            jobs.push(Job {
                id: Arc::from(record.job_id),
                name: record.job_name,
                owner: record.user,
                state: match record.state.as_str() {
                    "BOOT_FAIL" | "FAILED" | "NODE_FAIL" | "OUT_OF_MEMORY" | "TIMEOUT"
                    | "DEADLINE" => JobState::Failed,
                    "CANCELLED" => JobState::Suspended,
                    "COMPLETED" => JobState::Completed,
                    "PENDING" => JobState::Queuing,
                    "COMPLETING" => JobState::Completing,
                    "RUNNING" => JobState::Running,
                    _ => JobState::Unknown,
                },
                exit_status_code: record.exit_code.split(':').next().unwrap_or("0").parse()?,
                error_output: tokio::fs::read_to_string(format!("{}/STDERR", record.work_dir))
                    .await
                    .unwrap_or_default(),
                resource_used: JobResources {
                    cpu: record.ncpus,
                    avg_memory: record.ave_mem.unwrap_or(0),
                    max_memory: record.mem.unwrap_or(0),
                    storage: 0,
                    wall_time: record.elapsed,
                    cpu_time: record.cpu_time,
                    start_time: parse_time(&record.start),
                    end_time: parse_time(&record.end),
                    node: record.nnodes,
                },
            })
        }
        match jobs.first().cloned() {
            Some(x) => Ok(x),
            None => anyhow::bail!("No such id"),
        }
    }

    async fn get_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let out = self
            .prj_ref()
            .command("sacct")
            .args(["-PXo", "JobID,JobName,User,State,ExitCode,WorkDir"])
            .output()
            .await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for get_jobs. real: {}", out.status)
        }
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(b'|')
            .quoting(false)
            .from_reader(out.stdout.as_slice());
        let mut jobs = Vec::<Job>::new();
        for record in csv_reader.deserialize() {
            let record: SlurmJob = record?;
            jobs.push(Job {
                id: Arc::from(record.job_id),
                name: record.job_name,
                owner: record.user,
                state: match record.state.as_str() {
                    "BOOT_FAIL" | "FAILED" | "NODE_FAIL" | "OUT_OF_MEMORY" | "TIMEOUT"
                    | "DEADLINE" => JobState::Failed,
                    "CANCELLED" => JobState::Suspended,
                    "COMPLETED" => JobState::Completed,
                    "PENDING" => JobState::Queuing,
                    "COMPLETING" => JobState::Completing,
                    "RUNNING" => JobState::Running,
                    _ => JobState::Unknown,
                },
                exit_status_code: record.exit_code.split(':').next().unwrap_or("0").parse()?,
                error_output: tokio::fs::read_to_string(format!("{}/STDERR", record.work_dir))
                    .await
                    .unwrap_or_default(),
                resource_used: JobResources {
                    cpu: record.ncpus,
                    avg_memory: record.ave_mem.unwrap_or(0),
                    max_memory: record.mem.unwrap_or(0),
                    storage: 0,
                    wall_time: record.elapsed,
                    cpu_time: record.cpu_time,
                    start_time: parse_time(&record.start),
                    end_time: parse_time(&record.end),
                    node: record.nnodes,
                },
            })
        }
        Ok(jobs)
    }

    async fn submit_job(&self, script_path: &str) -> anyhow::Result<String> {
        let out = {
            let mut path = PathBuf::new();
            path.push(self.base_path.as_str());
            path.push(script_path);

            if let Some((mut scp, ssh)) = self.prj_ref().scp() {
                let remote_path = PathBuf::from_iter([&ssh.home_dir, &ssh.save_dir, script_path]);
                let out = self
                    .prj_ref()
                    .command("mkdir")
                    .arg("-p")
                    .arg(remote_path.parent().unwrap().to_string_lossy().as_ref())
                    .output()
                    .await;
                match out {
                    Ok(out) => {
                        if !out.status.success() {
                            tracing::error!(
                                "Unable to create directory {} on for slurm script.",
                                remote_path.parent().unwrap().to_string_lossy(),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("{e}");
                    }
                }
                let _ = scp.local_path(&path).remote_path(&remote_path).output().await?;
                // let sinfo_out_bytes =
                //     self.prj_ref().command("sinfo").arg("-h").output().await?.stdout;
                // let sinfo_out = String::from_utf8(sinfo_out_bytes)?;
                // let partition = sinfo_out
                //     .lines()
                //     .next()
                //     .with_context(|| {
                //         format!("Unable to get sinfo from sinfo -h. stdout: {sinfo_out}")
                //     })?
                //     .split_whitespace()
                //     .next()
                //     .with_context(|| {
                //         format!("Unable to get partition from sinfo -h. stdout: {sinfo_out}")
                //     })?
                //     .replace('*', "");
                let out = self
                    .prj_ref()
                    .command("cd")
                    .arg(remote_path.parent().unwrap())
                    .arg(";")
                    .arg("sbatch")
                    // .arg(format!("--partition={partition}"))
                    .arg(remote_path)
                    .output()
                    .await?;
                if !out.status.success() {
                    anyhow::bail!(
                        "Exit Status not 0 for ssh submit_job. real: {}, err: {}",
                        out.status,
                        String::from_utf8(out.stderr)?
                    )
                }
                out
            } else {
                // let sinfo_out_bytes = Command::new("sinfo").arg("-h").output().await?.stdout;
                // let sinfo_out = String::from_utf8(sinfo_out_bytes)?;
                // let partition = sinfo_out
                //     .lines()
                //     .next()
                //     .with_context(|| {
                //         format!("Unable to get sinfo from sinfo -h. stdout: {sinfo_out}")
                //     })?
                //     .split_whitespace()
                //     .next()
                //     .with_context(|| {
                //         format!("Unable to get partition from sinfo -h. stdout: {sinfo_out}")
                //     })?;
                let out = Command::new("sbatch")
                    // .arg(format!("--partition={partition}"))
                    .arg(&path)
                    .current_dir(path.parent().unwrap())
                    .output()
                    .await?;
                if !out.status.success() {
                    anyhow::bail!(
                        "Exit Status not 0 for submit_job. real: {}, err: {}",
                        out.status,
                        String::from_utf8(out.stderr)?
                    )
                }
                out
            }
        };
        Ok(String::from_utf8_lossy(&out.stdout)
            .replace("Submitted batch job ", "")
            .trim()
            .to_string())
    }

    async fn submit_job_script(&self, script_info: ScriptInfo) -> anyhow::Result<String> {
        let mut path = PathBuf::new();
        path.push(self.base_path.as_str());
        if !path.exists() {
            tokio::fs::create_dir_all(path.as_path()).await?;
        }
        path.push(script_info.path.as_str());
        tokio::fs::write(
            path,
            self.gen_script(&self.base_path, &self.include_env, script_info.clone()),
        )
        .await?;
        self.submit_job(script_info.path.as_str()).await
    }

    async fn delete_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("scancel").arg(job_id).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for delete_job. real: {}", out.status)
        }
        Ok(())
    }

    async fn pause_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("scontrol").args(["suspend", job_id]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for pause_job. real: {}", out.status)
        }
        Ok(())
    }

    async fn continue_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("scontrol").args(["resume", job_id]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for continue_job. real: {}", out.status)
        }
        Ok(())
    }
}

impl<Deps> SlurmClient<Deps>
where
    Deps: AsRef<SlurmClientState>,
{
    fn gen_script(&self, base_path: &str, include_env: &str, script_info: ScriptInfo) -> String {
        let header = "#!/bin/bash";
        let parent_id = script_info.parent_id.clone();
        let env: Vec<String> = script_info
            .environments
            .iter()
            .map(|(k, v)| format!("export {}={}", k, v))
            .collect();
        let env_string = env.join("\n");
        let touch = format!("echo -n \"{}\" > $SLURM_SUBMIT_DIR/.co.sig", parent_id);
        let script = format!("{} {}", script_info.name, script_info.arguments.join(" "));
        let script = match script_info.std_in {
            Some(StdInKind::Text { text }) => {
                format!("{script} << EOF\n{text}\nEOF")
            }
            Some(StdInKind::File { path }) => {
                format!("{script} < {path}")
            }
            None => script,
        };
        let script = match self.mpi {
            true => format!("mpirun -np $SLURM_NPROCS {script}"),
            false => script,
        };
        let load_software = script_info.load_software;
        let resource_header = match script_info.requirements {
            None => String::default(),
            Some(x) => {
                let mut header = String::default();
                let mut cores = 1usize;
                header += match x.node_count {
                    Some(x) => {
                        if x <= 0 {
                            "#SBATCH --nodes=1\n".to_string()
                        } else {
                            cores = x as usize;
                            format!("#SBATCH --nodes={x}\n")
                        }
                    }
                    None => "#SBATCH --nodes=1\n".to_string(),
                }
                .as_str();
                header += match x.cpu_cores {
                    Some(x) => {
                        cores *= x;
                        format!("#SBATCH --ntasks-per-node={x}\n")
                    }
                    None => "#SBATCH --ntasks-per-node={x}\n".to_string(),
                }
                .as_str();
                header += match x.max_wall_time {
                    Some(x) => format!("#SBATCH --time={}\n", x),
                    None => String::default(),
                }
                .as_str();
                header += match x.max_cpu_time {
                    Some(x) => format!("#SBATCH --time={}\n", x / cores),
                    None => String::default(),
                }
                .as_str();
                header
            }
        };
        // mpirun -np $SLURM_NPROCS {script}
        formatdoc! {r#"
            {header}
            #SBATCH --output={base_path}/{parent_id}/STDOUT
            #SBATCH --error={base_path}/{parent_id}/STDERR
            cd $SLURM_SUBMIT_DIR
            {resource_header}
            {env_string}
            {include_env}
            {load_software}
            {script}
            ec=$?
            {touch}
            exit $ec
        "#}
    }
}

fn parse_time(time: &str) -> i64 {
    if time.eq("UNKNOWN") {
        return 0;
    }
    match chrono::DateTime::parse_from_rfc3339(time) {
        Ok(x) => x.timestamp(),
        Err(_) => 0,
    }
}
