use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use domain::service::JobService;
use infrastructure::sync::timer;

use crate::infrastructure::ioc::Container;

pub async fn refresh_jobs(job_sv: Arc<Container>, interval: Duration) {
    timer::new::<(), _, _>(interval, || async {
        job_sv.refresh_all().await;
        ControlFlow::Continue(())
    })
    .await;
}
