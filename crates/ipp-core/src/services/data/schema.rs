use crate::components::{DynamicPropertyKind, DynamicValue};

use super::DataError;

/// One named, fixed core value type. Text columns also declare a byte bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataColumn {
    /// Stable name used for schema or Host-wide source resolution.
    pub name: String,
    /// Exact existing core value type or source kind.
    pub kind: DynamicPropertyKind,
    /// Required byte bound for Text; absent for every other kind.
    pub text_max_bytes: Option<usize>,
}

impl DataColumn {
    /// Declare a column in an existing core kind; Text requires the bounded constructor.
    pub fn new(name: impl Into<String>, kind: DynamicPropertyKind) -> Self {
        Self {
            name: name.into(),
            kind,
            text_max_bytes: None,
        }
    }

    /// Declare a bounded UTF-8 text column.
    pub fn text(name: impl Into<String>, max_bytes: usize) -> Self {
        Self {
            name: name.into(),
            kind: DynamicPropertyKind::Text,
            text_max_bytes: Some(max_bytes),
        }
    }
}

/// Fixed for one source incarnation; display dimensions are not schema columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSchema {
    /// Named columns in fixed row value order.
    pub columns: Vec<DataColumn>,
}

impl DataSchema {
    /// Validate column names, supported kinds and text bounds before admission.
    pub fn validate(&self) -> Result<(), DataError> {
        if self.columns.is_empty() {
            return Err(DataError::InvalidSchema);
        }
        for (index, column) in self.columns.iter().enumerate() {
            if column.name.is_empty()
                || self.columns[..index]
                    .iter()
                    .any(|previous| previous.name == column.name)
            {
                return Err(DataError::InvalidSchema);
            }
            if column.kind == DynamicPropertyKind::Asset {
                return Err(DataError::UnsupportedKind);
            }
            if (column.kind == DynamicPropertyKind::Text) != column.text_max_bytes.is_some() {
                return Err(DataError::InvalidSchema);
            }
        }
        Ok(())
    }

    /// Resolve a raw column name to its fixed schema position.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name == name)
    }

    pub(super) fn validate_row(&self, values: &[DynamicValue]) -> Result<(), DataError> {
        if values.len() != self.columns.len() {
            return Err(DataError::InvalidRow);
        }
        for (column, value) in self.columns.iter().zip(values) {
            if value.kind() != column.kind || value.validate().is_err() {
                return Err(DataError::InvalidRow);
            }
            if let DynamicValue::Text(text) = value
                && text.len() > column.text_max_bytes.expect("validated text column")
            {
                return Err(DataError::InvalidRow);
            }
        }
        Ok(())
    }
}
