use std::fmt;

use crate::components::DynamicValue;

use super::{
    DataAvailability, DataProducerHandle, DataRowId, DataService, DataSourceKind, retention,
    source::DataRow,
};

/// Deltas are indivisible admission units; a malformed row rejects its complete delta.
#[derive(Debug)]
pub enum DataDelta {
    /// Append multiple rows in source position order.
    Append {
        /// Complete typed rows, each matching the fixed schema.
        rows: Vec<Vec<DynamicValue>>,
    },
    /// Buffer position, independent of the new row's commit-order identity.
    Insert {
        /// Source position before which new rows are inserted.
        index: usize,
        /// Complete typed rows, each matching the fixed schema.
        rows: Vec<Vec<DynamicValue>>,
    },
    /// Replace one buffer row without changing its identity or position.
    Edit {
        /// Existing identity scoped to the producer's source incarnation.
        row: DataRowId,
        /// Complete replacement row, matching the fixed schema.
        values: Vec<DynamicValue>,
    },
    /// Remove one buffer row by stable identity.
    Remove {
        /// Existing identity scoped to the producer's source incarnation.
        row: DataRowId,
    },
}

/// Validated admission and lifetime failure; no hidden rollback is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataError {
    /// A name is empty or contains a control character.
    InvalidName,
    /// Columns are empty, unnamed, duplicated or have invalid text bounds.
    InvalidSchema,
    /// Asset columns have no dataset ownership contract and are rejected.
    UnsupportedKind,
    /// Row width, exact core types, finite values or text bounds are invalid.
    InvalidRow,
    /// Window kind, width, anchor or unit declaration is invalid.
    InvalidWindow,
    /// A window names a missing or non-scalar-numeric raw column.
    InvalidColumn,
    /// A window update would rewind an existing anchor on the same raw column.
    AnchorMovedBackwards,
    /// No current source resolves under the requested name.
    MissingSource,
    /// The resolved source has the other source kind.
    KindMismatch,
    /// A live producer already holds this source name.
    ProducerExists,
    /// The producer detached, was destroyed or names a replaced incarnation.
    StaleProducer,
    /// The source incarnation is no longer present on this service.
    StaleSource,
    /// The consumer was released or belongs to another service.
    StaleConsumer,
    /// This exact binding identity is already registered.
    ConsumerExists,
    /// A buffer insert position exceeds its current length.
    InvalidPosition,
    /// The buffer contains no row with this incarnation-scoped sequence number.
    MissingRow,
    /// Streaming sources permit only append updates.
    UnsupportedDelta,
    /// Identity space, checked byte accounting or owned allocation is exhausted.
    Capacity,
    /// Host time or its selected unit conversion would be nonfinite or go backwards.
    InvalidTime,
}

impl fmt::Display for DataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for DataError {}

/// Committed prefix and deterministic assignment evidence for an ordered update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DataBatchOutcome {
    /// Number of complete deltas already applied.
    pub committed_deltas: usize,
    /// Assigned identities include samples that expired at commit.
    pub assigned_rows: usize,
    /// Final row identity assigned by this batch, including immediately expired arrivals.
    pub last_assigned_row: Option<DataRowId>,
}

/// Earlier deltas stay committed; `delta_index` is the first unapplied delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataBatchError {
    /// Zero-based index of the first rejected delta.
    pub delta_index: usize,
    /// Admission or lifetime failure at that delta.
    pub reason: DataError,
    /// Exact prefix outcome preserved without rollback.
    pub committed: DataBatchOutcome,
}

impl fmt::Display for DataBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "delta {} failed: {}; {} earlier deltas committed",
            self.delta_index, self.reason, self.committed.committed_deltas
        )
    }
}

impl std::error::Error for DataBatchError {}

impl DataService {
    /// Apply in order, stopping at the first invalid delta without rolling back the prefix.
    /// Input ownership transfers here; the service never borrows producer payload after return.
    pub fn apply_batch(
        &mut self,
        producer: DataProducerHandle,
        deltas: impl IntoIterator<Item = DataDelta>,
    ) -> Result<DataBatchOutcome, DataBatchError> {
        let mut outcome = DataBatchOutcome::default();
        let index = self
            .producer_index(producer)
            .map_err(|reason| DataBatchError {
                delta_index: 0,
                reason,
                committed: outcome,
            })?;
        for (delta_index, delta) in deltas.into_iter().enumerate() {
            let before = self.sources[index].next_row;
            if let Err(reason) = self.apply_delta(index, delta) {
                return Err(DataBatchError {
                    delta_index,
                    reason,
                    committed: outcome,
                });
            }
            let assigned = self.sources[index].next_row - before;
            outcome.committed_deltas += 1;
            outcome.assigned_rows += assigned as usize;
            if assigned > 0 {
                outcome.last_assigned_row = Some(DataRowId(self.sources[index].next_row));
            }
            self.notify_source(producer.source);
        }
        Ok(outcome)
    }

