use super::runtime_state::{
    ColumnInterpolation, ColumnInterpolationRate, ColumnInterpolationRow, DataBindingRuntime,
    PreparedColumn,
};
use crate::{
    DynamicProperties, DynamicValue, expressions::ExpressionResult, services::data::DataRowId,
};

pub(super) fn retarget(column: &mut PreparedColumn) -> bool {
    let Some(interpolation) = &mut column.interpolation else {
        return false;
    };
    interpolation.active_rows.retain(|row| {
        matches!(
            (&column.values[row.index], &interpolation.targets[row.index]),
            (ExpressionResult::Valid(current), ExpressionResult::Valid(target)) if current != target
        )
    });
    let retained = interpolation.active_rows.len();
    let mut changed = false;
    for (row, (displayed, target)) in column
        .values
        .iter_mut()
        .zip(&interpolation.targets)
        .enumerate()
    {
        if matches!(
            (&*displayed, target),
            (ExpressionResult::Valid(_), ExpressionResult::Valid(_))
        ) {
            if displayed != target
                && interpolation.active_rows[..retained]
                    .binary_search_by_key(&row, |active| active.index)
                    .is_err()
            {
                interpolation.active_rows.push(ColumnInterpolationRow {
                    index: row,
                    residual: [0.0; 4],
                });
            }
        } else {
            changed |= displayed != target;
            displayed.clone_from(target);
        }
    }
    if interpolation.active_rows.len() > retained {
        interpolation
            .active_rows
            .sort_unstable_by_key(|row| row.index);
    }
    changed
}

pub(super) fn advance(
    column: &mut PreparedColumn,
    properties: &DynamicProperties,
    dt: f64,
    reference: Option<f64>,
) -> bool {
    let Some(interpolation) = &mut column.interpolation else {
        return false;
    };
    if interpolation.active_rows.is_empty() || dt == 0.0 {
        return false;
    }
    let scalar = |descriptor| match properties.get_descriptor(descriptor) {
        Some(DynamicValue::F32(value)) => f64::from(value),
        _ => unreachable!("prepared interpolation property"),
    };
    let speed = match interpolation.rate {
        ColumnInterpolationRate::Fixed(speed) => scalar(speed),
        ColumnInterpolationRate::Percent {
            percentage,
            reference: explicit,
        } => {
            scalar(percentage)
                * explicit
                    .map(scalar)
                    .or(reference)
                    .expect("resolved reference")
                / 100.0
        }
    };
    if speed == 0.0 {
        // A zero reference holds the authoritative display and earns no motion.
        return false;
    }
    let step = speed * dt;
    let mut changed = false;
    interpolation.active_rows.retain_mut(|row| {
        let displayed = &mut column.values[row.index];
        let target = &interpolation.targets[row.index];
        let (ExpressionResult::Valid(current), ExpressionResult::Valid(target)) =
            (displayed, target)
        else {
            return false;
        };
        changed |= approach(current, target, step, &mut row.residual);
        current != target
    });
    changed
}

fn approach(
    current: &mut DynamicValue,
    target: &DynamicValue,
    step: f64,
    residual: &mut [f64; 4],
) -> bool {
    fn lane(current: &mut f32, target: f32, step: f64, residual: &mut f64) -> bool {
        let distance = f64::from(target) - f64::from(*current);
        let budget = step + *residual;
        let mut next = if distance.abs() <= budget {
            target
        } else {
            (f64::from(*current) + distance.signum() * budget) as f32
        };
        if (f64::from(next) - f64::from(*current)).abs() > budget {
            // Round toward the display when nearest-F32 rounding would consume
            // movement that the Host has not supplied yet. The remainder keeps
            // sub-ULP progress without changing the authoritative display buffer.
            next = if next == 0.0 {
                f32::from_bits(if *current > 0.0 {
                    1
                } else {
                    0x8000_0001
                })
            } else if (next > *current) == (next > 0.0) {
                f32::from_bits(next.to_bits() - 1)
            } else {
                f32::from_bits(next.to_bits() + 1)
            };
        }
        *residual = if next == target {
            0.0
        } else {
            budget - (f64::from(next) - f64::from(*current)).abs()
        };
        let changed = *current != next;
        *current = next;
        changed
    }

    fn lanes<const N: usize>(
        current: &mut [f32; N],
        target: &[f32; N],
        step: f64,
        residual: &mut [f64; 4],
    ) -> bool {
        let mut changed = false;
        for ((current, target), residual) in current.iter_mut().zip(target).zip(residual) {
            changed |= lane(current, *target, step, residual);
        }
        changed
    }

    match (current, target) {
        (DynamicValue::F32(current), DynamicValue::F32(target)) => {
            lane(current, *target, step, &mut residual[0])
        }
        (DynamicValue::Vec2(current), DynamicValue::Vec2(target)) => {
            lanes(current, target, step, residual)
        }
        (DynamicValue::Vec3(current), DynamicValue::Vec3(target)) => {
            lanes(current, target, step, residual)
        }
        (DynamicValue::Vec4(current), DynamicValue::Vec4(target)) => {
            lanes(current, target, step, residual)
        }
        _ => unreachable!("prepared interpolation kind"),
    }
}

