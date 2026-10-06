//! Allocation-free read futures and scoped immutable windows.

use super::{IoError, IoPlatformSend};
use std::{
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};

/// Host-local storage identity for deduplicating retained memory accounting.
/// This opaque token is not a transferable descriptor or client authority.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IoStorageId(pub(crate) usize);

impl IoStorageId {
    /// Identify existing backing without exposing an address or creating authority.
    pub fn of_backing<T: ?Sized>(backing: &std::sync::Arc<T>) -> Self {
        Self(std::sync::Arc::as_ptr(backing) as *const () as usize)
    }
}

/// Actual allocation retained by a reader, including borrowed and filling spans.
#[derive(Clone, Copy)]
pub struct IoReaderStorage {
    /// Equal identities refer to the same retained allocation.
    pub identity: IoStorageId,
    /// Retained owned heap allocation, not only the current readable range.
    pub bytes: usize,
    /// File-backed virtual address space, distinct from owned heap pressure.
    pub mapped_bytes: usize,
}

/// Dynamically dispatchable input with an independent logical cursor.
pub trait IoReader: IoReadBackend {
    /// Await a contiguous minimum, or a shorter explicitly final window.
    fn read(&mut self, minimum: NonZeroUsize) -> IoReadFuture<'_>;
}

impl<T: IoReadBackend> IoReader for T {
    fn read(&mut self, minimum: NonZeroUsize) -> IoReadFuture<'_> {
        IoReadFuture::new(self, minimum)
    }
}

/// Provider implementation boundary. Consumers use [`IoReader::read`].
/// Readiness never consumes input. Success guarantees that the next window
/// contains the requested minimum or is final. Pending work belongs to the reader.
pub trait IoReadBackend: IoPlatformSend {
    /// Prepare a window without exposing mutable storage to consumers.
    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
        minimum: NonZeroUsize,
    ) -> Poll<Result<(), IoError>>;

    /// Lend the range prepared by a successful readiness poll.
    fn window(&mut self) -> IoReadWindow<'_>;

    /// Current retained backing. None means the provider cannot account its storage;
    /// callers must preserve that distinction instead of recording a known zero.
    /// Identity may change between polls. Storage changes outside poll_ready must
    /// wake its registered read waiter so acquisition pressure remains observable.
    fn retained_storage(&self) -> Option<IoReaderStorage> {
        None
    }

    /// Observe retained storage independently of read readiness. Register before
    /// sampling retained_storage to close the allocation/snapshot wake race.
    /// Providers changing capacity or identity outside poll_ready must retain this
    /// observer and wake it on every change, even without a pending read. The
    /// default is valid only for immutable storage or changes inside poll_ready.
    /// Registration never prepares a window or consumes input. Drop must detach
    /// the observer if provider state survives the reader.
    fn register_storage_waker(&mut self, _waker: &std::task::Waker) {}

    /// Platform correlation for this exact acquisition, if any.
    fn request_id(&self) -> Option<u64> {
        None
    }

    /// Cancels platform acquisition when the registration is revoked.
    fn cancellation(&self) -> Option<super::IoCancellation> {
        None
    }
}

/// Provider-owned cursor and backing protected by an exclusive window borrow.
pub trait IoWindowBackend: IoPlatformSend {
    /// Currently lent immutable range.
    fn bytes(&self) -> &[u8];

    /// Whether no bytes follow this range.
    fn is_final(&self) -> bool;

    /// Advance an already checked prefix; failure must leave the cursor unchanged.
    fn consume(&mut self, count: usize) -> Result<(), IoError>;

    /// Release backing without consuming input and resume filling if needed.
    fn release(&mut self) {}
}

/// Named read future with no allocation at dynamic reader boundaries.
pub struct IoReadFuture<'a> {
    backend: Option<&'a mut dyn IoReadBackend>,
    minimum: NonZeroUsize,
}

impl<'a> IoReadFuture<'a> {
    /// Construct a future over provider-owned readiness state.
    pub fn new(backend: &'a mut dyn IoReadBackend, minimum: NonZeroUsize) -> Self {
        Self {
            backend: Some(backend),
            minimum,
        }
    }
}

impl<'a> Future for IoReadFuture<'a> {
    type Output = Result<IoReadWindow<'a>, IoError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this
            .backend
            .as_mut()
            .expect("completed IO read future")
            .poll_ready(cx, this.minimum)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                this.backend.take();
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(())) => {
                let window = this.backend.take().expect("ready IO reader").window();
                if window.bytes().len() < this.minimum.get() && !window.is_final() {
                    return Poll::Ready(Err(
                        "IO provider returned less than the requested minimum".into(),
                    ));
                }
                Poll::Ready(Ok(window))
            }
        }
    }
}

/// Scoped immutable range. Only explicit prefix consumption advances input.
pub struct IoReadWindow<'a> {
    backend: &'a mut dyn IoWindowBackend,
    counter: Option<&'a AtomicU64>,
}

impl<'a> IoReadWindow<'a> {
    /// Lend backing; this exclusive borrow prevents mutation, unmapping and reuse.
    pub fn new(backend: &'a mut dyn IoWindowBackend) -> Self {
        Self {
            backend,
            counter: None,
        }
    }

    pub(crate) fn count_into(mut self, counter: &'a AtomicU64) -> Self {
        self.counter = Some(counter);
        self
    }

    /// Borrow bytes until this window is consumed or released.
    pub fn bytes(&self) -> &[u8] {
        self.backend.bytes()
    }

    /// No bytes follow this range; an empty final window is cursor EOF.
    pub fn is_final(&self) -> bool {
        self.backend.is_final()
    }

    /// Consume exactly this prefix. Invalid counts fail without advancement.
    pub fn consume(self, count: usize) -> Result<(), IoError> {
        if count > self.bytes().len() {
            return Err("IO consumption exceeds the lent window".into());
        }
        self.backend.consume(count)?;
        if let Some(counter) = self.counter {
            counter.fetch_add(count as u64, Ordering::Relaxed);
        }
        Ok(())
    }
}

impl Drop for IoReadWindow<'_> {
    fn drop(&mut self) {
        self.backend.release();
    }
}
