use crate::components::{DynamicPropertyKind, DynamicValue};

use super::{
    DataConsumerRequest, DataError, DataSourceKind, DataWindow, DataWindowAnchor,
    consumer::DataConsumer,
    source::{DataRow, DataSource},
};

/// Every supported scalar fits exactly in f64; integer comparisons never narrow to f32.
pub(super) fn numeric(value: &DynamicValue) -> Option<f64> {
    match value {
        DynamicValue::F32(value) => Some(f64::from(*value)),
        DynamicValue::I32(value) => Some(f64::from(*value)),
        DynamicValue::U32(value) => Some(f64::from(*value)),
        _ => None,
    }
}

pub(super) fn validate_request(request: &DataConsumerRequest) -> Result<(), DataError> {
    if request.name.is_empty() || request.name.chars().any(char::is_control) {
        return Err(DataError::InvalidName);
    }
    if request.kind == DataSourceKind::Buffer && !request.windows.is_empty() {
        return Err(DataError::InvalidWindow);
    }
    for window in &request.windows {
        if let DataWindow::Range {
            column,
            width,
            anchor,
        } = window
        {
            if column.is_empty() || !width.is_finite() || *width < 0.0 {
                return Err(DataError::InvalidWindow);
            }
            match anchor {
                DataWindowAnchor::Supplied(value) => {
                    if numeric(value).is_none() || value.validate().is_err() {
                        return Err(DataError::InvalidWindow);
                    }
                }
                DataWindowAnchor::HostTime {
                    units_per_second,
                } => {
                    if !units_per_second.is_finite() || *units_per_second <= 0.0 {
                        return Err(DataError::InvalidWindow);
                    }
                }
                DataWindowAnchor::Latest => {}
            }
        }
    }
    Ok(())
}

pub(super) fn validate_source_windows(
    request: &DataConsumerRequest,
    source: &DataSource,
) -> Result<(), DataError> {
    if request.kind != source.kind {
        return Err(DataError::KindMismatch);
    }
    for window in &request.windows {
        if let DataWindow::Range {
            column,
            anchor,
            ..
        } = window
        {
            let index = source
                .schema
                .column_index(column)
                .ok_or(DataError::InvalidColumn)?;
            let kind = source.schema.columns[index].kind;
            if !matches!(
                kind,
                DynamicPropertyKind::F32 | DynamicPropertyKind::I32 | DynamicPropertyKind::U32
            ) {
                return Err(DataError::InvalidColumn);
            }
            if let DataWindowAnchor::Supplied(value) = anchor
                && value.kind() != kind
            {
                return Err(DataError::InvalidWindow);
            }
        }
    }
    Ok(())
}

pub(super) fn validate_forward(
    previous: &DataConsumerRequest,
    next: &DataConsumerRequest,
    source: Option<&DataSource>,
    time: f64,
) -> Result<(), DataError> {
    if previous.name != next.name || previous.kind != next.kind {
        return Ok(());
    }
    for (index, window) in next.windows.iter().enumerate() {
        let DataWindow::Range {
            column,
            anchor,
            ..
        } = window
        else {
            continue;
        };
        // Repeated raw-column constraints are independent windows. Match their
        // occurrence order instead of comparing one anchor with every anchor.
        let occurrence = next.windows[..index]
            .iter()
            .filter(|window| {
                matches!(window,
                    DataWindow::Range { column: candidate, .. } if candidate == column
                )
            })
            .count();
        let prior = previous
            .windows
            .iter()
            .filter(|window| {
                matches!(window,
                    DataWindow::Range { column: candidate, .. } if candidate == column
                )
            })
            .nth(occurrence);
        if let Some(DataWindow::Range {
            anchor: prior_anchor,
            ..
        }) = prior
            && let (Some(next), Some(previous)) = (
                anchor_value(column, anchor, source, time),
                anchor_value(column, prior_anchor, source, time),
            )
            && next < previous
        {
            return Err(DataError::AnchorMovedBackwards);
        }
    }
    Ok(())
}

fn anchor_value(
    column: &str,
    anchor: &DataWindowAnchor,
    source: Option<&DataSource>,
    time: f64,
) -> Option<f64> {
    match anchor {
        DataWindowAnchor::Supplied(value) => numeric(value),
        DataWindowAnchor::Latest => {
            let source = source?;
            source.latest[source.schema.column_index(column)?]
        }
        DataWindowAnchor::HostTime {
            units_per_second,
        } => Some(time * units_per_second),
    }
}

pub(super) fn contains(
    source: &DataSource,
    consumer: &DataConsumer,
    row: &DataRow,
    time: f64,
) -> bool {
    if source.kind == DataSourceKind::Buffer {
        return true;
    }
    if consumer.request.windows.is_empty() {
        return u128::from(row.id.0) >= consumer.default_start;
    }
    contains_parts(
        source.next_row,
        &source.schema,
        &source.latest,
        consumer,
        row,
        time,
    )
}

pub(super) fn contains_parts(
    next_row: u64,
    schema: &super::DataSchema,
    latest: &[Option<f64>],
    consumer: &DataConsumer,
    row: &DataRow,
    time: f64,
) -> bool {
    if consumer.request.windows.is_empty() {
        return u128::from(row.id.0) >= consumer.default_start;
    }
    consumer.request.windows.iter().all(|window| match window {
        DataWindow::Count(count) => row.id.0 > next_row.saturating_sub(*count as u64),
        DataWindow::Range {
            column,
            width,
            anchor,
        } => {
            let index = schema.column_index(column).expect("resolved raw column");
            let value = numeric(&row.values[index]).expect("resolved numeric column");
            let anchor = match anchor {
                DataWindowAnchor::Supplied(value) => numeric(value),
                DataWindowAnchor::Latest => latest[index],
                DataWindowAnchor::HostTime {
                    units_per_second,
                } => Some(time * units_per_second),
            };
            anchor.is_some_and(|anchor| value <= anchor && value >= anchor - width)
        }
    })
}
