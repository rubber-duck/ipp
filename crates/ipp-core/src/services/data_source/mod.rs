//! Generic bounded data I/O, independent of assets, executors and wire formats.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

mod memory;
mod reader;
mod writer;

#[cfg(feature = "zip-data-source")]
mod zip;

#[cfg(feature = "zip-data-source")]
pub use zip::ZipDataSource;

pub use memory::MemoryDataSource;
pub use reader::{DataReader, MemoryDataReader};
pub use writer::{DataWriteJob, DataWriter, MemoryDataWriter};

/// Maximum input retained by one asynchronous stream.
pub const STREAM_CAPACITY: usize = 64 << 10;

/// Bounds and immutable recovery policy for an input operation.
#[derive(Clone, Copy, Debug)]
pub struct DataReadOptions {
    /// Optional caller bound, used by persistence transfers; assets have no byte quota.
    pub max_bytes: Option<usize>,
    /// Reopening must reproduce previously accepted immutable content.
    pub recovery: bool,
}

/// Host-local identity of a source registration, never reused after replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataSourceRegistrationId(pub u64);

/// A source interprets its complete opaque identifiers and owns I/O policy.
pub trait DataSource {
    /// List available identifiers, or report that enumeration is unsupported.
    fn list(&mut self, identifier: &str) -> Result<Vec<String>, String>;

    /// Open bounded input. No borrowed identifier survives this call.
    fn open_read(
        &mut self,
        identifier: &str,
        options: DataReadOptions,
    ) -> Result<Box<dyn DataReader>, String>;

    /// Whether this destination can be written by the current caller.
    fn can_write(&self, _identifier: &str) -> bool {
        false
    }

    /// Open bounded output with explicit completion and cancellation.
    fn open_write(
        &mut self,
        _identifier: &str,
        _max_bytes: usize,
    ) -> Result<Box<dyn DataWriter>, String> {
        Err("Data source is read-only".into())
    }
}

/// Host I/O request correlated with exactly one live reader.
#[derive(Clone, Debug)]
pub struct DataReadRequest {
    /// Host-local request identity, never reused by replacement registrations.
    pub id: u64,
    /// Complete original identifier.
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

mod service;
pub use service::DataSourceManagementService;
