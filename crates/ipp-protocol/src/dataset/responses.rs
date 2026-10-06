use super::payload::{write_kind, write_schema, write_values};
use super::*;
use crate::codec::{ProtocolError, Writer};
use crate::contract::wire_manifest::*;
use ipp_core::components::DynamicValue;
use ipp_core::services::data::{
    DataBatchError, DataBatchOutcome, DataReadView, DataSchema, DataSourceKind, DataSourceMemory,
};

/// Owned bounded typed observation. Pages do not promise a shared snapshot.
#[derive(Debug)]
pub struct DatasetPage {
    /// Literal Host-wide source name.
    pub name: String,
    /// Exact observed source incarnation.
    pub incarnation: u64,
    /// Fixed source behavior.
    pub kind: DataSourceKind,
    /// Fixed raw core columns in value order.
    pub schema: DataSchema,
    /// Bounded retained rows in source position order.
    pub rows: Vec<DatasetRow>,
    /// Next independent observation offset, or end.
    pub next_offset: Option<u64>,
    /// Observed source storage accounting, without changing demand.
    pub memory: DataSourceMemory,
}

/// Stable source-row identity and exact core values.
#[derive(Debug)]
pub struct DatasetRow {
    /// Stable commit-order identity in this source incarnation.
    pub id: u64,
    /// Exact core values in schema order.
    pub values: Vec<DynamicValue>,
}

/// Transport credit and semantic outcomes are different records.
#[derive(Debug)]
pub enum DatasetResponse {
    /// Physical operation accepted; not a semantic update outcome.
    Credit,
    /// Connection-owned producer token, equal to the source incarnation.
    Created(u64),
    /// Exactly one final semantic outcome per admitted complete or interrupted update.
    Outcome(Result<DataBatchOutcome, DataBatchError>),
    /// Release or explicit destruction completed.
    Complete,
    /// Independent bounded typed read.
    Page(DatasetPage),
    /// Bounded completed binding observation encoded by the canonical observer.
    BindingView(Vec<u8>),
    /// Read-only expression driver status encoded by the canonical observer.
    DriverStatus(Vec<u8>),
    /// Recoverable refusal of one physical operation or read.
    Error(String),
    /// Exactly one final refusal of an update before domain admission.
    Refused(String),
}

fn outcome(writer: &mut Writer, outcome: DataBatchOutcome) -> Result<(), ProtocolError> {
    writer.u64(outcome.committed_deltas as u64)?;
    writer.u64(outcome.assigned_rows as u64)?;
    writer.u64(outcome.last_assigned_row.map_or(0, |row| row.0))
}

/// Encode a bounded connection reply. Its reservation remains charged through delivery.
pub fn response(
    connection: u64,
    id: u64,
    body: &DatasetResponse,
) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::new());
    writer.raw(RESPONSE_MAGIC)?;
    writer.u64(connection)?;
    writer.u64(id)?;
    match body {
        DatasetResponse::Credit => writer.u8(DATASET_RESPONSE_CREDIT)?,
        DatasetResponse::Created(producer) => {
            writer.u8(DATASET_RESPONSE_CREATED)?;
            writer.u64(*producer)?;
        }
        DatasetResponse::Outcome(result) => {
            writer.u8(DATASET_RESPONSE_OUTCOME)?;
            match result {
                Ok(committed) => {
                    writer.u8(0)?;
                    outcome(&mut writer, *committed)?;
                }
                Err(error) => {
                    writer.u8(1)?;
                    outcome(&mut writer, error.committed)?;
                    writer.u64(error.delta_index as u64)?;
                    writer.string(&error.reason.to_string())?;
                }
            }
        }
        DatasetResponse::Complete => writer.u8(DATASET_RESPONSE_COMPLETE)?,
        DatasetResponse::Page(page) => {
            writer.u8(DATASET_RESPONSE_PAGE)?;
            writer.string(&page.name)?;
            writer.u64(page.incarnation)?;
            write_kind(&mut writer, page.kind)?;
            write_schema(&mut writer, &page.schema)?;
            writer.u64(page.memory.retained_rows as u64)?;
            writer.u64(page.memory.retained_bytes as u64)?;
            writer.u64(page.memory.allocated_bytes as u64)?;
            writer.u64(page.memory.schema_bytes as u64)?;
            writer.u8(u8::from(page.next_offset.is_some()))?;
            writer.u64(page.next_offset.unwrap_or(0))?;
            writer.count(page.rows.len(), PAGE_ROWS)?;
            for row in &page.rows {
                writer.u64(row.id)?;
                write_values(&mut writer, &row.values)?;
            }
        }
        DatasetResponse::BindingView(bytes) | DatasetResponse::DriverStatus(bytes) => {
            writer.u8(if matches!(body, DatasetResponse::BindingView(_)) {
                DATASET_RESPONSE_BINDING_VIEW
            } else {
                DATASET_RESPONSE_DRIVER_STATUS
            })?;
            writer.count(bytes.len(), PAGE_BYTES - 25)?;
            writer.raw(bytes)?;
        }
        DatasetResponse::Error(error) | DatasetResponse::Refused(error) => {
            writer.u8(if matches!(body, DatasetResponse::Error(_)) {
                DATASET_RESPONSE_ERROR
            } else {
                DATASET_RESPONSE_REFUSED
            })?;
            writer.string(&error[..error.floor_char_boundary(4096)])?;
        }
    }
    if writer.len() > PAGE_BYTES {
        return Err(ProtocolError::Limit("dataset response"));
    }
    Ok(writer.0)
}

impl DatasetPage {
    /// Copy only one bounded page from a borrowed committed view. Queries acquire no demand.
    pub fn observe(view: DataReadView<'_>, offset: u64, limit: u32) -> Result<Self, ProtocolError> {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        if limit == 0 || limit > PAGE_ROWS {
            return Err(ProtocolError::Limit("dataset page rows"));
        }
        if view.schema().columns.len() > COLUMNS || view.name().len() > NAME_BYTES {
            return Err(ProtocolError::Limit("dataset schema"));
        }
        let mut header = Writer::measuring();
        header.string(view.name())?;
        write_schema(&mut header, view.schema())?;
        // Framing, incarnation, kind, accounting, next-offset, count.
        let mut size = header.len() + 20 + 1 + 8 + 1 + 32 + 9 + 4;
        if size > PAGE_BYTES {
            return Err(ProtocolError::Limit("dataset page schema"));
        }
        let offset =
            usize::try_from(offset).map_err(|_| ProtocolError::Limit("dataset read offset"))?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(limit)
            .map_err(|_| ProtocolError::Limit("dataset allocation"))?;
        let mut next_offset = None;
        for row in view.rows().skip(offset) {
            let mut measured = Writer::measuring();
            measured.u64(row.id.0)?;
            write_values(&mut measured, row.values)?;
            if rows.len() == limit || size + measured.len() > PAGE_BYTES {
                if rows.is_empty() {
                    return Err(ProtocolError::Limit("dataset page row"));
                }
                next_offset = Some((offset + rows.len()) as u64);
                break;
            }
            size += measured.len();
            rows.push(DatasetRow {
                id: row.id.0,
                values: row.values.to_vec(),
            });
        }
        Ok(Self {
            name: view.name().into(),
            incarnation: view.source().incarnation(),
            kind: view.kind(),
            schema: view.schema().clone(),
            rows,
            next_offset,
            memory: view.memory(),
        })
    }
}
