use super::payload::{kind, schema, text, write_kind, write_schema};
use super::*;
use crate::codec::{ProtocolError, Reader, Writer};
use crate::wire::*;
use ipp_core::services::data::{DataSchema, DataSourceKind};

/// Source operations scoped to a physical Host connection.
#[derive(Debug)]
pub enum DatasetOperation<'a> {
    /// Create one fixed-schema source under a literal Host-wide name.
    Create {
        /// Literal Host-wide source name.
        name: String,
        /// Fixed source behavior.
        kind: DataSourceKind,
        /// Raw core columns.
        schema: DataSchema,
    },
    /// Reserve assembly and one eventual outcome before any bytes are accepted.
    Begin {
        /// Connection-owned source incarnation.
        producer: u64,
        /// Exact encoded update byte count.
        length: u64,
    },
    /// Ordered finite-transfer continuation; credit is separate from the update outcome.
    Chunk {
        /// Begin request identity.
        transfer: u64,
        /// Expected contiguous byte offset, or source row position for reads.
        offset: u64,
        /// Transient encoded update chunk.
        bytes: &'a [u8],
    },
    /// Complete one update, applying ordered deltas through DataService.
    Finish {
        /// Begin request identity.
        transfer: u64,
    },
    /// Cancel an incomplete update without mutation.
    Cancel {
        /// Begin request identity.
        transfer: u64,
    },
    /// Detach this connection's producer, preserving consumer-owned source lifetime.
    Release {
        /// Connection-owned source incarnation.
        producer: u64,
    },
    /// Explicitly destroy this producer's incarnation.
    Destroy {
        /// Connection-owned source incarnation.
        producer: u64,
    },
    /// Read an independent typed observation without demand or clock effects.
    Read {
        /// Literal Host-wide source name.
        name: String,
        /// Optional exact source incarnation fence.
        incarnation: Option<u64>,
        /// Expected contiguous byte offset, or source row position for reads.
        offset: u64,
        /// Maximum rows in this independent page.
        limit: u32,
    },
    /// Observe one completed entity-local binding through an attached World session.
    BindingView {
        /// Session must belong to this physical connection.
        session: u64,
        /// Exact generational entity identity.
        entity: u64,
        /// Positional page offset.
        offset: u64,
        /// Requested bounded row count.
        limit: u32,
    },
    /// Observe one expression driver without evaluating it.
    DriverStatus {
        /// Session must belong to this physical connection.
        session: u64,
        /// Exact generational entity identity.
        entity: u64,
    },
}

/// Monotonically correlated lane request, fenced to a Host connection.
pub struct DatasetRequest<'a> {
    /// Nonzero lane request identity.
    pub id: u64,
    /// Operation with transient chunk borrow.
    pub operation: DatasetOperation<'a>,
}

/// Decode bounded dataset framing independently of World sessions.
pub fn decode(bytes: &[u8], connection: u64) -> Result<DatasetRequest<'_>, ProtocolError> {
    if bytes.len() > FRAME_BYTES {
        return Err(ProtocolError::Limit("dataset frame"));
    }
    let mut reader = Reader {
        bytes,
        at: 0,
    };
    if reader.take(4)? != REQUEST_MAGIC || reader.u64()? != connection {
        return Err(ProtocolError::SessionMismatch);
    }
    let id = reader.u64()?;
    if id == 0 {
        return Err(ProtocolError::Malformed("dataset request identity"));
    }
    let operation = match reader.u8()? {
        DATASET_REQUEST_CREATE => DatasetOperation::Create {
            name: text(&mut reader, NAME_BYTES)?,
            kind: kind(&mut reader)?,
            schema: schema(&mut reader)?,
        },
        DATASET_REQUEST_BEGIN => DatasetOperation::Begin {
            producer: reader.u64()?,
            length: reader.u64()?,
        },
        DATASET_REQUEST_CHUNK => {
            let transfer = reader.u64()?;
            let offset = reader.u64()?;
            let length = reader.count(CHUNK_BYTES)?;
            DatasetOperation::Chunk {
                transfer,
                offset,
                bytes: reader.take(length)?,
            }
        }
        DATASET_REQUEST_FINISH => DatasetOperation::Finish {
            transfer: reader.u64()?,
        },
        DATASET_REQUEST_CANCEL => DatasetOperation::Cancel {
            transfer: reader.u64()?,
        },
        DATASET_REQUEST_RELEASE => DatasetOperation::Release {
            producer: reader.u64()?,
        },
        DATASET_REQUEST_DESTROY => DatasetOperation::Destroy {
            producer: reader.u64()?,
        },
        DATASET_REQUEST_READ => DatasetOperation::Read {
            name: text(&mut reader, NAME_BYTES)?,
            incarnation: match reader.u64()? {
                0 => None,
                value => Some(value),
            },
            offset: reader.u64()?,
            limit: reader.u32()?,
        },
        DATASET_REQUEST_BINDING_VIEW => DatasetOperation::BindingView {
            session: reader.u64()?,
            entity: reader.u64()?,
            offset: reader.u64()?,
            limit: reader.u32()?,
        },
        DATASET_REQUEST_DRIVER_STATUS => DatasetOperation::DriverStatus {
            session: reader.u64()?,
            entity: reader.u64()?,
        },
        tag => return Err(ProtocolError::Unsupported(tag)),
    };
    if reader.at != bytes.len() {
        return Err(ProtocolError::Malformed("dataset request trailing bytes"));
    }
    Ok(DatasetRequest {
        id,
        operation,
    })
}

