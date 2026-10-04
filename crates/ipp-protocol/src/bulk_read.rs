//! Schema-independent, connection-fenced sequential bulk reads.
//!
//! A read returns at most 64 KiB. Up to eight chunks may be outstanding before
//! cumulative acknowledgement. EOF is acknowledged explicitly; delivery alone
//! never releases a backing lease. All byte counts on this boundary are u64.

use crate::codec::{ProtocolError, Reader, Writer};

/// Bulk request marker, routed independently of the generated contract.
pub const REQUEST_MAGIC: &[u8; 4] = b"IPDR";
/// Bulk reply and revocation-notice marker.
pub const RESPONSE_MAGIC: &[u8; 4] = b"IPDS";
/// Maximum payload of one sequential chunk.
pub const CHUNK_BYTES: usize = 64 * 1024;
/// Maximum chunks issued before acknowledging a consumed prefix.
pub const PIPELINE_CHUNKS: usize = 8;
/// Complete chunk frame plus bounded failure/header room.
pub const FRAME_BYTES: usize = CHUNK_BYTES + 128;

/// Exact connection-incarnation authority to one output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkReadReference {
    /// Fresh physical connection identity.
    pub connection: u64,
    /// Host-assigned identity, never reused by this Host.
    pub read: u64,
}

/// Domain-neutral output read metadata; representation metadata travels beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkReadDescriptor {
    /// Exact connection-owned read authority.
    pub reference: BulkReadReference,
    /// None for a stream whose complete length is established only at EOF.
    pub length: Option<u64>,
}

/// Schema-independent output ownership and consumption operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkReadOperation {
    /// Request the next bounded ordered chunk.
    Read {
        /// Exact expected byte position.
        offset: u64,
    },
    /// Commit a cumulative consumed prefix; final EOF releases the lease.
    Acknowledge {
        /// Complete contiguous prefix consumed by the client.
        consumed: u64,
        /// The client consumed the final EOF marker as well as all bytes.
        eof: bool,
    },
    /// Abandon unread bytes without consuming them.
    Release,
}

/// A validated request fenced to the exact connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkReadRequest {
    /// Nonzero client correlation identity in the bulk plane.
    pub id: u64,
    /// Exact read authority.
    pub reference: BulkReadReference,
    /// Sequential consumption or explicit abandonment.
    pub operation: BulkReadOperation,
}

/// Bounded output or an explicit recoverable read failure.
pub enum BulkReadResponse<'a> {
    /// Next immutable bytes, with explicit final EOF.
    Chunk {
        /// Echoed exact expected byte position.
        offset: u64,
        /// No bytes follow this range.
        eof: bool,
        /// Bounded payload borrowed only during encoding.
        bytes: &'a [u8],
    },
    /// Acknowledgement or release completed.
    Complete,
    /// Explicit failure; request zero denotes a proactive revocation notice.
    Error(&'a str),
}

/// Decode bounded framing without allocating payload or World state.
pub fn decode(bytes: &[u8], connection: u64) -> Result<BulkReadRequest, ProtocolError> {
    if bytes.len() > 46 {
        return Err(ProtocolError::Limit("bulk read request"));
    }
    let mut reader = Reader {
        bytes,
        at: 0,
    };
    if reader.take(4)? != REQUEST_MAGIC || reader.u64()? != connection {
        return Err(ProtocolError::SessionMismatch);
    }
    let id = reader.u64()?;
    let read = reader.u64()?;
    if id == 0 || read == 0 {
        return Err(ProtocolError::Malformed("zero bulk read identity"));
    }
    let operation = match reader.u8()? {
        0 => BulkReadOperation::Read {
            offset: reader.u64()?,
        },
        1 => BulkReadOperation::Acknowledge {
            consumed: reader.u64()?,
            eof: reader.boolean()?,
        },
        2 => BulkReadOperation::Release,
        _ => return Err(ProtocolError::Malformed("bulk read operation")),
    };
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("bulk read trailing bytes"));
    }
    Ok(BulkReadRequest {
        id,
        reference: BulkReadReference {
            connection,
            read,
        },
        operation,
    })
}

/// Encode one bounded reply; no generated contract is required.
pub fn response(
    reference: BulkReadReference,
    id: u64,
    body: BulkReadResponse<'_>,
) -> Result<Vec<u8>, ProtocolError> {
    let capacity = match &body {
        BulkReadResponse::Chunk {
            bytes,
            ..
        } => 42 + bytes.len(),
        BulkReadResponse::Complete => 29,
        BulkReadResponse::Error(reason) => 33 + reason.len().min(64),
    };
    let mut writer = Writer::new(Vec::with_capacity(capacity));
    writer.raw(RESPONSE_MAGIC)?;
    writer.u64(reference.connection)?;
    writer.u64(id)?;
    writer.u64(reference.read)?;
    match body {
        BulkReadResponse::Chunk {
            offset,
            eof,
            bytes,
        } => {
            if bytes.len() > CHUNK_BYTES {
                return Err(ProtocolError::Limit("bulk read chunk"));
            }
            writer.u8(0)?;
            writer.u64(offset)?;
            writer.u8(u8::from(eof))?;
            writer.bytes(bytes)?;
        }
        BulkReadResponse::Complete => writer.u8(1)?,
        BulkReadResponse::Error(reason) => {
            writer.u8(2)?;
            writer.string(&reason[..reason.floor_char_boundary(64)])?;
        }
    }
    Ok(writer.0)
}

/// The fixed bootstrap reply carries a descriptor, never contract bytes.
pub fn contract_descriptor(descriptor: BulkReadDescriptor) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::with_capacity(28));
    writer.raw(&crate::CONTRACT_REPLY_MAGIC)?;
    writer.u64(descriptor.reference.connection)?;
    writer.u64(descriptor.reference.read)?;
    writer.u64(
        descriptor
            .length
            .ok_or(ProtocolError::Malformed("contract length unknown"))?,
    )?;
    Ok(writer.0)
}
