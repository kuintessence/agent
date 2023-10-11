use std::time::Duration;

use anyhow::Context;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use task_local_extensions::Extensions;

/// The workaround for [`reqwest::ClientBuilder::timeout`] doesn't work sometimes.
/// See [`reqwest#issue:1279`] and [`hyper#issue:2312`].
///
/// It should be the **last** middleware of client.
///
/// [`reqwest#issue:1279`]: https://github.com/seanmonstar/reqwest/issues/1279
/// [`hyper#issue:2312`]: https://github.com/hyperium/hyper/issues/2312
pub struct TimeoutMiddleware {
    waiting: Duration,
}

impl TimeoutMiddleware {
    #[inline]
    pub fn new(waiting: Duration) -> Self {
        Self { waiting }
    }
}

#[async_trait::async_trait]
impl Middleware for TimeoutMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        tokio::time::timeout(self.waiting, next.run(req, extensions))
            .await
            .context("Request timeout")?
    }
}
