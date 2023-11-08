use std::future::Future;

use chrono::Utc;
use reqwest::{Response, StatusCode};
use reqwest_middleware::ClientWithMiddleware;
use reqwest_retry::{
    default_on_request_failure, default_on_request_success, RetryPolicy, Retryable,
    RetryableStrategy,
};
use retry_policies::RetryDecision;

pub struct RetryOnError;

impl RetryableStrategy for RetryOnError {
    fn handle(&self, res: &reqwest_middleware::Result<Response>) -> Option<Retryable> {
        match res {
            // [`TimeoutMiddleware`] returns `Error::Middleware` when timeout,
            // [`reqwest_retry::DefaultRetryableStrategy`] won't retry in such case.
            Err(reqwest_middleware::Error::Middleware(_)) => Some(Retryable::Transient),
            Err(e @ reqwest_middleware::Error::Reqwest(_)) => default_on_request_failure(e),
            Ok(resp) => {
                // We need to retry when token expired.
                if resp.status() == StatusCode::UNAUTHORIZED {
                    Some(Retryable::Transient)
                } else {
                    default_on_request_success(resp)
                }
            }
        }
    }
}

/// **(Not a middleware)**
/// [`reqwest_retry::RetryTransientMiddleware`] clones the sending request internally.
/// But cloning [`Request`] with **stream** body is forbidden. (See [`reqwest::Request::try_clone`])
/// So here we accept a closure where users send their request inside, then we parse the result.
///
/// Stream body comes from:
/// - [`RequestBuilder::multipart`]
/// - [`Body::wrap_stream`]
///
/// [`Request`]: reqwest::Request
/// [`RequestBuilder::multipart`]: reqwest::RequestBuilder::multipart
/// [`Body::wrap_stream`]: reqwest::Body::wrap_stream
pub struct RetryStreamClient<T, R> {
    base: ClientWithMiddleware,
    retry_policy: T,
    retryable_strategy: R,
}

impl<T, R> RetryStreamClient<T, R>
where
    T: RetryPolicy,
    R: RetryableStrategy,
{
    pub fn new(client: ClientWithMiddleware, retry_policy: T, retryable_strategy: R) -> Self {
        Self {
            base: client,
            retry_policy,
            retryable_strategy,
        }
    }

    /// This function will try to execute the request, if it fails
    /// with an error classified as transient it will call itself
    /// to retry the request.
    pub async fn execute<'a, F, Res>(&'a self, send: F) -> reqwest_middleware::Result<Response>
    where
        F: Fn(&'a ClientWithMiddleware) -> Res,
        Res: Future<Output = reqwest_middleware::Result<Response>>,
    {
        let mut n_past_retries = 0;
        loop {
            let result = send(&self.base).await;

            // We classify the response which will return None if not
            // errors were returned.
            break match self.retryable_strategy.handle(&result) {
                Some(Retryable::Transient) => {
                    // If the response failed and the error type was transient
                    // we can safely try to retry the request.
                    let retry_decision = self.retry_policy.should_retry(n_past_retries);
                    if let RetryDecision::Retry { execute_after } = retry_decision {
                        let duration = (execute_after - Utc::now())
                            .to_std()
                            .map_err(reqwest_middleware::Error::middleware)?;
                        // Sleep the requested amount before we try again.
                        tracing::warn!(
                            "Retry attempt #{}. Sleeping {:?} before the next attempt",
                            n_past_retries,
                            duration
                        );
                        tokio::time::sleep(duration).await;

                        n_past_retries += 1;
                        continue;
                    } else {
                        result
                    }
                }
                Some(_) | None => result,
            };
        }
    }
}
