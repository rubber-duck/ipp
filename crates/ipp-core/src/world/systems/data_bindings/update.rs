use super::runtime_state::*;
use crate::world::systems::asset_dependencies::source_key_from_fields;
use crate::{
    DynamicProperties, DynamicPropertyKind, DynamicValue,
    expressions::*,
    services::{
        asset_management::{
            AssetManagementService,
            formats::expression::{EXPRESSION_TYPE, ExpressionAsset},
        },
        data::*,
    },
};

/// One binding's authored inputs and runtime for an evaluation pass.
pub(super) struct DataBindingEvaluation<'a> {
    pub properties: &'a DynamicProperties,
    /// Authored window-position reconciliation of interpolated outputs instead
    /// of exact row identity.
    pub by_position: bool,
    pub runtime: &'a mut DataBindingRuntime,
}

pub(super) fn evaluate(
    binding: DataBindingEvaluation<'_>,
    data: &DataService,
    assets: &AssetManagementService,
    world: crate::WorldId,
    tick: u64,
    dt: f64,
) {
    let DataBindingEvaluation {
        properties,
        by_position,
        runtime,
    } = binding;
    if !runtime.needs_prepare && !runtime.needs_evaluate {
        super::interpolation::advance_or_defer(runtime, properties, tick, dt);
        return;
    }
    let view = runtime
        .consumer
        .ok_or(DataError::MissingSource)
        .and_then(|handle| data.read_consumer(handle));
    let view = match view {
        Ok(view) => view,
        Err(error) => {
            runtime.unavailable(DataBindingUnavailable::Source(error));
            runtime.evaluated_tick = Some(tick);
            return;
        }
    };
    if runtime.source != Some(view.source()) {
        runtime.reset_rows();
        runtime.source = Some(view.source());
    }
    if runtime.needs_prepare {
        if let Err(reason) = prepare(properties, runtime, view.schema(), assets, world) {
            runtime.unavailable(reason);
            runtime.evaluated_tick = Some(tick);
            return;
        }
        runtime.dirty = true;
        runtime.needs_prepare = false;
    }
    if runtime.availability != DataBindingAvailability::Ready {
        runtime.dirty = true;
        runtime.availability = DataBindingAvailability::Ready;
    }
    let count = view.len();
    let rows_changed = runtime.rows.len() != count
        || runtime
            .rows
            .iter()
            .zip(view.rows())
            .any(|(stored, row)| *stored != row.id);
    runtime.dirty |= rows_changed;
    let row_indices = if rows_changed
        && runtime
            .columns
            .iter()
            .any(|column| column.interpolation.is_some())
    {
        Some(super::interpolation::correspondence(
            &runtime.rows,
            view.rows().map(|row| row.id),
            by_position,
        ))
    } else {
        None
    };
    for column in &mut runtime.columns {
        if let Some(indices) = &row_indices
            && column.interpolation.is_some()
        {
            super::interpolation::reorder(column, indices);
        }
        if column.values.len() != count {
            runtime.dirty = true;
        }
        column.values.resize(
            count,
            ExpressionResult::Invalid(ExpressionInvalid::Calculation),
        );
        let interpolated = column.interpolation.is_some();
        let (values, mut progress) = if let Some(interpolation) = &mut column.interpolation {
            interpolation.targets.resize(
                count,
                ExpressionResult::Invalid(ExpressionInvalid::Calculation),
            );
            (
                &mut interpolation.targets,
                Some((&mut interpolation.active_rows, &column.values)),
            )
        } else {
            (&mut column.values, None)
        };
        if let Some(index) = column.raw_identity {
            // Only an exact one-node raw-column identity reaches this path.
            // Data Service admitted each complete value against this schema;
            // preserve its bits and keep the existing owned output materialization.
            for (row_index, (stored, row)) in values.iter_mut().zip(view.rows()).enumerate() {
                let value = &row.values[index];
                let changed =
                    !matches!(stored, ExpressionResult::Valid(previous) if previous == value);
                if changed && let Some((progress, displayed)) = &mut progress {
                    super::interpolation::retarget_progress(
                        progress,
                        row_index,
                        &displayed[row_index],
                        stored,
                        &ExpressionResult::Valid(value.clone()),
                    );
                }
                runtime.dirty |= !interpolated && changed;
                *stored = ExpressionResult::Valid(value.clone());
            }
            runtime.dirty |= super::interpolation::retarget(column);
            continue;
        }

        column.parameters.clear();
        column
            .parameters
            .extend(column.inputs.iter().map(|input| match input {
                InputAccess::Parameter(descriptor) => {
                    descriptor.and_then(|descriptor| properties.get_descriptor(descriptor))
                }
                InputAccess::Column(_) => None,
            }));
        // Borrowed row inputs cannot be retained across service mutation; allocate
        // this reference list once per column, reuse it for every selected row.
        let mut inputs = vec![None; column.inputs.len()];
        for (row_index, (stored, row)) in values.iter_mut().zip(view.rows()).enumerate() {
            for ((slot, access), parameter) in inputs
                .iter_mut()
                .zip(&column.inputs)
                .zip(&column.parameters)
            {
                *slot = match access {
                    InputAccess::Column(index) => row.values.get(*index),
                    InputAccess::Parameter(_) => parameter.as_ref(),
                };
            }
            // Schema and descriptors were resolved at preparation. Calculation
            // invalidity remains a typed result, never a zero or previous sample.
            let result = column
                .plan
                .evaluate(&mut column.scratch, &inputs)
                .expect("prepared row inputs");
            runtime.dirty |= !interpolated && *stored != *result;
            if stored != result
                && let Some((progress, displayed)) = &mut progress
            {
                super::interpolation::retarget_progress(
                    progress,
                    row_index,
                    &displayed[row_index],
                    stored,
                    result,
                );
            }
            stored.clone_from(result);
        }
        // Parameter values are temporary input scratch, never a property mirror.
        column.parameters.clear();
        runtime.dirty |= super::interpolation::retarget(column);
    }
    runtime.rows.resize(count, DataRowId(0));
    for (stored, row) in runtime.rows.iter_mut().zip(view.rows()) {
        *stored = row.id;
    }
    runtime.evaluated_tick = Some(tick);
    runtime.needs_evaluate = false;
    super::interpolation::advance_or_defer(runtime, properties, tick, dt);
}

