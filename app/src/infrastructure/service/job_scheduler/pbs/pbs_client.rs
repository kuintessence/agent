use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use chrono::{Local, NaiveDateTime};
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
use tokio::{fs, process::Command};
use walkdir::WalkDir;

use super::PBSJobs;
use crate::infrastructure::command::{MaybeSsh, Scp};

#[derive(DepInj)]
#[target(PbsClient)]
pub struct PBSClientState {
    base_path: String,
    include_env: String,
}

impl PBSClientState {
    pub fn new(base_path: String, include_env: String) -> Self {
        Self {
            base_path,
            include_env,
        }
    }
}

#[async_trait::async_trait]
impl<Deps> JobScheduler for PbsClient<Deps>
where
    Deps: AsRef<PBSClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_jobs(&self) -> anyhow::Result<Vec<Job>> {
        match self.get_pbs_jobs().await {
            Ok(x) => Ok(x),
            Err(_) => self.get_pbs_jobs_alternative().await,
        }
    }

    async fn get_job(&self, id: &str) -> anyhow::Result<Job> {
        match self.get_pbs_job(id).await {
            Ok(x) => Ok(x),
            Err(_) => self.get_pbs_job_alternative(id).await,
        }
    }

    async fn submit_job(&self, script_path: &str) -> anyhow::Result<String> {
        let out = 'block: {
            let path = PathBuf::from_iter([&self.base_path, script_path]);

            let Some((mut scp, ssh)) = self.prj_ref().scp() else {
                let out = Command::new("qsub")
                    .arg(&path)
                    .current_dir(path.parent().unwrap())
                    .output()
                    .await?;
                if !out.status.success() {
                    anyhow::bail!("Exit Status not 0 for submit_job. real: {}", out.status)
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
                .arg("qsub")
                .arg(remote_path)
                .output()
                .await?;
            if !out.status.success() {
                anyhow::bail!("Exit Status not 0 for submit_job. real: {}", out.status)
            }
            out
        };

        Ok(String::from_utf8_lossy(&out.stdout)
            .split('.')
            .next()
            .context("Id parse error")?
            .trim()
            .to_owned())
    }

    async fn submit_job_script(&self, script_info: ScriptInfo) -> anyhow::Result<String> {
        let mut path = PathBuf::new();
        path.push(self.base_path.as_str());
        if !path.exists() {
            fs::create_dir_all(path.as_path()).await?;
        }
        path.push(script_info.path.as_str());
        fs::write(
            path,
            Self::gen_script(&self.base_path, &self.include_env, script_info.clone()),
        )
        .await?;
        self.submit_job(script_info.path.as_str()).await
    }

    async fn delete_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("qdel").args(["-x", job_id]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for delete_job. real: {}", out.status)
        }
        Ok(())
    }

    async fn pause_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("qhold").arg(job_id).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for pause_job. real: {}", out.status)
        }
        Ok(())
    }

    async fn continue_job(&self, job_id: &str) -> anyhow::Result<()> {
        let out = self.prj_ref().command("qrls").arg(job_id).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for continue_job. real: {}", out.status)
        }
        Ok(())
    }
}

