//! One physical-connection account, retained through actual transport completion.

use ipp_core::services::reliable_output::{OutputLimits, ReliableOutputAccount};

pub(crate) mod outbox;
pub(crate) mod progress;
mod reservation;

pub use reservation::{PreparedOutputCopy, ReliableResponse, ResponseLease};
pub(crate) use reservation::{ReplyReservation, SharedReplyReservation};

/// Bytes charged for every retained record in addition to its payload and encoding.
///
/// It covers the Host bookkeeping a record keeps until physical completion: its linked outbox
/// node, the shared reservation cell, correlation map entries for replies, the transport's
/// completion record (WASM delivery node or native in-flight lease) and allocator headers.
/// `record_overhead_covers_retained_bookkeeping` measures those structures on the build target
/// and requires at least twice their size: about 400 bytes on 64-bit targets, less on wasm32.
/// The margin covers allocator size classes and partially filled map nodes. Because every record
/// pays it, the byte budget alone bounds how many records a connection can retain: at most
/// `MAX_OUTPUT_BYTES / RESPONSE_METADATA_BYTES` (8192) even for empty payloads.
pub(crate) const RESPONSE_METADATA_BYTES: usize = 1024;

/// Bookkeeping for a transport's second copy of a record, such as the worker-side buffer.
const COPY_METADATA_BYTES: usize = 1024;

/// Reliable output one physical connection may retain until physical completion: payloads,
/// peak encodings, transport copies and per-record bookkeeping of replies, events, notices
/// and frame progress together. Eight maximum-size messages hold the reply reserve, a native
/// socket's two in-flight maximum-size buffers and a burst from many Worlds sharing the
/// connection, while bounding memory for a reader that stops draining. There is no separate
/// record-count limit.
pub(crate) const MAX_OUTPUT_BYTES: usize = 8 * ipp_protocol::MAX_MESSAGE_BYTES;

/// Part of [`MAX_OUTPUT_BYTES`] that only replies to admitted requests may use: one
/// maximum-size reply with its bookkeeping plus a simultaneous transport copy. Uncorrelated
/// output fails at the remaining share, so it can never leave an admitted request unanswerable.
pub(crate) const REPLY_RESERVE_BYTES: usize =
    2 * ipp_protocol::MAX_MESSAGE_BYTES + RESPONSE_METADATA_BYTES + COPY_METADATA_BYTES;

/// Bytes ordinary output may occupy before the reply reserve.
pub(crate) const ORDINARY_OUTPUT_BYTES: usize = MAX_OUTPUT_BYTES - REPLY_RESERVE_BYTES;

#[derive(Clone)]
pub(crate) struct SharedReplyBudget(pub(crate) ReliableOutputAccount);

impl Default for SharedReplyBudget {
    fn default() -> Self {
        Self(ReliableOutputAccount::new(OutputLimits {
            bytes: MAX_OUTPUT_BYTES,
            reply_reserve: REPLY_RESERVE_BYTES,
        }))
    }
}
