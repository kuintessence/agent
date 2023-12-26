use std::io::BufRead;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug)]
pub struct LsfJobs {
    pub jobs: Vec<LsfJob>,
}

#[derive(Debug, Deserialize)]
pub struct LsfJob {
    #[serde(rename = "JOBID")]
    pub job_id: String,
    #[serde(rename = "STAT")]
    pub state: String,
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
}
