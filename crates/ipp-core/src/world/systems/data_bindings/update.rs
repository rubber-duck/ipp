use super::runtime::*;
use crate::world::systems::asset_dependencies::source_key_from_fields;
use crate::{
    DynamicProperties, DynamicPropertyKind, DynamicValue,
    expressions::*,
    services::{
        asset_management::{
            AssetManagementService,
            expression::{EXPRESSION_TYPE, ExpressionAsset},
        },
        data::*,
    },
};

pub(super) fn evaluate(
    properties: &DynamicProperties,
    runtime: &mut DataBindingRuntime,
    data: &DataService,
    assets: &AssetManagementService,
    world: crate::WorldId,
    tick: u64,
) {
    if !runtime.needs_prepare && !runtime.needs_evaluate {
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
    runtime.source = Some(view.source());
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
    if runtime.rows.len() != count {
        runtime.dirty = true;
    }
    runtime.rows.resize(count, DataRowId(0));
    for (stored, row) in runtime.rows.iter_mut().zip(view.rows()) {
        runtime.dirty |= *stored != row.id;
        *stored = row.id;
    }
    for column in &mut runtime.columns {
        if column.values.len() != count {
            runtime.dirty = true;
        }
        column.values.resize(
            count,
            ExpressionResult::Invalid(ExpressionInvalid::Calculation),
        );
        if let Some(index) = column.raw_identity {
            // Only an exact one-node raw-column identity reaches this path.
            // Data Service admitted each complete value against this schema;
            // preserve its bits and keep the existing owned output materialization.
            for (stored, row) in column.values.iter_mut().zip(view.rows()) {
                let value = &row.values[index];
                runtime.dirty |=
                    !matches!(stored, ExpressionResult::Valid(previous) if previous == value);
                *stored = ExpressionResult::Valid(value.clone());
            }
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
        for (stored, row) in column.values.iter_mut().zip(view.rows()) {
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
            runtime.dirty |= *stored != *result;
            stored.clone_from(result);
        }
        // Parameter values are temporary input scratch, never a property mirror.
        column.parameters.clear();
    }
    runtime.evaluated_tick = Some(tick);
    runtime.needs_evaluate = false;
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

        let values = runtime
            .columns
            .iter_mut()
            .find(|old| old.name == name && old.kind == plan.output_kind())
            .map(|old| std::mem::take(&mut old.values))
            .unwrap_or_default();
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
        });
    }
    runtime.columns = prepared;
    Ok(())
}
