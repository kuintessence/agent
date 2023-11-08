use std::future::Future;
use std::ops::ControlFlow;
use std::time::Duration;

pub async fn new<T, F, Fut>(interval: Duration, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ControlFlow<T>>,
{
    let mut interval = tokio::time::interval(interval);

    loop {
        interval.tick().await;

        if let ControlFlow::Break(ret) = f().await {
            break ret;
        }
    }
}

pub async fn new_fn<T, F>(interval: Duration, mut f: F) -> T
where
    F: FnMut() -> ControlFlow<T>,
{
    let mut interval = tokio::time::interval(interval);

    loop {
        interval.tick().await;

        if let ControlFlow::Break(ret) = f() {
            break ret;
        }
    }
}
