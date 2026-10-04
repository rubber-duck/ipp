//! Generic asynchronous byte I/O, independent of assets, executors and wire formats.

use std::{future::Future, pin::Pin};

mod buffer_reader;
mod cancellation;
mod listing;
mod memory;
mod platform;
mod reader;
mod service;
mod stream;
mod stream_source;
mod upload;
mod writer;

pub use buffer_reader::{BufferIoReader, IoImmutableBacking, MappedIoReader};
pub use cancellation::{IoCancellation, IoCancellationWaiter, IoCancelledFuture};
pub use listing::{IoListing, IoListingBackend, IoNextFuture, MemoryIoListing};
pub use memory::MemoryIoSource;
pub use platform::{IoPlatformSend, IoPlatformSync};
pub use reader::{
    IoReadBackend, IoReadFuture, IoReadWindow, IoReader, IoReaderStorage, IoStorageId,
    IoWindowBackend,
};
pub use service::IoService;
pub use stream::{IoInputReservation, IoStreamInput, StreamIoReader};
pub use upload::{IoUploadAssembly, IoUploadError};
pub use writer::{
    IoFlushFuture, IoWriteBackend, IoWriteFuture, IoWriteJob, IoWriter, MemoryIoWriter,
};

/// Error from byte routing, acquisition, consumption or publication.
pub type IoError = String;

/// One owned asynchronous operation; allocation is per open, never per read.
#[cfg(not(target_arch = "wasm32"))]
pub type IoOperation<T> = Pin<Box<dyn Future<Output = Result<T, IoError>> + Send + 'static>>;

/// Owned browser-local asynchronous operation.
#[cfg(target_arch = "wasm32")]
pub type IoOperation<T> = Pin<Box<dyn Future<Output = Result<T, IoError>> + 'static>>;

/// Owned operation opening an independent reader.
pub type IoOpenReadFuture = IoOperation<Box<dyn IoReader>>;

/// Owned operation opening a writer.
pub type IoOpenWriteFuture = IoOperation<Box<dyn IoWriter>>;

/// Owned operation opening an incremental listing.
pub type IoListFuture = IoOperation<Box<dyn IoListing>>;

/// Transport prefetch quantum; consumer lookahead may require a larger window.
pub const STREAM_CAPACITY: usize = 64 << 10;

/// Bounds and immutable recovery policy for an input operation.
#[derive(Clone, Copy, Debug)]
pub struct IoReadOptions {
    /// Optional caller bound; assets have no total-input byte quota.
    pub max_bytes: Option<usize>,
    /// Reopening must reproduce previously accepted immutable content.
    pub recovery: bool,
}

/// Host-local source registration identity, never reused after replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoSourceRegistrationId(pub u64);

/// Host-owned source policy; returned operations capture owned requests immediately.
pub trait IoSource {
    /// Open an incremental listing, or fail if enumeration is unsupported.
    fn list(&mut self, identifier: &str) -> IoListFuture;

    /// Capture this source incarnation and open independent input asynchronously.
    fn open_read(&mut self, identifier: &str, options: IoReadOptions) -> IoOpenReadFuture;

    /// Query explicit destination capability without opening output.
    fn can_write(&self, _identifier: &str) -> bool {
        false
    }

    /// Open owned output with explicit flush, completion and cancellation.
    fn open_write(&mut self, _identifier: &str, _max_bytes: usize) -> IoOpenWriteFuture {
        Box::pin(std::future::ready(Err("Data source is read-only".into())))
    }
}

/// Host I/O request correlated with one exact live reader.
#[derive(Clone, Debug)]
pub struct IoReadRequest {
    /// Host-local identity, never reused by replacement registrations.
    pub id: u64,
    /// Complete original opaque identifier.
    pub identifier: String,
    /// Optional caller bound for this operation.
    pub max_bytes: Option<usize>,
    /// Require recovery of the same immutable content.
    pub recovery: bool,
}

fn bounded_error(mut error: String) -> String {
    if error.len() > 2048 {
        let mut end = 2048;
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        error.truncate(end);
    }
    error
}
