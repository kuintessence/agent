#[async_trait::async_trait]
pub trait JobService {
    /// Refresh all jobs
    async fn refresh_all(&self);
}
