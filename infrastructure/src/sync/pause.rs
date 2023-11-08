use std::future::Future;
use std::pin::Pin;
use std::sync::atomic;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::task::AtomicWaker;

#[derive(Debug, Default, Clone)]
pub struct PauseToken(Arc<PauseFlag>);

#[derive(Debug, Default)]
struct PauseFlag {
    set: AtomicBool,
    waker: AtomicWaker,
}

#[pin_project::pin_project]
pub struct PausableFuture<'a, F> {
    #[pin]
    inner: F,
    flag: &'a PauseFlag,
}

impl PauseToken {
    pub fn pause(&self) {
        self.0.set.store(true, atomic::Ordering::Release);
    }

    pub fn resume(&self) {
        self.0.set.store(false, atomic::Ordering::Release);
        self.0.waker.wake();
    }

    pub fn attach<F>(&self, future: F) -> PausableFuture<'_, F>
    where
        F: Future,
    {
        PausableFuture {
            inner: future,
            flag: &self.0,
        }
    }
}

impl<'a, F> Future for PausableFuture<'a, F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.flag.set.load(atomic::Ordering::Acquire) {
            return self.project().inner.poll(cx);
        }

        self.flag.waker.register(cx.waker());

        // Need to check condition **after** `register` to avoid a race
        // condition that would result in lost notifications.
        if !self.flag.set.load(atomic::Ordering::Acquire) {
            self.project().inner.poll(cx)
        } else {
            Poll::Pending
        }
    }
}
