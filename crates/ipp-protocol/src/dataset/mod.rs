//! Connection-owned logical datasets, independent of World commands and asset sources.

mod observations;
mod payload;
mod requests;
mod responses;

pub use observations::{binding_observation, driver_observation};
pub use payload::{decode_update, encode_update};
pub use requests::{DatasetOperation, DatasetRequest, decode, encode_request};
pub use responses::{DatasetPage, DatasetResponse, DatasetRow, response};

/// Dataset request marker.
pub const REQUEST_MAGIC: &[u8; 4] = b"IPDS";
/// Dataset response marker.
pub const RESPONSE_MAGIC: &[u8; 4] = b"IPDR";
/// Maximum bytes in one transfer chunk.
pub const CHUNK_BYTES: usize = 64 * 1024;
/// Maximum complete logical update, before decoding.
pub const UPDATE_BYTES: usize = 1024 * 1024;
/// Maximum request frame including schema and transfer metadata.
pub const FRAME_BYTES: usize = CHUNK_BYTES + 128;
/// Maximum typed read response, including schema and row metadata.
pub const PAGE_BYTES: usize = 64 * 1024;
/// Maximum records in a read page.
pub const PAGE_ROWS: usize = 128;
/// Maximum raw columns in a source or row.
pub const COLUMNS: usize = 256;
/// Maximum source and column name length in UTF-8 bytes.
pub const NAME_BYTES: usize = 1024;
/// Maximum deltas in a logical update.
pub const DELTAS: usize = 4096;
/// Maximum simultaneously assembling updates per connection.
pub const TRANSFERS: usize = 4;
/// Maximum declared bytes of all assembling updates on one connection.
pub const STAGING_BYTES: usize = 2 * UPDATE_BYTES;
/// Maximum live producers on one connection.
pub const PRODUCERS: usize = 64;

#[cfg(test)]
mod codec_tests;
