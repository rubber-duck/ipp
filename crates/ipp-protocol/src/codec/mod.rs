mod animation;

mod lifecycle;

#[cfg(feature = "surfaces")]
mod surface;

#[cfg(feature = "gui")]
mod gui;

#[cfg(feature = "gui")]
mod semantics;

use crate::{MAX_MESSAGE_BYTES, Request, RequestBody, Response, ResponseBody, wire::*};
use ipp_core::components::schema::FieldValue as ResolvedValue;
use ipp_core::{
    Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, FieldValue,
    FieldWrite,
};

/// Explicit wire rejection, before any core mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// Fixed protocol version differs.
    VersionMismatch,
    /// Target registry/defaults/features/wire identity differs.
    SchemaMismatch,
    /// Request belongs to another connection.
    SessionMismatch,
    /// Incomplete, invalid, or trailing data.
    Malformed(&'static str),
    /// Tag identifies no implemented operation.
    Unsupported(u8),
    /// A complete message or nested collection exceeds its bound.
    Limit(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ProtocolError {}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}

pub(crate) struct Writer(pub(crate) Vec<u8>);

#[cfg(test)]
mod manifest_tests;

#[cfg(test)]
mod codec_tests;

mod decode;
pub use decode::{decode_request, decode_request_with_buffer};

mod encode;
pub use encode::{encode_response, encode_response_into};
