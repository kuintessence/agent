mod authorization;
mod retry;
mod timeout;

use std::sync::Arc;
use std::time::Duration;

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use typed_builder::TypedBuilder;

#[rustfmt::skip]
pub use self::{
    authorization::AuthMiddleware,
    retry::{RetryOnError, RetryStreamRequest},
    timeout::TimeoutMiddleware,
};

#[derive(TypedBuilder)]
pub struct MiddlewareMenu {
    #[builder(default, setter(strip_option))]
    retries: Option<u32>,
    auth: Arc<AuthMiddleware>,
    timeout: Duration,
}

impl MiddlewareMenu {
    pub fn make(self) -> ClientWithMiddleware {
        let Self {
            retries,
            auth,
            timeout,
        } = self;

        /*
         * Pay attention here.
         * The correct order to attach our middlewares is
         *  ```
         *  (Retry ->) Auth -> Timeout
         *  ```
         *  because middlewares run in the order they were attached.
         *
         *  When the sending request has stream body, don't attach Retry!
         *  Using [`RetryStreamRequest`].
         */

        let cb = ClientBuilder::new(reqwest::Client::new());
        let cb = if let Some(retries) = retries {
            let policy = ExponentialBackoff::builder().build_with_max_retries(retries);
            let mdw = RetryTransientMiddleware::new_with_policy_and_strategy(policy, RetryOnError);
            cb.with(mdw)
        } else {
            cb
        };

        cb.with_arc(auth.clone()).with(TimeoutMiddleware::new(timeout)).build()
    }
}
