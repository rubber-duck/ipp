use crate::components::DynamicValue;

use super::DataSchema;

/// Fixed source behavior for one incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSourceKind {
    /// Mutable positional rows without automatic expiry.
    Buffer,
    /// Append-only rows retained by the union of ready consumer demand.
    Streaming,
}

/// Opaque authority for an exact source incarnation on one service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataSourceHandle {
    pub(super) service: u64,
    pub(super) incarnation: u64,
}

impl DataSourceHandle {
    /// Observational identity; this does not manufacture a live handle.
    pub fn incarnation(self) -> u64 {
        self.incarnation
    }
}

/// Local producer authority. Detach, destruction and replacement invalidate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataProducerHandle {
    pub(super) source: DataSourceHandle,
}

impl DataProducerHandle {
    /// Observe the fenced incarnation associated with this handle or view.
    pub fn source(self) -> DataSourceHandle {
        self.source
    }
}

/// Commit-order sequence number scoped to a source incarnation, starting at one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataRowId(pub u64);

/// Accounting for source-owned storage; text references count their full byte length.
/// Shared text is conservatively counted per row, independent of external owners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DataSourceMemory {
    /// Number of live rows in the source-wide union.
    pub retained_rows: usize,
    /// Live row descriptors, value allocations and referenced text bytes.
    pub retained_bytes: usize,
    /// Reserved row storage plus live value allocations and referenced text bytes.
    pub allocated_bytes: usize,
    /// Source metadata, schema strings and numeric high-water-mark allocation bytes.
    pub schema_bytes: usize,
}

pub(super) struct DataRow {
    /// Stable commit-order identity scoped to the viewed source incarnation.
    pub id: DataRowId,
    /// Immutable core values in source schema order.
    pub values: Vec<DynamicValue>,
    pub bytes: usize,
}

pub(super) struct DataSource {
    pub handle: DataSourceHandle,
    /// Stable name used for schema or Host-wide source resolution.
    pub name: String,
    /// Exact existing core value type or source kind.
    pub kind: DataSourceKind,
    pub schema: DataSchema,
    pub producer: bool,
    pub next_row: u64,
    pub rows: Vec<DataRow>,
    // Numeric high water marks include committed samples that were immediately expired.
    pub latest: Vec<Option<f64>>,
}

impl DataSource {
    /// Observe source-wide retained and allocated memory accounting.
    pub fn memory(&self) -> DataSourceMemory {
        let retained_bytes = self.rows.iter().map(|row| row.bytes).sum();
        DataSourceMemory {
            retained_rows: self.rows.len(),
            retained_bytes,
            allocated_bytes: retained_bytes
                + (self.rows.capacity() - self.rows.len()) * std::mem::size_of::<DataRow>(),
            schema_bytes: std::mem::size_of::<Self>()
                + self.name.capacity()
                + self.schema.columns.capacity() * std::mem::size_of::<super::DataColumn>()
                + self
                    .schema
                    .columns
                    .iter()
                    .map(|column| column.name.capacity())
                    .sum::<usize>()
                + self.latest.capacity() * std::mem::size_of::<Option<f64>>(),
        }
    }
}
