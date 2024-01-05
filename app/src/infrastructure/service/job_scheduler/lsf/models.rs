use anyhow::Context;
use regex::Regex;
use serde::Deserialize;
use std::io::BufRead;

#[derive(Debug)]
pub struct LsfJobs {
    pub jobs: Vec<LsfJob>,
}

#[derive(Debug, Deserialize)]
pub struct LsfJob {
    #[serde(rename = "JOBID")]
    pub id: String,
    #[serde(rename = "STAT")]
    pub state: String,
    #[serde(rename = "JOB_NAME")]
    pub job_name: String,
    #[serde(rename = "FROM")]
    pub user: String,
}

impl LsfJob {
    #[inline]
    pub fn parse_job_id(s: &str) -> anyhow::Result<String> {
        let e_str = "Id parse error";
        Ok(s.strip_prefix("Job <")
            .context(e_str)?
            .split_once('>')
            .context(e_str)?
            .0
            .to_string())
    }
}

impl LsfJobs {
    #[inline]
    pub fn new(s: &[u8]) -> anyhow::Result<Self> {
        let mut lines = s.lines().collect::<Result<Vec<String>, std::io::Error>>()?;
        lines.remove(1);
        let s = lines.join("\n");
        let s = s.as_bytes();

        let re = Regex::new(r"\ +").unwrap();
        let mut t = re.replace_all(&String::from_utf8(s.to_vec())?, " ").to_string();
        let re = Regex::new(r"[A-z][a-z]{2} [0-3][0-9] [0-2][0-9]:[0-6][0-9]").unwrap();
        t = re.replace_all(&t, "xx").to_string();

        let mut reader = csv::ReaderBuilder::new().delimiter(b' ').from_reader(t.as_bytes());
        Ok(Self {
            jobs: reader
                .deserialize()
                .map(|record| {
                    let job: LsfJob = record?;
                    Ok(job)
                })
                .collect::<anyhow::Result<Vec<LsfJob>>>()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::service::job_scheduler::LsfJob;

    use super::LsfJobs;
    use indoc::indoc;

    #[test]
    fn deserialize() {
        let out = indoc! {"
            JOBID   STAT     USER       JOB_NAME        QUEUE             FROM         SUBMIT_TIME    START_TIME     NODENUM NODELIST
            --------------------------------------------------------------------------------------------------------------------------
            3402265 EXIT     suanwang   07bc9b07        q_share           sn01         Dec 26 14:50   Dec 26 14:50   -       -
            3402266 DONE     suanwang   07bc9b07        q_share           sn01         Dec 26 14:51   Dec 26 14:51   -       -
            3402270 RUN      suanwang   vasp_test       q_share           sn01         Dec 26 14:53   Dec 26 14:53   1       668"};

        LsfJobs::new(out.as_bytes()).unwrap();
    }

    #[test]
    fn parse_job_id() {
        let sub_std_out = "Job <3407845> has been submitted to queue <q_share>";
        let id = LsfJob::parse_job_id(sub_std_out).unwrap();
        assert_eq!("3407845", id);
    }
}
