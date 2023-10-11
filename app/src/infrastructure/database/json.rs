use std::sync::Arc;

use domain::model::entity::*;
use tokio::sync::Mutex;

pub struct JsonDb {
    pub(in crate::infrastructure) tasks: Arc<Mutex<Vec<Task>>>,
    pub(in crate::infrastructure) sub_tasks: Arc<Mutex<Vec<SubTask>>>,
    pub(in crate::infrastructure) task_files: Arc<Mutex<Vec<File>>>,
    pub(in crate::infrastructure) save_dir: String,
}

impl JsonDb {
    pub async fn new(save_dir: &str) -> anyhow::Result<Self> {
        let mut tasks_path = std::path::PathBuf::new();
        let mut sub_tasks_path = std::path::PathBuf::new();
        let mut task_files_path = std::path::PathBuf::new();
        tasks_path.push(save_dir);
        tasks_path.push("./tasks.json");
        sub_tasks_path.push(save_dir);
        sub_tasks_path.push("./sub_tasks.json");
        task_files_path.push(save_dir);
        task_files_path.push("task_files.json");
        let tasks: Vec<Task> = match tasks_path.exists() && tasks_path.is_file() {
            true => match tokio::fs::read(tasks_path).await {
                Ok(x) => serde_json::from_slice(&x)?,
                Err(_) => vec![],
            },
            false => vec![],
        };
        let sub_tasks: Vec<SubTask> = match sub_tasks_path.exists() && sub_tasks_path.is_file() {
            true => match tokio::fs::read(sub_tasks_path).await {
                Ok(x) => serde_json::from_slice(&x)?,
                Err(_) => vec![],
            },
            false => vec![],
        };
        let task_files: Vec<File> = match task_files_path.exists() && task_files_path.is_file() {
            true => match tokio::fs::read(task_files_path).await {
                Ok(x) => serde_json::from_slice(&x)?,
                Err(_) => vec![],
            },
            false => vec![],
        };
        Ok(Self {
            task_files: Arc::new(Mutex::new(task_files)),
            tasks: Arc::new(Mutex::new(tasks)),
            sub_tasks: Arc::new(Mutex::new(sub_tasks)),
            save_dir: save_dir.to_string(),
        })
    }

    pub(in crate::infrastructure) async fn save_changed(&self) -> anyhow::Result<bool> {
        let mut tasks_path = std::path::PathBuf::new();
        let mut sub_tasks_path = std::path::PathBuf::new();
        let mut task_files_path = std::path::PathBuf::new();
        tasks_path.push(self.save_dir.as_str());
        tasks_path.push("./tasks.json");
        sub_tasks_path.push(self.save_dir.as_str());
        sub_tasks_path.push("./sub_tasks.json");
        task_files_path.push(self.save_dir.as_str());
        task_files_path.push("task_files.json");
        let tasks = self.tasks.lock().await;
        let sub_tasks = self.sub_tasks.lock().await;
        let task_files = self.task_files.lock().await;
        let tasks_json = serde_json::to_vec(&tasks.clone())?;
        let sub_tasks_json = serde_json::to_vec(&sub_tasks.clone())?;
        let task_files_json = serde_json::to_vec(&task_files.clone())?;
        tokio::fs::write(tasks_path, tasks_json).await?;
        tokio::fs::write(sub_tasks_path, sub_tasks_json).await?;
        tokio::fs::write(task_files_path, task_files_json).await?;
        Ok(true)
    }
}
