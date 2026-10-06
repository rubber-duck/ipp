use super::{DataBindingAvailability, DataBindingRuntime};
use crate::{
    DynamicPropertyKind, EntityId, ErrorReason,
    expressions::ExpressionResult,
    services::data::{DataRowId, DataSourceHandle},
};

/// Maximum rows returned by one observation.
pub const DATA_BINDING_QUERY_MAX_ROWS: usize = 1024;
/// Maximum estimated owned payload bytes returned by one observation.
pub const DATA_BINDING_QUERY_MAX_BYTES: usize = 1 << 20;

/// Bounded positional pagination over a completed view. Pair successive pages with
/// source, binding incarnation and evaluated tick; observations never pin an old cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataBindingViewQuery {
    /// Zero-based row position in the completed view.
    pub offset: usize,
    /// Requested row count, clamped to the public row and byte bounds.
    pub limit: usize,
}

impl Default for DataBindingViewQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DATA_BINDING_QUERY_MAX_ROWS,
        }
    }
}

/// Borrowed page of one typed projection output.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBindingColumnView<'a> {
    /// Authored output property name.
    pub name: &'a str,
    /// Exact expression output type.
    pub kind: DynamicPropertyKind,
    /// Aligned with `row_ids`; invalid calculations retain their explicit reason.
    pub values: &'a [ExpressionResult],
}

/// Borrowed bounded observation of completed component runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBindingView<'a> {
    /// Exact resolved source lifetime, including an incompatible source when known.
    pub source: Option<DataSourceHandle>,
    /// Exact binding component lifetime in this World.
    pub binding_incarnation: u64,
    /// World tick that last evaluated the output, absent before evaluation.
    pub evaluated_tick: Option<u64>,
    /// Binding-wide readiness and preparation failure reason.
    pub availability: &'a DataBindingAvailability,
    /// Presentation handoff flag; observing it never acknowledges an update.
    pub dirty: bool,
    /// Total rows in this completed binding view.
    pub total_rows: usize,
    /// Zero-based row position in the completed view.
    pub offset: usize,
    /// Continuation position when more rows remain in this cut.
    pub next_offset: Option<usize>,
    /// Stable source row identities, aligned with every output column.
    pub row_ids: &'a [DataRowId],
    /// Typed named projection outputs in deterministic property-name order.
    pub columns: Vec<DataBindingColumnView<'a>>,
}

/// Full borrowed internal preparation cut for a runtime presentation consumer.
/// Unlike client observations this is neither paginated nor byte/row limited.
pub struct DataBindingPreparedView<'a> {
    /// Exact resolved source incarnation.
    pub source: Option<DataSourceHandle>,
    /// Exact binding component incarnation.
    pub binding_incarnation: u64,
    /// Binding-wide preparation availability.
    pub availability: &'a DataBindingAvailability,
    /// Sole binding-owned presentation handoff flag.
    pub dirty: bool,
    /// Entire selected source-row identity set.
    pub row_ids: &'a [DataRowId],
    /// Entire evaluated row-aligned output columns.
    pub columns: Vec<DataBindingColumnView<'a>>,
}

/// Owned page of one typed projection output.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBindingOutputColumn {
    /// Authored output property name.
    pub name: String,
    /// Exact expression output type.
    pub kind: DynamicPropertyKind,
    /// Row-aligned values with explicit calculation validity.
    pub values: Vec<ExpressionResult>,
}

/// Owned bounded observation for deferred encoding. Contains derived outputs only;
/// this is never source persistence or authority to consume a presentation update.
#[derive(Clone, Debug, PartialEq)]
pub struct DataBindingViewResult {
    /// Exact resolved source lifetime, including an incompatible source when known.
    pub source: Option<DataSourceHandle>,
    /// Exact binding component lifetime in this World.
    pub binding_incarnation: u64,
    /// World tick that last evaluated the output, absent before evaluation.
    pub evaluated_tick: Option<u64>,
    /// Binding-wide readiness and preparation failure reason.
    pub availability: DataBindingAvailability,
    /// Presentation handoff flag; observing it never acknowledges an update.
    pub dirty: bool,
    /// Total rows in this completed binding view.
    pub total_rows: usize,
    /// Zero-based row position in the completed view.
    pub offset: usize,
    /// Continuation position when more rows remain in this cut.
    pub next_offset: Option<usize>,
    /// Stable source row identities, aligned with every output column.
    pub row_ids: Vec<DataRowId>,
    /// Typed named projection outputs in deterministic property-name order.
    pub columns: Vec<DataBindingOutputColumn>,
}