/// Encode a dataset request for maintained transport fixtures and adapters.
pub fn encode_request(
    connection: u64,
    id: u64,
    operation: &DatasetOperation<'_>,
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::new());
    writer.raw(REQUEST_MAGIC)?;
    writer.u64(connection)?;
    writer.u64(id)?;
    match operation {
        DatasetOperation::Create {
            name,
            kind,
            schema,
        } => {
            writer.u8(DATASET_REQUEST_CREATE)?;
            writer.string(name)?;
            write_kind(&mut writer, *kind)?;
            write_schema(&mut writer, schema)?;
        }
        DatasetOperation::Begin {
            producer,
            length,
        } => {
            writer.u8(DATASET_REQUEST_BEGIN)?;
            writer.u64(*producer)?;
            writer.u64(*length)?;
        }
        DatasetOperation::Chunk {
            transfer,
            offset,
            bytes,
        } => {
            writer.u8(DATASET_REQUEST_CHUNK)?;
            writer.u64(*transfer)?;
            writer.u64(*offset)?;
            writer.count(bytes.len(), CHUNK_BYTES)?;
            writer.raw(bytes)?;
        }
        DatasetOperation::Finish {
            transfer,
        } => {
            writer.u8(DATASET_REQUEST_FINISH)?;
            writer.u64(*transfer)?;
        }
        DatasetOperation::Cancel {
            transfer,
        } => {
            writer.u8(DATASET_REQUEST_CANCEL)?;
            writer.u64(*transfer)?;
        }
        DatasetOperation::Release {
            producer,
        } => {
            writer.u8(DATASET_REQUEST_RELEASE)?;
            writer.u64(*producer)?;
        }
        DatasetOperation::Destroy {
            producer,
        } => {
            writer.u8(DATASET_REQUEST_DESTROY)?;
            writer.u64(*producer)?;
        }
        DatasetOperation::Read {
            name,
            incarnation,
            offset,
            limit,
        } => {
            writer.u8(DATASET_REQUEST_READ)?;
            writer.string(name)?;
            writer.u64(incarnation.unwrap_or(0))?;
            writer.u64(*offset)?;
            writer.u32(*limit)?;
        }
        DatasetOperation::BindingView {
            session,
            entity,
            offset,
            limit,
        } => {
            writer.u8(DATASET_REQUEST_BINDING_VIEW)?;
            writer.u64(*session)?;
            writer.u64(*entity)?;
            writer.u64(*offset)?;
            writer.u32(*limit)?;
        }
        DatasetOperation::DriverStatus {
            session,
            entity,
        } => {
            writer.u8(DATASET_REQUEST_DRIVER_STATUS)?;
            writer.u64(*session)?;
            writer.u64(*entity)?;
        }
    }
    if writer.0.len() > FRAME_BYTES {
        return Err(ProtocolError::Limit("dataset frame"));
    }
    Ok(writer.0)
}
