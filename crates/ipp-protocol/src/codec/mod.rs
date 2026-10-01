mod animation;

mod lifecycle;

#[cfg(test)]
mod reference_tests;

use crate::{MAX_MESSAGE_BYTES, Request, RequestBody, Response, ResponseBody, wire::*};
use ipp_core::components::schema::FieldValue as ResolvedValue;
use ipp_core::{
    BatchOutcome, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, FieldValue,
    FieldWrite,
};

/// Explicit wire rejection, before any core mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    /// Request belongs to another connection.
    SessionMismatch,
    /// A transported World or output token no longer names its exact live lifetime.
    InvalidReference,
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

pub(crate) struct Writer(pub(crate) Vec<u8>, Option<usize>);

impl Writer {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes, None)
    }

    pub(crate) fn measuring() -> Self {
        Self(Vec::new(), Some(0))
    }

    pub(crate) fn len(&self) -> usize {
        self.1.unwrap_or(self.0.len())
    }

    pub(crate) fn framed(
        &mut self,
        write: impl Fn(&mut Self) -> Result<(), ProtocolError>,
    ) -> Result<(), ProtocolError> {
        let mut measurement = Self::measuring();
        write(&mut measurement)?;
        self.count(measurement.len(), MAX_MESSAGE_BYTES)?;
        write(self)
    }
}

#[cfg(test)]
mod manifest_tests;

#[cfg(test)]
mod codec_tests;

mod decode;
pub use decode::{
    RejectedBatchPage, RequestDecodeError, decode_request, decode_request_with_buffer,
    decode_world_request, is_batch_page,
};

mod encode;
pub use encode::{encode_response, encode_response_into, encoded_response_size};