pub(super) fn retarget_progress(
    active_rows: &mut [ColumnInterpolationRow],
    row: usize,
    displayed: &ExpressionResult,
    previous: &ExpressionResult,
    target: &ExpressionResult,
) {
    let Ok(index) = active_rows.binary_search_by_key(&row, |active| active.index) else {
        return;
    };
    let residual = &mut active_rows[index].residual;
    let (
        ExpressionResult::Valid(displayed),
        ExpressionResult::Valid(previous),
        ExpressionResult::Valid(target),
    ) = (displayed, previous, target)
    else {
        *residual = [0.0; 4];
        return;
    };
    let lanes = displayed.floats().expect("prepared interpolation kind");
    let previous = previous.floats().expect("prepared interpolation kind");
    let target = target.floats().expect("prepared interpolation kind");
    for (((residual, &displayed), &previous), &target) in
        residual.iter_mut().zip(lanes).zip(previous).zip(target)
    {
        // Unrepresented movement remains earned toward a target on the same
        // side of the authoritative display. Reversing or settling a lane
        // discards that budget, independently of changes in other lanes.
        if target == displayed || (previous > displayed) != (target > displayed) {
            *residual = 0.0;
        }
    }
}

/// Each current row's previous display index. Identity reconciliation follows
/// exact row lifetimes. Positional reconciliation keeps every slot that persists
/// in the binding's view, so its display retargets toward whichever row now
/// occupies it; appearing slots have no predecessor and initialize immediately.
pub(super) fn correspondence(
    previous: &[DataRowId],
    current: impl Iterator<Item = DataRowId>,
    by_position: bool,
) -> Vec<Option<usize>> {
    if by_position {
        return current
            .enumerate()
            .map(|(slot, _)| (slot < previous.len()).then_some(slot))
            .collect();
    }

    let previous: std::collections::BTreeMap<_, _> = previous
        .iter()
        .enumerate()
        .map(|(index, &id)| (id, index))
        .collect();
    current.map(|id| previous.get(&id).copied()).collect()
}

pub(super) fn reorder(column: &mut PreparedColumn, indices: &[Option<usize>]) {
    let Some(interpolation) = &mut column.interpolation else {
        return;
    };
    let previous = std::mem::take(&mut column.values);
    let targets = std::mem::take(&mut interpolation.targets);
    let mut positions = vec![None; previous.len()];
    for (index, old) in indices.iter().enumerate() {
        if let Some(old) = old
            && let Some(position) = positions.get_mut(*old)
        {
            *position = Some(index);
        }
        let invalid =
            || ExpressionResult::Invalid(crate::expressions::ExpressionInvalid::Calculation);
        column.values.push(
            old.and_then(|old| previous.get(old))
                .cloned()
                .unwrap_or_else(invalid),
        );
        interpolation.targets.push(
            old.and_then(|old| targets.get(old))
                .cloned()
                .unwrap_or_else(invalid),
        );
    }
    interpolation.active_rows.retain_mut(|row| {
        if let Some(Some(index)) = positions.get(row.index) {
            row.index = *index;
            true
        } else {
            false
        }
    });
    interpolation
        .active_rows
        .sort_unstable_by_key(|row| row.index);
}

pub(super) fn new(rate: ColumnInterpolationRate) -> ColumnInterpolation {
    ColumnInterpolation {
        rate,
        targets: Vec::new(),
        active_rows: Vec::new(),
    }
}

pub(super) fn needs_reference(column: &PreparedColumn) -> bool {
    column.interpolation.as_ref().is_some_and(|interpolation| {
        interpolation.rate.implicit() && !interpolation.active_rows.is_empty()
    })
}

pub(super) fn advance_or_defer(
    runtime: &mut DataBindingRuntime,
    properties: &DynamicProperties,
    tick: u64,
    dt: f64,
) {
    runtime.pending_interpolation = None;
    if dt <= 0.0 {
        return;
    }
    if runtime.columns.iter().any(needs_reference) {
        runtime.pending_interpolation = Some(tick);
        return;
    }
    let mut active = false;
    for column in &mut runtime.columns {
        if column
            .interpolation
            .as_ref()
            .is_some_and(|interpolation| !interpolation.active_rows.is_empty())
        {
            // A settled implicit column needs no reference or numerical work.
            active = true;
            runtime.dirty |= advance(column, properties, dt, None);
        }
    }
    if active {
        runtime.evaluated_tick = Some(tick);
    }
}

#[cfg(test)]
#[path = "interpolation_tests.rs"]
mod tests;