impl<Deps> PbsClient<Deps>
where
    Deps: AsRef<PBSClientState> + MaybeSsh + Scp + Send + Sync,
{
    async fn get_pbs_jobs(&self) -> anyhow::Result<Vec<Job>> {
        let out = self.prj_ref().command("qstat").args(["-xfF", "json"]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for get_pbs_jobs. real: {}", out.status)
        }
        let result: PBSJobs = serde_json::from_slice(&out.stdout)?;
        let mut jobs = Vec::with_capacity(result.jobs.len());

        for (id, item) in result.jobs {
            let job = Job {
                id: Arc::from(id),
                name: item.job_name,
                owner: item.job_owner,
                state: match item.job_state.as_str() {
                    "R" => JobState::Running,
                    "E" => {
                        if item.exit_status != 0 || item.exit_status != 254 {
                            JobState::Failed
                        } else {
                            JobState::Completing
                        }
                    }
                    "F" => {
                        if item.exit_status != 0 || item.exit_status != 254 {
                            JobState::Failed
                        } else {
                            JobState::Completed
                        }
                    }
                    "S" => JobState::Suspended,
                    "U" => JobState::Suspended,
                    "Q" => JobState::Queuing,
                    "H" => JobState::Suspended,
                    _ => JobState::Unknown,
                },
                exit_status_code: item.exit_status,
                error_output: fs::read_to_string(
                    item.error_path.split_once(':').unwrap_or_default().1,
                )
                .await
                .unwrap_or_default(),
                resource_used: JobResources {
                    cpu: item.resources_used.ncpus as u64,
                    avg_memory: parse_memory(&item.resources_used.mem),
                    max_memory: parse_memory(&item.resources_used.mem),
                    storage: directory_size(
                        item.error_path
                            .split_once(':')
                            .unwrap_or_default()
                            .1
                            .replace("/STDERR", ""),
                    )
                    .await
                    .unwrap_or(0),
                    wall_time: parse_duration(&item.resources_used.walltime),
                    cpu_time: parse_duration(&item.resources_used.cput),
                    start_time: parse_time(&item.stime),
                    end_time: match item.job_state.as_str() {
                        "F" | "E" => parse_time(&item.mtime),
                        _ => 0,
                    },
                    node: item.resource_list.nodect as u64,
                },
            };

            jobs.push(job);
        }

        Ok(jobs)
    }

    async fn get_pbs_jobs_alternative(&self) -> anyhow::Result<Vec<Job>> {
        let out = self.prj_ref().command("qstat").arg("-xfw").output().await?;
        if !out.status.success() {
            anyhow::bail!(
                "Exit Status not 0 for get_pbs_jobs_alternative. real: {}",
                out.status
            )
        }
        let lines = String::from_utf8_lossy(out.stdout.as_slice());
        let lines = lines.split('\n');
        let mut results = Vec::<Job>::new();
        for line in lines {
            if line.starts_with("Job Id: ") {
                let temp = Job {
                    id: Arc::from(line.replacen("Job Id: ", "", 1)),
                    ..Job::default()
                };
                results.push(temp);
            } else {
                let temp = match results.last_mut() {
                    Some(x) => x,
                    None => continue,
                };
                let line = line.trim();
                if line.starts_with("job_state = ") {
                    let state = line.replace("job_state = ", "");
                    temp.state = match state.as_str() {
                        "R" => JobState::Running,
                        "E" => {
                            if temp.exit_status_code != 0 || temp.exit_status_code != 254 {
                                JobState::Failed
                            } else {
                                JobState::Completing
                            }
                        }
                        "F" => {
                            if temp.exit_status_code != 0 || temp.exit_status_code != 254 {
                                JobState::Failed
                            } else {
                                JobState::Completed
                            }
                        }
                        "S" => JobState::Suspended,
                        "U" => JobState::Suspended,
                        "Q" => JobState::Queuing,
                        "H" => JobState::Suspended,
                        _ => JobState::Unknown,
                    };
                } else if line.starts_with("Job_Name = ") {
                    let name = line.replace("Job_Name = ", "");
                    temp.name = name;
                } else if line.starts_with("Job_Owner = ") {
                    let owner = line.replace("Job_Owner = ", "");
                    temp.owner = owner;
                } else if line.starts_with("Exit_status = ") {
                    let exit_status = line.replace("Exit_status = ", "");
                    temp.exit_status_code = exit_status.parse().unwrap_or_default();
                } else if line.starts_with("resources_used.walltime = ") {
                    let value = line.replace("resources_used.walltime = ", "");
                    temp.resource_used.wall_time = parse_duration(&value);
                } else if line.starts_with("resources_used.cput = ") {
                    let value = line.replace("resources_used.cput = ", "");
                    temp.resource_used.cpu_time = parse_duration(&value);
                } else if line.starts_with("resources_used.ncpus = ") {
                    let value = line.replace("resources_used.ncpus = ", "");
                    temp.resource_used.cpu = value.parse().unwrap_or_default();
                } else if line.starts_with("resources_used.mem = ") {
                    let value = line.replace("resources_used.mem = ", "");
                    temp.resource_used.max_memory = parse_memory(&value);
                    temp.resource_used.avg_memory = parse_memory(&value);
                } else if line.starts_with("stime = ") {
                    let value = line.replace("stime = ", "");
                    temp.resource_used.start_time = parse_time(&value);
                } else if line.starts_with("mtime = ") {
                    if temp.state == JobState::Failed
                        || temp.state == JobState::Completed
                        || temp.state == JobState::Completing
                    {
                        let value = line.replace("mtime = ", "");
                        temp.resource_used.end_time = parse_time(&value);
                    }
                } else if line.starts_with("Resource_List.nodect = ") {
                    let value = line.replace("Resource_List.nodect = ", "");
                    temp.resource_used.node = value.parse().unwrap_or_default();
                } else if line.starts_with("Error_Path = ") {
                    let value = line.replace("Error_Path = ", "");
                    let value = value.split_once(':').unwrap_or_default().1;
                    temp.error_output = fs::read_to_string(value).await.unwrap_or_default();
                    temp.resource_used.storage = directory_size(value).await.unwrap_or(0);
                } else {
                    continue;
                }
            }
        }
        Ok(results)
    }

    async fn get_pbs_job(&self, id: &str) -> anyhow::Result<Job> {
        let out = self.prj_ref().command("qstat").args(["-xfF", "json", id]).output().await?;
        if !out.status.success() {
            anyhow::bail!("Exit Status not 0 for get_pbs_job. real: {}", out.status)
        }
        let result: PBSJobs = serde_json::from_slice(&out.stdout)?;
        let Some((id, item)) = result.jobs.into_iter().next() else {
            anyhow::bail!("Job not found");
        };

        Ok(Job {
            id: Arc::from(id),
            name: item.job_name,
            owner: item.job_owner,
            state: match item.job_state.as_str() {
                "R" => JobState::Running,
                "E" => {
                    if item.exit_status != 0 && item.exit_status != 254 {
                        JobState::Failed
                    } else {
                        JobState::Completing
                    }
                }
                "F" => {
                    if item.exit_status != 0 && item.exit_status != 254 {
                        JobState::Failed
                    } else {
                        JobState::Completed
                    }
                }
                "S" => JobState::Suspended,
                "U" => JobState::Suspended,
                "Q" => JobState::Queuing,
                "H" => JobState::Suspended,
                _ => JobState::Unknown,
            },
            exit_status_code: item.exit_status,
            error_output: fs::read_to_string(item.error_path.split_once(':').unwrap_or_default().1)
                .await
                .unwrap_or_default(),
            resource_used: JobResources {
                cpu: item.resources_used.ncpus as u64,
                avg_memory: parse_memory(&item.resources_used.mem),
                max_memory: parse_memory(&item.resources_used.mem),
                storage: directory_size(
                    item.error_path.split_once(':').unwrap_or_default().1.replace("/STDERR", ""),
                )
                .await
                .unwrap_or(0),
                wall_time: parse_duration(&item.resources_used.walltime),
                cpu_time: parse_duration(&item.resources_used.cput),
                start_time: parse_time(&item.stime),
                end_time: match item.job_state.as_str() {
                    "F" | "E" => parse_time(&item.mtime),
                    _ => 0,
                },
                node: item.resource_list.nodect as u64,
            },
        })
    }

    async fn get_pbs_job_alternative(&self, id: &str) -> anyhow::Result<Job> {
        let out = self.prj_ref().command("qstat").args(["-xfw", id]).output().await?;
        if !out.status.success() {
            anyhow::bail!(
                "Exit Status not 0 for get_pbs_job_alternative. real: {}",
                out.status
            )
        }
        let lines = String::from_utf8_lossy(&out.stdout);
        let lines = lines.split('\n');
        let mut results = Vec::<Job>::new();
        for line in lines {
            if line.starts_with("Job Id: ") {
                let temp = Job {
                    id: Arc::from(line.replacen("Job Id: ", "", 1)),
                    ..Job::default()
                };
                results.push(temp);
            } else {
                let temp = match results.last_mut() {
                    Some(x) => x,
                    None => continue,
                };
                let line = line.trim();
                if line.starts_with("job_state = ") {
                    let state = line.replace("job_state = ", "");
                    temp.state = match state.as_str() {
                        "R" => JobState::Running,
                        "E" => {
                            if temp.exit_status_code != 0 && temp.exit_status_code != 254 {
                                JobState::Failed
                            } else {
                                JobState::Completing
                            }
                        }
                        "F" => {
                            if temp.exit_status_code != 0 && temp.exit_status_code != 254 {
                                JobState::Failed
                            } else {
                                JobState::Completed
                            }
                        }
                        "S" => JobState::Suspended,
                        "U" => JobState::Suspended,
                        "Q" => JobState::Queuing,
                        "H" => JobState::Suspended,
                        _ => JobState::Unknown,
                    };
                } else if line.starts_with("Job_Name = ") {
                    let name = line.replace("Job_Name = ", "");
                    temp.name = name;
                } else if line.starts_with("Job_Owner = ") {
                    let owner = line.replace("Job_Owner = ", "");
                    temp.owner = owner;
                } else if line.starts_with("Exit_status = ") {
                    let exit_status = line.replace("Exit_status = ", "");
                    temp.exit_status_code = exit_status.parse().unwrap_or_default();
                } else if line.starts_with("resources_used.walltime = ") {
                    let value = line.replace("resources_used.walltime = ", "");
                    temp.resource_used.wall_time = parse_duration(&value);
                } else if line.starts_with("resources_used.cput = ") {
                    let value = line.replace("resources_used.cput = ", "");
                    temp.resource_used.cpu_time = parse_duration(&value);
                } else if line.starts_with("resources_used.ncpus = ") {
                    let value = line.replace("resources_used.ncpus = ", "");
                    temp.resource_used.cpu = value.parse().unwrap_or_default();
                } else if line.starts_with("resources_used.mem = ") {
                    let value = line.replace("resources_used.mem = ", "");
                    temp.resource_used.max_memory = parse_memory(&value);
                } else if line.starts_with("stime = ") {
                    let value = line.replace("stime = ", "");
                    temp.resource_used.start_time = parse_time(&value);
                } else if line.starts_with("mtime = ") {
                    if temp.state == JobState::Failed
                        || temp.state == JobState::Completed
                        || temp.state == JobState::Completing
                    {
                        let value = line.replace("mtime = ", "");
                        temp.resource_used.end_time = parse_time(&value);
                    }
                } else if line.starts_with("Resource_List.nodect = ") {
                    let value = line.replace("Resource_List.nodect = ", "");
                    temp.resource_used.node = value.parse().unwrap_or_default();
                } else if line.starts_with("Error_Path = ") {
                    let value = line.replace("Error_Path = ", "");
                    let value = value.split_once(':').unwrap_or_default().1;
                    temp.error_output = fs::read_to_string(value).await.unwrap_or_default();
                    temp.resource_used.storage = directory_size(value).await.unwrap_or(0);
                } else {
                    continue;
                }
            }
        }

        results.into_iter().next().context("Job not found")
    }
}

