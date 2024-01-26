use serde::*;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlurmJob {
    #[serde(rename = "JobID")]
    pub job_id: String,
    #[serde(rename = "JobName")]
    pub job_name: String,
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "ExitCode")]
    pub exit_code: String,
    #[serde(rename = "WorkDir")]
    pub work_dir: String,
    #[serde(rename = "CPUTimeRAW")]
    pub cpu_time: u64,
    #[serde(rename = "ElapsedRaw")]
    pub elapsed: u64,
    #[serde(rename = "NCPUS")]
    pub ncpus: u64,
    #[serde(rename = "AveRSS")]
    pub ave_mem: Option<u64>,
    #[serde(rename = "MaxRSS")]
    pub mem: Option<u64>,
    #[serde(rename = "Start")]
    pub start: String,
    #[serde(rename = "End")]
    pub end: String,
    #[serde(rename = "NNodes")]
    pub nnodes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn deserialize() {
        let x = indoc! {r#"
            JobID|JobName|User|State|ExitCode|WorkDir|CPUTimeRAW|ElapsedRaw|NCPUS|AveRSS|MaxRSS|NNodes|Start|End
            01|job_name|user|COMPLETED|0:0|pathto|10|10|10|||10|1999-01-01T00:00:01|2000-01-01T00:00:01
            "#
        };
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'|')
            .quoting(false)
            .from_reader(x.as_bytes());
        for record in reader.deserialize() {
            let _record: SlurmJob = record.unwrap();
        }
    }
}
