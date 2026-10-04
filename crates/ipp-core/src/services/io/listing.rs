//! Incremental listing handles and allocation-free entry futures.

use super::{IoError, IoPlatformSend};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// An owned incremental listing, independent of source registration mutation.
pub trait IoListing: IoListingBackend {
    /// Await the next entry; None explicitly completes the listing.
    fn next(&mut self) -> IoNextFuture<'_>;
}

impl<T: IoListingBackend> IoListing for T {
    fn next(&mut self) -> IoNextFuture<'_> {
        IoNextFuture {
            listing: self,
        }
    }
}

/// Provider readiness boundary for incremental enumeration.
pub trait IoListingBackend: IoPlatformSend {
    /// Yield one available entry without collecting the entire source.
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<String>, IoError>>;
}

/// Named next-entry future borrowing one listing.
pub struct IoNextFuture<'a> {
    listing: &'a mut dyn IoListingBackend,
}

impl Future for IoNextFuture<'_> {
    type Output = Result<Option<String>, IoError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().listing.poll_next(cx)
    }
}

/// Already-addressable name snapshot, yielded one entry at a time.
pub struct MemoryIoListing {
    entries: VecDeque<String>,
}

impl MemoryIoListing {
    /// Adopt a source-owned snapshot.
    pub fn new(entries: impl IntoIterator<Item = String>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
}

impl IoListingBackend for MemoryIoListing {
    fn poll_next(&mut self, _cx: &mut Context<'_>) -> Poll<Result<Option<String>, IoError>> {
        Poll::Ready(Ok(self.entries.pop_front()))
    }
}
