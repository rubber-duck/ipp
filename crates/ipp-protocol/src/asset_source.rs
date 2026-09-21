//! Session-fenced source delivery, separate from the World command codec.

use crate::codec::{ProtocolError, Reader, Writer};
use ipp_core::services::asset_management::{AssetSource, AssetTypeId};

/// Prefix routed to source delivery before World command decoding.
pub const REQUEST_MAGIC: &[u8; 4] = b"IPAS";
/// Source replies carry no simulation tick.
pub const RESPONSE_MAGIC: &[u8; 4] = b"IPAR";
/// Maximum payload per delivery chunk.
pub const CHUNK_BYTES: usize = 64 * 1024;
/// Independent framing budget, including source descriptors and headers.
pub const MAX_FRAME_BYTES: usize = CHUNK_BYTES + 128;

/// Provider ownership and transfer operations.
#[derive(Debug)]
pub enum SourceOperation<'a> {
    /// Announce an immutable source and its complete encoded length.
    Begin {
        /// Source identity owned by this producer session.
        source: AssetSource,
        /// Exact byte count required before publication.
        length: u64,
    },
    /// Deliver the next contiguous bytes without publishing partial content.
    Chunk {
        /// Begin request identity.
        transfer: u64,
        /// Expected accumulated byte count.
        offset: u64,
        /// Borrow valid only during the receive call.
        bytes: &'a [u8],
    },
    /// Publish complete bytes to the existing resource provider.
    Finish {
        /// Begin request identity.
        transfer: u64,
    },
    /// Discard incomplete provider staging.
    Cancel {
        /// Begin request identity.
        transfer: u64,
    },
    /// Release producer ownership while preserving resource consumers.
    Release {
        /// Previously registered source identity.
        source: AssetSource,
    },
}

/// A decoded request fenced to the attached World session.
pub struct SourceRequest<'a> {
    /// Nonzero source-plane request correlation.
    pub id: u64,
    /// Owned metadata and transient input bytes.
    pub operation: SourceOperation<'a>,
}

fn source(reader: &mut Reader<'_>) -> Result<AssetSource, ProtocolError> {
    Ok(AssetSource {
        kind: AssetTypeId(
            u16::try_from(reader.u32()?).map_err(|_| ProtocolError::Malformed("asset kind"))?,
        ),
        uri: reader.string()?,
        variant: reader.u32()?,
    })
}

/// Validate source framing without allocating command buffers or copying chunk payloads.
pub fn decode(bytes: &[u8], session: u64) -> Result<SourceRequest<'_>, ProtocolError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::Limit("asset source frame"));
    }
    let mut reader = Reader {
        bytes,
        at: 0,
    };
    if reader.take(4)? != REQUEST_MAGIC || reader.u64()? != session {
        return Err(ProtocolError::SessionMismatch);
    }
    let id = reader.u64()?;
    if id == 0 {
        return Err(ProtocolError::Malformed("asset source identity"));
    }
    let operation = match reader.u8()? {
        0 => SourceOperation::Begin {
            source: source(&mut reader)?,
            length: reader.u64()?,
        },
        1 => {
            let transfer = reader.u64()?;
            let offset = reader.u64()?;
            let length = reader.count(CHUNK_BYTES)?;
            SourceOperation::Chunk {
                transfer,
                offset,
                bytes: reader.take(length)?,
            }
        }
        2 => SourceOperation::Finish {
            transfer: reader.u64()?,
        },
        3 => SourceOperation::Cancel {
            transfer: reader.u64()?,
        },
        4 => SourceOperation::Release {
            source: source(&mut reader)?,
        },
        tag => return Err(ProtocolError::Unsupported(tag)),
    };
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("asset source trailing bytes"));
    }
    Ok(SourceRequest {
        id,
        operation,
    })
}

/// Encode provider acceptance or an explicit recoverable failure.
pub fn response(
    session: u64,
    id: u64,
    result: Result<(), String>,
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer(Vec::new());
    writer.raw(RESPONSE_MAGIC)?;
    writer.u64(session)?;
    writer.u64(id)?;
    match result {
        Ok(()) => writer.u8(0)?,
        Err(error) => {
            writer.u8(1)?;
            writer.string(&error[..error.floor_char_boundary(4096)])?;
        }
    }
    Ok(writer.0)
}

#[cfg(test)]
#[path = "asset_source_tests.rs"]
mod tests;
