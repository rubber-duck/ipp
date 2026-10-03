//! Completed entity-local observations. The connection owns session fencing and output credit.

use super::{PAGE_BYTES, PAGE_ROWS, payload::write_values};
use crate::codec::{ProtocolError, Writer};
use ipp_core::systems::{constraints::*, data_bindings::*};
use ipp_core::{EntityId, WorldContext, expressions::*};

fn optional(writer: &mut Writer, value: Option<u64>) -> Result<(), ProtocolError> {
    writer.u8(u8::from(value.is_some()))?;
    writer.u64(value.unwrap_or(0))
}

fn availability(writer: &mut Writer, value: &DataBindingAvailability) -> Result<(), ProtocolError> {
    let (reason, detail, output, input) = match value {
        DataBindingAvailability::Ready => ("Ready", String::new(), "", ""),
        DataBindingAvailability::Unavailable(reason) => match reason {
            DataBindingUnavailable::NotEvaluated => ("NotEvaluated", String::new(), "", ""),
            DataBindingUnavailable::Source(error) => ("Source", error.to_string(), "", ""),
            DataBindingUnavailable::MissingAsset {
                output,
            } => ("MissingAsset", String::new(), output.as_str(), ""),
            DataBindingUnavailable::MissingInput {
                output,
                input,
            } => (
                "MissingInput",
                String::new(),
                output.as_str(),
                input.as_str(),
            ),
            DataBindingUnavailable::InputType {
                output,
                input,
            } => ("InputType", String::new(), output.as_str(), input.as_str()),
            DataBindingUnavailable::InvalidInputName {
                output,
                input,
            } => (
                "InvalidInputName",
                String::new(),
                output.as_str(),
                input.as_str(),
            ),
        },
    };
    for text in [reason, detail.as_str(), output, input] {
        writer.string(text)?;
    }
    Ok(())
}

fn header(
    writer: &mut Writer,
    view: &DataBindingView<'_>,
    count: usize,
) -> Result<(), ProtocolError> {
    optional(writer, view.source.map(|source| source.incarnation()))?;
    writer.u64(view.binding_incarnation)?;
    optional(writer, view.evaluated_tick)?;
    availability(writer, view.availability)?;
    writer.u8(u8::from(view.dirty))?;
    writer.u64(view.total_rows as u64)?;
    writer.u64(view.offset as u64)?;
    optional(
        writer,
        (view.offset + count < view.total_rows).then_some((view.offset + count) as u64),
    )?;
    writer.count(view.columns.len(), super::COLUMNS)?;
    for column in &view.columns {
        writer.string(column.name)?;
        writer.u8(column.kind as u8)?;
    }
    writer.count(count, PAGE_ROWS)
}

fn row(writer: &mut Writer, view: &DataBindingView<'_>, index: usize) -> Result<(), ProtocolError> {
    writer.u64(view.row_ids[index].0)?;
    for column in &view.columns {
        match &column.values[index] {
            ExpressionResult::Valid(value) => {
                writer.u8(1)?;
                write_values(writer, std::slice::from_ref(value))?;
            }
            ExpressionResult::Invalid(reason) => {
                writer.u8(0)?;
                let (name, slot) = match reason {
                    ExpressionInvalid::MissingInput {
                        slot,
                    } => ("MissingInput", Some(*slot as u64)),
                    ExpressionInvalid::InvalidInput {
                        slot,
                    } => ("InvalidInput", Some(*slot as u64)),
                    ExpressionInvalid::Calculation => ("Calculation", None),
                };
                writer.string(name)?;
                optional(writer, slot)?;
            }
        }
    }
    Ok(())
}

/// Observe a completed page without demand, preparation, evaluation or dirty acknowledgement.
/// Bound actual wire bytes before copying, independently of core's owned-query memory budget.
pub fn binding_observation(
    world: &WorldContext<'_>,
    entity: EntityId,
    offset: u64,
    limit: u32,
) -> Result<Vec<u8>, ProtocolError> {
    if limit == 0 || limit as usize > PAGE_ROWS {
        return Err(ProtocolError::Limit("binding page rows"));
    }
    let offset =
        usize::try_from(offset).map_err(|_| ProtocolError::Limit("binding page offset"))?;
    let view = world
        .data_binding_view(
            entity,
            DataBindingViewQuery {
                offset,
                limit: limit as usize,
            },
        )
        .map_err(|_| ProtocolError::Malformed("binding unavailable entity or component"))?;
    encode_view(&view)
}

fn encode_view(view: &DataBindingView<'_>) -> Result<Vec<u8>, ProtocolError> {
    let mut measured = Writer::measuring();
    header(&mut measured, view, 0)?;
    let mut size = measured.len();
    let budget = PAGE_BYTES - 25;
    if size > budget {
        return Err(ProtocolError::Limit("binding page descriptors"));
    }
    let mut count = 0;
    for index in 0..view.row_ids.len() {
        let mut measured = Writer::measuring();
        row(&mut measured, view, index)?;
        if measured.len() > budget - size {
            if count == 0 {
                return Err(ProtocolError::Limit("binding page row"));
            }
            break;
        }
        size += measured.len();
        count += 1;
    }
    let mut writer = Writer::new(Vec::with_capacity(size));
    header(&mut writer, view, count)?;
    for index in 0..count {
        row(&mut writer, view, index)?;
    }
    Ok(writer.0)
}

/// Observe current driver status. Reasons carry stable names and exact optional input slots.
pub fn driver_observation(status: &ExpressionDriverStatus) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = Writer::new(Vec::new());
    writer.string(match status.availability {
        ExpressionDriverAvailability::Pending => "Pending",
        ExpressionDriverAvailability::Ready => "Ready",
        ExpressionDriverAvailability::Unavailable => "Unavailable",
    })?;
    let (state, reason, slot, detail) = match &status.state {
        ExpressionDriverState::Prepared => ("Prepared", "", None, String::new()),
        ExpressionDriverState::Written => ("Written", "", None, String::new()),
        ExpressionDriverState::Retained(reason) => {
            let (name, slot, detail) = match reason {
                ExpressionDriverReason::AssetUnavailable => {
                    ("AssetUnavailable", None, String::new())
                }
                ExpressionDriverReason::AssetFailed => ("AssetFailed", None, String::new()),
                ExpressionDriverReason::InputMapping => ("InputMapping", None, String::new()),
                ExpressionDriverReason::InputType {
                    slot,
                } => ("InputType", Some(u64::from(*slot)), String::new()),
                ExpressionDriverReason::MissingInput {
                    slot,
                } => ("MissingInput", Some(u64::from(*slot)), String::new()),
                ExpressionDriverReason::InvalidInput {
                    slot,
                } => ("InvalidInput", Some(u64::from(*slot)), String::new()),
                ExpressionDriverReason::Calculation => ("Calculation", None, String::new()),
                ExpressionDriverReason::TargetUnavailable => {
                    ("TargetUnavailable", None, String::new())
                }
                ExpressionDriverReason::TargetRejected(error) => {
                    ("TargetRejected", None, error.to_string())
                }
                ExpressionDriverReason::Cycle => ("Cycle", None, String::new()),
            };
            ("Retained", name, slot, detail)
        }
    };
    writer.string(state)?;
    writer.string(reason)?;
    optional(&mut writer, slot)?;
    writer.string(&detail)?;
    writer.u8(u8::from(status.recovered))?;
    Ok(writer.0)
}

#[cfg(test)]
#[path = "observations_tests.rs"]
mod tests;