impl DataBindingView<'_> {
    /// Detach this already bounded page without observing or changing live state.
    pub fn to_owned(&self) -> DataBindingViewResult {
        DataBindingViewResult {
            source: self.source,
            binding_incarnation: self.binding_incarnation,
            evaluated_tick: self.evaluated_tick,
            availability: self.availability.clone(),
            dirty: self.dirty,
            total_rows: self.total_rows,
            offset: self.offset,
            next_offset: self.next_offset,
            row_ids: self.row_ids.to_vec(),
            columns: self
                .columns
                .iter()
                .map(|column| DataBindingOutputColumn {
                    name: column.name.to_owned(),
                    kind: column.kind,
                    values: column.values.to_vec(),
                })
                .collect(),
        }
    }
}

fn page(
    runtime: &DataBindingRuntime,
    incarnation: u64,
    query: DataBindingViewQuery,
) -> Result<DataBindingView<'_>, ErrorReason> {
    let start = query.offset.min(runtime.rows.len());
    let mut end = start;
    let mut bytes = std::mem::size_of::<DataBindingViewResult>();
    // Include owned descriptors and failure text before adding any rows.
    bytes = bytes.saturating_add(format_size(&runtime.availability));
    for column in &runtime.columns {
        bytes = bytes
            .saturating_add(column.name.len())
            .saturating_add(std::mem::size_of::<DataBindingOutputColumn>());
    }
    if bytes > DATA_BINDING_QUERY_MAX_BYTES {
        return Err(ErrorReason::Capacity);
    }
    let row_limit = query.limit.min(DATA_BINDING_QUERY_MAX_ROWS);
    while end < runtime.rows.len() && end - start < row_limit {
        let mut row_bytes = std::mem::size_of::<DataRowId>();
        for column in &runtime.columns {
            row_bytes = row_bytes.saturating_add(std::mem::size_of::<ExpressionResult>());
            if let ExpressionResult::Valid(crate::DynamicValue::Text(value)) = &column.values[end] {
                row_bytes = row_bytes.saturating_add(value.len());
            }
        }
        if row_bytes > DATA_BINDING_QUERY_MAX_BYTES - bytes {
            if end == start {
                return Err(ErrorReason::Capacity);
            }
            break;
        }
        bytes += row_bytes;
        end += 1;
    }
    Ok(DataBindingView {
        source: runtime.source,
        binding_incarnation: incarnation,
        evaluated_tick: runtime.evaluated_tick,
        availability: &runtime.availability,
        dirty: runtime.dirty,
        total_rows: runtime.rows.len(),
        offset: start,
        next_offset: (end < runtime.rows.len()).then_some(end),
        row_ids: &runtime.rows[start..end],
        columns: runtime
            .columns
            .iter()
            .map(|column| DataBindingColumnView {
                name: &column.name,
                kind: column.kind,
                values: &column.values[start..end],
            })
            .collect(),
    })
}

fn format_size(availability: &DataBindingAvailability) -> usize {
    use super::DataBindingUnavailable::*;
    match availability {
        DataBindingAvailability::Unavailable(MissingAsset {
            output,
        }) => output.len(),
        DataBindingAvailability::Unavailable(
            MissingInput {
                output,
                input,
            }
            | InputType {
                output,
                input,
            }
            | InvalidInputName {
                output,
                input,
            },
        ) => output.len().saturating_add(input.len()),
        _ => 0,
    }
}

