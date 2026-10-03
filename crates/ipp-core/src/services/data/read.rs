use crate::components::DynamicValue;

use super::{
    DataRowId, DataSchema, DataSourceHandle, DataSourceKind, DataSourceMemory,
    consumer::DataConsumer, retention::contains, source::DataSource,
};

/// Borrowed source storage. Rust forbids mutation, replacement or expiry until this view ends.
#[derive(Clone, Copy)]
pub struct DataReadView<'a> {
    pub(super) source: &'a DataSource,
    pub(super) consumer: Option<&'a DataConsumer>,
    pub(super) time: f64,
}

/// Borrowed values from one stable source-row identity.
#[derive(Clone, Copy, Debug)]
pub struct DataRowView<'a> {
    /// Stable commit-order identity scoped to the viewed source incarnation.
    pub id: DataRowId,
    /// Immutable core values in source schema order.
    pub values: &'a [DynamicValue],
}

impl DataRowView<'_> {
    /// Borrow a row value at the raw schema column position.
    pub fn value(&self, column: usize) -> Option<&DynamicValue> {
        self.values.get(column)
    }
}

impl<'a> DataReadView<'a> {
    /// Observe the fenced incarnation associated with this handle or view.
    pub fn source(self) -> DataSourceHandle {
        self.source.handle
    }

    /// Borrow the Host-wide source name.
    pub fn name(self) -> &'a str {
        &self.source.name
    }

    /// Observe the fixed source kind.
    pub fn kind(self) -> DataSourceKind {
        self.source.kind
    }

    /// Borrow the immutable schema for this incarnation.
    pub fn schema(self) -> &'a DataSchema {
        &self.source.schema
    }

    /// Source-wide accounting, including rows retained solely by other consumers.
    pub fn memory(self) -> DataSourceMemory {
        self.source.memory()
    }

    /// Iterate selected rows in source position order without copying payload.
    pub fn rows(self) -> impl DoubleEndedIterator<Item = DataRowView<'a>> + 'a {
        self.source
            .rows
            .iter()
            .filter(move |row| {
                self.consumer
                    .is_none_or(|consumer| contains(self.source, consumer, row, self.time))
            })
            .map(|row| DataRowView {
                id: row.id,
                values: &row.values,
            })
    }

    /// Find a selected row by stable identity, independently of its position.
    pub fn row(self, id: DataRowId) -> Option<DataRowView<'a>> {
        self.rows().find(|row| row.id == id)
    }

    /// Count rows in this selected view.
    pub fn len(self) -> usize {
        self.rows().count()
    }

    /// Whether this selected view exposes any rows.
    pub fn is_empty(self) -> bool {
        self.rows().next().is_none()
    }
}
