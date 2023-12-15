use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Status {
    #[serde(rename = "Jobs", default)]
    jobs: HashMap<String, Job>,
}

#[derive(Debug, Deserialize)]
pub struct Job {
    job_state: JobState,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub enum JobState {
    #[serde(rename = "Q")]
    Queued,
    #[serde(rename = "R")]
    Running,
    #[serde(other)]
    Other,
}

impl Status {
    pub const ARGS: &'static [&'static str] = &["-f", "-F", "json"];

    #[inline]
    pub fn new(s: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(s)
    }

    /// get the count of queued and running jobs separately: `(queueds, runnings)`
    pub fn qr_count(&self) -> (usize, usize) {
        self.jobs.values().fold((0, 0), |(mut queued, mut running), j| {
            match j.job_state {
                JobState::Queued => queued += 1,
                JobState::Running => running += 1,
                JobState::Other => (),
            }

            (queued, running)
        })
    }
}