fn prepare(
    properties: &DynamicProperties,
    runtime: &mut DataBindingRuntime,
    schema: &DataSchema,
    assets: &AssetManagementService,
    world: crate::WorldId,
) -> Result<(), DataBindingUnavailable> {
    let mut prepared = Vec::new();
    for (name, descriptor) in properties.descriptors() {
        if descriptor.kind != DynamicPropertyKind::Asset {
            continue;
        }
        let name = name.to_string();
        let source = properties.asset(&name).expect("asset descriptor");
        let key = source_key_from_fields(
            assets,
            world,
            None,
            EXPRESSION_TYPE,
            &source.uri,
            source.variant,
        )
        .ok_or_else(|| DataBindingUnavailable::MissingAsset {
            output: name.clone(),
        })?;
        let asset = assets
            .get(key)
            .and_then(|provider| provider.data())
            .and_then(|asset| asset.decoded().downcast_ref::<ExpressionAsset>())
            .ok_or_else(|| DataBindingUnavailable::MissingAsset {
                output: name.clone(),
            })?;
        let plan = asset.prepared().clone();
        let mut inputs = Vec::with_capacity(plan.input_slots().len());
        for input in plan.input_slots() {
            let access = if let Some(raw) = input.name.strip_prefix("column:") {
                let index = schema.column_index(raw).ok_or_else(|| {
                    DataBindingUnavailable::MissingInput {
                        output: name.clone(),
                        input: input.name.clone(),
                    }
                })?;
                if schema.columns[index].kind != input.kind {
                    return Err(DataBindingUnavailable::InputType {
                        output: name.clone(),
                        input: input.name.clone(),
                    });
                }
                InputAccess::Column(index)
            } else if input.name == "parameter" {
                let descriptor = properties
                    .descriptors()
                    .get(format!("{name}_parameter").as_str())
                    .copied();
                if descriptor.is_some_and(|descriptor| descriptor.kind != input.kind) {
                    return Err(DataBindingUnavailable::InputType {
                        output: name.clone(),
                        input: input.name.clone(),
                    });
                }
                InputAccess::Parameter(descriptor)
            } else {
                return Err(DataBindingUnavailable::InvalidInputName {
                    output: name.clone(),
                    input: input.name.clone(),
                });
            };
            inputs.push(access);
        }
        let raw_identity = match (
            asset.declaration().nodes.as_slice(),
            asset.declaration().output,
            inputs.as_slice(),
        ) {
            ([ExpressionNode::Input(0)], 0, [InputAccess::Column(index)]) => Some(*index),
            _ => None,
        };

        let interpolation_speed = properties
            .descriptors()
            .get(format!("{name}_interp").as_str())
            .copied();
        let percentage = properties
            .descriptors()
            .get(format!("{name}_interp_percent").as_str())
            .copied();
        let reference = properties
            .descriptors()
            .get(format!("{name}_interp_reference").as_str())
            .copied();
        let rate = interpolation_speed
            .map(ColumnInterpolationRate::Fixed)
            .or_else(|| {
                percentage.map(|percentage| ColumnInterpolationRate::Percent {
                    percentage,
                    reference,
                })
            });
        if rate.is_some()
            && !matches!(
                plan.output_kind(),
                DynamicPropertyKind::F32
                    | DynamicPropertyKind::Vec2
                    | DynamicPropertyKind::Vec3
                    | DynamicPropertyKind::Vec4
            )
        {
            return Err(DataBindingUnavailable::InputType {
                output: name.clone(),
                input: if interpolation_speed.is_some() {
                    format!("{name}_interp")
                } else {
                    format!("{name}_interp_percent")
                },
            });
        }
        let previous = runtime
            .columns
            .iter_mut()
            .find(|old| old.name == name && old.kind == plan.output_kind() && old.asset == key);
        let (values, previous_interpolation) = previous
            .map(|old| (std::mem::take(&mut old.values), old.interpolation.take()))
            .unwrap_or_default();
        let interpolation = rate.map(|rate| {
            let mut interpolation =
                previous_interpolation.unwrap_or_else(|| super::interpolation::new(rate));
            interpolation.rate = rate;
            interpolation
        });
        prepared.push(PreparedColumn {
            name,
            kind: plan.output_kind(),
            asset: key,
            scratch: plan.scratch(),
            raw_identity,
            parameters: Vec::<Option<DynamicValue>>::with_capacity(inputs.len()),
            inputs,
            plan,
            values,
            interpolation,
        });
    }
    runtime.columns = prepared;
    Ok(())
}
