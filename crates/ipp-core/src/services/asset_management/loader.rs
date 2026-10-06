//! Owned asynchronous decoding and executor-neutral scheduling at the resource loading boundary.

use super::{Asset, AssetLoader, IoReader};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Failure may preserve a complete CPU representation after graphics allocation fails.
pub struct AssetLoadFailure<T> {
    /// Observable loading failure; a retained representation does not imply readiness.
    pub error: String,
    /// Completely validated CPU data, with no partially usable graphics allocations.
    pub decoded: Option<T>,
}

impl<T> AssetLoadFailure<T> {
    /// Preserve usable CPU data independently of failed graphics readiness.
    pub fn with_decoded(error: String, decoded: T) -> Self {
        Self {
            error,
            decoded: Some(decoded),
        }
    }
}

impl<T> From<String> for AssetLoadFailure<T> {
    fn from(error: String) -> Self {
        Self {
            error,
            decoded: None,
        }
    }
}

type LoadFuture<T> = Pin<Box<dyn Future<Output = Result<T, AssetLoadFailure<T>>>>>;
type StartLoad<T> = Box<dyn FnOnce(Box<dyn IoReader>) -> LoadFuture<T>>;

/// One future per load; reads themselves allocate no futures or encoded staging.
///
/// The future is Host-local, allowing graphics loaders to capture their device.
/// Dropping this adapter cancels its reader and destroys private partial output.
pub struct AsyncAssetLoader<T: Asset> {
    start: Option<StartLoad<T>>,
    future: Option<LoadFuture<T>>,
    failed_data: Option<T>,
}

impl<T: Asset> AsyncAssetLoader<T> {
    /// Construct a decoder or graphics loader with explicit failure retention.
    pub fn new<F, Fut>(load: F) -> Self
    where
        F: FnOnce(Box<dyn IoReader>) -> Fut + 'static,
        Fut: Future<Output = Result<T, AssetLoadFailure<T>>> + 'static,
    {
        Self {
            start: Some(Box::new(move |reader| Box::pin(load(reader)))),
            future: None,
            failed_data: None,
        }
    }

    /// Construct a CPU decoder whose errors expose no partial representation.
    pub fn decode<F, Fut>(decode: F) -> Self
    where
        F: FnOnce(Box<dyn IoReader>) -> Fut + 'static,
        Fut: Future<Output = Result<T, String>> + 'static,
    {
        Self::new(move |reader| async move { decode(reader).await.map_err(Into::into) })
    }
}

impl<T: Asset> AssetLoader for AsyncAssetLoader<T> {
    type Data = T;

    fn start_load(&mut self, reader: Box<dyn IoReader>) -> Result<(), String> {
        let start = self.start.take().ok_or("Asset loading already started")?;
        self.future = Some(start(reader));
        Ok(())
    }

    fn take_failed_data(&mut self) -> Option<T> {
        self.failed_data.take()
    }

    fn poll_load(&mut self, cx: &mut Context<'_>) -> Poll<Result<T, String>> {
        let Some(future) = &mut self.future else {
            return Poll::Ready(Err("Asset loader is not running".into()));
        };
        let result = match future.as_mut().poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.future = None;
        Poll::Ready(result.map_err(|failure| {
            self.failed_data = failure.decoded;
            failure.error
        }))
    }
}

/// Owned load task. Dropping this lease cancels its future without releasing
/// buffers still owned by an outstanding platform operation.
pub trait AssetLoadTask {}

/// Host-local scheduling supplied before resource acquisition starts.
/// The Host executor supplies real readiness wakers and owns cancellation/draining.
pub trait AssetLoadScheduler {
    /// Schedule owned acquisition/decoding work; the task never borrows a World.
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>) -> Box<dyn AssetLoadTask>;
}
