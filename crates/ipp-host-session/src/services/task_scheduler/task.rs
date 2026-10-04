use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// The owning handle or Host cancelled the task before its result was observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskCancelled;

impl std::fmt::Display for TaskCancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("task cancelled")
    }
}

impl std::error::Error for TaskCancelled {}

#[derive(Default)]
pub(super) struct Cancellation {
    cancelled: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Cancellation {
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    fn poll(&self, cx: &Context<'_>) -> bool {
        let mut waker = self.waker.lock().unwrap();
        if self.cancelled.load(Ordering::Acquire) {
            return true;
        }
        if waker
            .as_ref()
            .is_none_or(|current| !current.will_wake(cx.waker()))
        {
            *waker = Some(cx.waker().clone());
        }
        false
    }
}

pub(super) async fn cancellable<F: Future>(
    future: F,
    cancellation: Arc<Cancellation>,
) -> Result<F::Output, TaskCancelled> {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(move |cx| {
        if cancellation.poll(cx) {
            Poll::Ready(Err(TaskCancelled))
        } else {
            future.as_mut().poll(cx).map(Ok)
        }
    })
    .await
}

/// Awaitable task ownership; dropping the handle cancels its future.
#[must_use = "dropping a task handle cancels its future"]
pub struct TaskHandle<T> {
    pub(super) task: Option<async_task::Task<Result<T, TaskCancelled>>>,
    pub(super) cancellation: Arc<Cancellation>,
}

impl<T> TaskHandle<T> {
    pub(super) fn cancelled() -> Self {
        Self {
            task: None,
            cancellation: Arc::new(Cancellation::default()),
        }
    }

    /// Request cancellation and await release of the task's asynchronous state.
    /// A running blocking operation retains its own inputs until it finishes.
    pub async fn cancel(self) {
        self.cancellation.cancel();
        let _ = self.await;
    }

    /// Whether execution has reached its terminal state.
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(async_task::Task::is_finished)
    }
}

impl<T> Future for TaskHandle<T> {
    type Output = Result<T, TaskCancelled>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.task.as_mut() {
            Some(task) => Pin::new(task).poll(cx),
            None => Poll::Ready(Err(TaskCancelled)),
        }
    }
}