    fn apply_delta(&mut self, index: usize, delta: DataDelta) -> Result<(), DataError> {
        let streaming = self.sources[index].kind == DataSourceKind::Streaming;
        match delta {
            DataDelta::Append {
                rows,
            } => {
                let position = self.sources[index].rows.len();
                self.insert_rows(index, position, rows)
            }
            DataDelta::Insert {
                index: position,
                rows,
            } => {
                if streaming {
                    return Err(DataError::UnsupportedDelta);
                }
                self.insert_rows(index, position, rows)
            }
            DataDelta::Edit {
                row,
                values,
            } => {
                if streaming {
                    return Err(DataError::UnsupportedDelta);
                }
                let source = &mut self.sources[index];
                let position = source
                    .rows
                    .iter()
                    .position(|stored| stored.id == row)
                    .ok_or(DataError::MissingRow)?;
                source.schema.validate_row(&values)?;
                let bytes = row_bytes(&values)?;
                source
                    .memory()
                    .retained_bytes
                    .checked_sub(source.rows[position].bytes)
                    .and_then(|total| total.checked_add(bytes))
                    .ok_or(DataError::Capacity)?;
                source.rows[position] = DataRow {
                    id: row,
                    values,
                    bytes,
                };
                Ok(())
            }
            DataDelta::Remove {
                row,
            } => {
                if streaming {
                    return Err(DataError::UnsupportedDelta);
                }
                let source = &mut self.sources[index];
                let position = source
                    .rows
                    .iter()
                    .position(|stored| stored.id == row)
                    .ok_or(DataError::MissingRow)?;
                source.rows.remove(position);
                Ok(())
            }
        }
    }

    fn insert_rows(
        &mut self,
        index: usize,
        position: usize,
        rows: Vec<Vec<DynamicValue>>,
    ) -> Result<(), DataError> {
        let source = &self.sources[index];
        if position > source.rows.len() {
            return Err(DataError::InvalidPosition);
        }
        let count = u64::try_from(rows.len()).map_err(|_| DataError::Capacity)?;
        let next_row = source
            .next_row
            .checked_add(count)
            .ok_or(DataError::Capacity)?;
        let mut prepared = Vec::new();
        prepared
            .try_reserve_exact(rows.len())
            .map_err(|_| DataError::Capacity)?;
        let mut latest = Vec::new();
        latest
            .try_reserve_exact(source.latest.len())
            .map_err(|_| DataError::Capacity)?;
        latest.extend_from_slice(&source.latest);
        for (offset, values) in rows.into_iter().enumerate() {
            source.schema.validate_row(&values)?;
            let bytes = row_bytes(&values)?;
            for (column, value) in values.iter().enumerate() {
                if let Some(value) = retention::numeric(value) {
                    latest[column] = Some(latest[column].map_or(value, |prior| prior.max(value)));
                }
            }
            prepared.push(DataRow {
                id: DataRowId(source.next_row + offset as u64 + 1),
                values,
                bytes,
            });
        }

        let mut committed_default_start = None;
        if source.kind == DataSourceKind::Streaming {
            let mut bytes = 0usize;
            let mut default_start = u128::from(next_row) + 1;
            for row in source.rows.iter().chain(&prepared).rev() {
                let Some(next) = bytes.checked_add(row.bytes) else {
                    break;
                };
                if next > self.config.default_stream_bytes {
                    break;
                }
                bytes = next;
                default_start = u128::from(row.id.0);
            }
            committed_default_start = Some(default_start);
            let wanted = |row: &DataRow| {
                self.consumers.iter().any(|consumer| {
                    consumer.state.source == Some(source.handle)
                        && consumer.state.availability == DataAvailability::Ready
                        && if consumer.request.windows.is_empty() {
                            u128::from(row.id.0) >= default_start.max(consumer.default_start)
                        } else {
                            retention::contains_parts(
                                next_row,
                                &source.schema,
                                &latest,
                                consumer,
                                row,
                                self.time,
                            )
                        }
                })
            };
            source
                .rows
                .iter()
                .chain(&prepared)
                .filter(|row| wanted(row))
                .try_fold(0usize, |total, row| total.checked_add(row.bytes))
                .ok_or(DataError::Capacity)?;
            // Outside-window arrivals consume IDs but do not allocate source row slots.
            prepared.retain(wanted);
        } else {
            source
                .rows
                .iter()
                .chain(&prepared)
                .try_fold(0usize, |total, row| total.checked_add(row.bytes))
                .ok_or(DataError::Capacity)?;
        }

        let source = &mut self.sources[index];
        source
            .rows
            .try_reserve(prepared.len())
            .map_err(|_| DataError::Capacity)?;
        source.next_row = next_row;
        source.latest = latest;
        source.rows.splice(position..position, prepared);
        self.expire_source_at(index, committed_default_start);
        Ok(())
    }
}

fn row_bytes(values: &Vec<DynamicValue>) -> Result<usize, DataError> {
    let storage = values
        .capacity()
        .checked_mul(std::mem::size_of::<DynamicValue>())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<DataRow>()))
        .ok_or(DataError::Capacity)?;
    values
        .iter()
        .try_fold(storage, |bytes, value| {
            bytes.checked_add(match value {
                DynamicValue::Text(text) => text.len(),
                _ => 0,
            })
        })
        .ok_or(DataError::Capacity)
}
