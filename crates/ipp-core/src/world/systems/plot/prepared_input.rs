//! Full borrowed preparation input; never the bounded client observation page.

use crate::expressions::ExpressionResult;
use crate::services::data::DataRowId;
use crate::systems::data_bindings::DataBindingColumnView;
use crate::{DynamicPropertyKind, DynamicValue, ErrorReason};

/// Every selected row and evaluated output of one ready entity-local binding.
/// A preparation function borrows this cut only for its call; no raw-source reads
/// or expression reevaluation are permitted in chart algorithms.
pub struct PlotPreparedInput<'a> {
    /// Stable source identities aligned with all output columns.
    pub row_ids: &'a [DataRowId],
    /// Borrowed full output columns, in property-name order.
    pub columns: Vec<DataBindingColumnView<'a>>,
}

impl PlotPreparedInput<'_> {
    /// Resolve a named output once per series, then index this borrowed handle per row.
    pub fn bind(&self, name: &str) -> Result<PlotPreparedColumn<'_>, ErrorReason> {
        self.columns
            .iter()
            .find(|column| column.name == name)
            .map(|column| PlotPreparedColumn {
                kind: column.kind,
                values: column.values,
            })
            .ok_or(ErrorReason::InvalidField)
    }

    /// Select a complete evaluated output. Resolve once per series, outside row loops.
    /// An absent selector is an authoring error.
    pub fn column(&self, name: &str) -> Result<&[ExpressionResult], ErrorReason> {
        self.columns
            .iter()
            .find(|column| column.name == name)
            .map(|column| column.values)
            .ok_or(ErrorReason::InvalidField)
    }

    /// Read one finite numeric value; explicit invalid results remain missing marks.
    pub fn number(&self, name: &str, row: usize) -> Result<Option<f64>, ErrorReason> {
        let value = self
            .column(name)?
            .get(row)
            .ok_or(ErrorReason::InvalidValue)?;
        Ok(match value {
            ExpressionResult::Valid(DynamicValue::F32(value)) => Some(f64::from(*value)),
            ExpressionResult::Valid(DynamicValue::I32(value)) => Some(f64::from(*value)),
            ExpressionResult::Valid(DynamicValue::U32(value)) => Some(f64::from(*value)),
            ExpressionResult::Invalid(_) => None,
            _ => return Err(ErrorReason::InvalidValue),
        })
    }

    /// Read per-row colour or the constant series colour when selector is empty.
    pub fn color(
        &self,
        name: &str,
        row: usize,
        fallback: [f32; 4],
    ) -> Result<Option<[f32; 4]>, ErrorReason> {
        if name.is_empty() {
            return Ok(Some(fallback));
        }
        let value = self
            .column(name)?
            .get(row)
            .ok_or(ErrorReason::InvalidValue)?;
        Ok(match value {
            ExpressionResult::Valid(DynamicValue::Vec3(value)) => {
                Some([value[0], value[1], value[2], 1.0])
            }
            ExpressionResult::Valid(DynamicValue::Vec4(value)) => Some(*value),
            ExpressionResult::Invalid(_) => None,
            _ => return Err(ErrorReason::InvalidValue),
        })
    }
}

/// Resolved row-aligned typed column. Binding names never need lookup in a sample loop.
#[derive(Clone, Copy)]
pub struct PlotPreparedColumn<'a> {
    /// Exact evaluated output kind.
    pub kind: DynamicPropertyKind,
    /// Full completed values; invalid samples preserve gaps.
    pub values: &'a [ExpressionResult],
}

impl PlotPreparedColumn<'_> {
    /// Read one finite number; explicit invalid results are absent, never zero.
    pub fn number(&self, row: usize) -> Result<Option<f64>, ErrorReason> {
        let value = self.values.get(row).ok_or(ErrorReason::InvalidValue)?;
        let number = match value {
            ExpressionResult::Valid(DynamicValue::F32(value)) => f64::from(*value),
            ExpressionResult::Valid(DynamicValue::I32(value)) => f64::from(*value),
            ExpressionResult::Valid(DynamicValue::U32(value)) => f64::from(*value),
            ExpressionResult::Invalid(_) => return Ok(None),
            _ => return Err(ErrorReason::InvalidValue),
        };
        Ok(number.is_finite().then_some(number))
    }

    /// Read one linear colour; invalid calculations omit the affected mark.
    pub fn color(&self, row: usize) -> Result<Option<[f32; 4]>, ErrorReason> {
        let color = match self.values.get(row).ok_or(ErrorReason::InvalidValue)? {
            ExpressionResult::Valid(DynamicValue::Vec3(value)) => {
                [value[0], value[1], value[2], 1.0]
            }
            ExpressionResult::Valid(DynamicValue::Vec4(value)) => *value,
            ExpressionResult::Invalid(_) => return Ok(None),
            _ => return Err(ErrorReason::InvalidValue),
        };
        Ok(color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            .then_some(color))
    }
}