macro_rules! query_access {
    ($context:ty) => {
        impl $context {
            /// Observe completed binding output. Does not prepare, recompute, progress time,
            /// take Data Service notifications or acknowledge presentation dirty state.
            pub fn data_binding_view(
                &self,
                entity: EntityId,
                query: DataBindingViewQuery,
            ) -> Result<DataBindingView<'_>, ErrorReason> {
                let record = self
                    .world
                    .state
                    .entities
                    .get(&entity)
                    .ok_or(ErrorReason::InvalidEntity)?;
                let index = entity.index() as usize;
                let (component, runtime) =
                    if let Some(value) = self.world.components.buffer_data_source_binding(index) {
                        (
                            crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING,
                            &value.runtime,
                        )
                    } else if let Some(value) =
                        self.world.components.streaming_data_source_binding(index)
                    {
                        (
                            crate::ComponentValue::STREAMING_DATA_SOURCE_BINDING,
                            &value.runtime,
                        )
                    } else {
                        return Err(ErrorReason::MissingComponent);
                    };
                let incarnation = record
                    .input(component)
                    .ok_or(ErrorReason::MissingComponent)?
                    .incarnation;
                page(runtime, incarnation, query)
            }

            /// Detach a bounded completed page for deferred transport encoding.
            pub fn data_binding_view_owned(
                &self,
                entity: EntityId,
                query: DataBindingViewQuery,
            ) -> Result<DataBindingViewResult, ErrorReason> {
                self.data_binding_view(entity, query)
                    .map(|view| view.to_owned())
            }
        }
    };
}

query_access!(crate::WorldContext<'_>);
query_access!(crate::systems::SystemRuntimeAccess<'_>);

impl crate::WorldContext<'_> {
    /// Narrow test control for headless dirty semantics, excluded from production.
    /// A real presentation consumer uses the exact-lifetime success API below.
    #[cfg(feature = "instrumentation")]
    pub fn acknowledge_data_binding_for_test(
        &mut self,
        entity: EntityId,
        binding_incarnation: u64,
    ) -> Result<(), ErrorReason> {
        let (component, incarnation) = binding_identity(self.world, entity)?;
        if incarnation != binding_incarnation {
            return Err(ErrorReason::InvalidEntity);
        }
        super::system::runtime_mut(&mut self.world.components, entity, component)
            .ok_or(ErrorReason::MissingComponent)?
            .dirty = false;
        Ok(())
    }
}

pub(super) fn binding_identity(
    world: &crate::world::WorldSimulationState,
    entity: EntityId,
) -> Result<(u16, u64), ErrorReason> {
    let record = world
        .state
        .entities
        .get(&entity)
        .ok_or(ErrorReason::InvalidEntity)?;
    for component in [
        crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING,
        crate::ComponentValue::STREAMING_DATA_SOURCE_BINDING,
    ] {
        if let Some(input) = record.input(component) {
            return Ok((component, input.incarnation));
        }
    }
    Err(ErrorReason::MissingComponent)
}

impl crate::systems::SystemRuntimeAccess<'_> {
    /// Observe only the component-owned dirty flag without allocating output descriptors.
    pub fn data_binding_dirty(&self, entity: EntityId) -> Result<bool, ErrorReason> {
        let (component, _) = binding_identity(self.world, entity)?;
        let index = entity.index() as usize;
        Ok(
            if component == crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING {
                self.world
                    .components
                    .buffer_data_source_binding(index)
                    .ok_or(ErrorReason::MissingComponent)?
                    .runtime
                    .dirty
            } else {
                self.world
                    .components
                    .streaming_data_source_binding(index)
                    .ok_or(ErrorReason::MissingComponent)?
                    .runtime
                    .dirty
            },
        )
    }

    /// Borrow the full completed binding input for runtime preparation only.
    /// This observes without recomputing or acknowledging the dirty handoff.
    pub fn data_binding_prepared_view(
        &self,
        entity: EntityId,
    ) -> Result<DataBindingPreparedView<'_>, ErrorReason> {
        let (component, binding_incarnation) = binding_identity(self.world, entity)?;
        let index = entity.index() as usize;
        let runtime = if component == crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING {
            &self
                .world
                .components
                .buffer_data_source_binding(index)
                .ok_or(ErrorReason::MissingComponent)?
                .runtime
        } else {
            &self
                .world
                .components
                .streaming_data_source_binding(index)
                .ok_or(ErrorReason::MissingComponent)?
                .runtime
        };
        Ok(DataBindingPreparedView {
            source: runtime.source,
            binding_incarnation,
            availability: &runtime.availability,
            dirty: runtime.dirty,
            row_ids: &runtime.rows,
            columns: runtime
                .columns
                .iter()
                .map(|column| DataBindingColumnView {
                    name: &column.name,
                    kind: column.kind,
                    values: &column.values,
                })
                .collect(),
        })
    }
}