impl<Deps> PbsClient<Deps> {
    fn gen_script(base_path: &str, include_env: &str, script_info: ScriptInfo) -> String {
        let header = "#!/bin/bash";
        let id = script_info.parent_id.clone();
        let env: Vec<String> = script_info
            .environments
            .iter()
            .map(|(k, v)| format!("export {k}={v}"))
            .collect();
        let env_string = env.join("\n");
        let touch = format!("echo -n \"{}\" > $PBS_O_WORKDIR/.co.sig", script_info.id);
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
        let load_software = script_info.load_software.clone();
        let resource_header = match script_info.requirements {
            None => String::default(),
            Some(x) => {
                let mut header = String::default();
                header += match x.node_count {
                    Some(x) => {
                        if x <= 0 {
                            "#PBS -l ".to_string()
                        } else {
                            format!("#PBS -l nodes={x}:")
                        }
                    }
                    None => "#PBS -l nodes=1:".to_string(),
                }
                .as_str();
                header += match x.cpu_cores {
                    Some(x) => format!("ppn={x}\n"),
                    None => "ppn=1\n".to_string(),
                }
                .as_str();
                header += match x.max_wall_time {
                    Some(x) => format!("#PBS -l walltime={}\n", format_duration(x)),
                    None => String::default(),
                }
                .as_str();
                header += match x.max_cpu_time {
                    Some(x) => format!("#PBS -l cput={}\n", format_duration(x)),
                    None => String::default(),
                }
                .as_str();
                header
            }
        };
        formatdoc! {r#"
            {header}
            #PBS -o {base_path}/{id}/STDOUT
            #PBS -e {base_path}/{id}/STDERR
            {resource_header}
            cd $PBS_O_WORKDIR
            NP=`cat $PBS_NODEFILE | wc -l`
            {env_string}
            {include_env}
            {load_software}
            mpirun -np $NP {script}
            result = $?
            {touch}
            $(exit $result)
        "#}
    }
}

fn parse_time(time: &str) -> i64 {
    time.ne("UNKNOWN")
        .then(|| {
            NaiveDateTime::parse_from_str(time, "%a %b %d %T %Y")
                .ok()
                .map(|nt| nt.and_local_timezone(Local).unwrap().timestamp())
        })
        .flatten()
        .unwrap_or_default()
}

fn parse_duration(duration: &str) -> u64 {
    let times = duration.rsplit(':');
    let mut second = 0u64;
    for time in times.enumerate() {
        let i = time.0 as u32;
        let time: u64 = time.1.parse().unwrap_or(0);
        match i {
            0..=2 => {
                second += 60u64.pow(i) * time;
            }
            3 => {
                second += 86_400u64 * time;
            }
            _ => {}
        }
    }
    second
}

fn format_duration(duration: usize) -> String {
    let hours = duration / 3600;
    let minutes = duration % 3600 / 60;
    let seconds = duration % 3600 % 60;

    format!(
        "{}:{}:{}",
        format_args!("{hours:0>2}"),
        format_args!("{minutes:0>2}"),
        format_args!("{seconds:0>2}")
    )
}

fn parse_memory(memory: &str) -> u64 {
    let unit = memory.trim_start_matches(char::is_numeric);
    let size = memory.trim_end_matches(char::is_alphabetic).parse().unwrap_or(0u64);
    match unit {
        "b" => size,
        "kb" => size * 1024,
        "mb" => size * 1024 * 1024,
        "gb" => size * 1024 * 1024 * 1024,
        "tb" => size * 1024 * 1024 * 1024 * 1024,
        "pb" => size * 1024 * 1024 * 1024 * 1024 * 1024,
        _ => size * 1024,
    }
}

async fn directory_size(path: impl Into<PathBuf>) -> io::Result<u64> {
    let path = path.into();

    tokio::task::spawn_blocking(move || {
        let mut total_size = 0;

        for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                total_size += metadata.len();
            }
        }

        Ok(total_size)
    })
    .await
    .unwrap()
}
