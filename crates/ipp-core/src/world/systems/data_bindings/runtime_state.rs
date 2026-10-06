use crate::{
    DynamicPropertyDescriptor, DynamicPropertyKind, EntityId, expressions::*, services::data::*,
};

/// Binding-wide preparation status; row calculations retain separate explicit validity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataBindingAvailability {
    /// Compatible source and all projection inputs/definitions are prepared.
    Ready,
    /// The completed view cannot currently be evaluated.
    Unavailable(DataBindingUnavailable),
}

/// Observable preparation failure, distinct from a per-row invalid expression result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataBindingUnavailable {
    /// No Host evaluation has prepared this new or restored binding.
    NotEvaluated,
    /// Source resolution, window, kind or service-lifetime failure.
    Source(DataError),
    /// An expression definition has no available CPU payload.
    MissingAsset {
        /// Output property selecting the unavailable definition.
        output: String,
    },
    /// The definition names an absent raw schema column.
    MissingInput {
        /// Output property being prepared.
        output: String,
        /// Declared expression input that cannot resolve.
        input: String,
    },
    /// A resolved input has another exact core value type.
    InputType {
        /// Output property being prepared.
        output: String,
        /// Declared expression input with a type mismatch.
        input: String,
    },
    /// The input is neither `column:<name>` nor `parameter`.
    InvalidInputName {
        /// Output property being prepared.
        output: String,
        /// Unsupported authored expression input name.
        input: String,
    },
}

pub(super) enum InputAccess {
    Column(usize),
    Parameter(Option<DynamicPropertyDescriptor>),
}

pub(super) struct PreparedColumn {
    pub name: String,
    pub kind: DynamicPropertyKind,
    pub asset: crate::services::asset_management::AssetKey,
    pub plan: PreparedExpression,
    pub scratch: ExpressionScratch,
    pub raw_identity: Option<usize>,
    pub inputs: Vec<InputAccess>,
    pub parameters: Vec<Option<crate::DynamicValue>>,
    pub values: Vec<ExpressionResult>,
    pub interpolation: Option<ColumnInterpolation>,
}

pub(super) struct ColumnInterpolation {
    pub rate: ColumnInterpolationRate,
    pub targets: Vec<ExpressionResult>,
    pub active_rows: Vec<ColumnInterpolationRow>,
}

#[derive(Clone, Copy)]
pub(super) enum ColumnInterpolationRate {
    Fixed(DynamicPropertyDescriptor),
    Percent {
        percentage: DynamicPropertyDescriptor,
        reference: Option<DynamicPropertyDescriptor>,
    },
}

impl ColumnInterpolationRate {
    pub fn implicit(self) -> bool {
        matches!(
            self,
            Self::Percent {
                reference: None,
                ..
            }
        )
    }
}

pub(super) struct ColumnInterpolationRow {
    pub index: usize,
    pub residual: [f64; 4],
}

/// Reconstructible state belonging to one occupied binding component incarnation.
/// Cloning authoring state creates fresh runtime; generic schema/persistence excludes it.
#[derive(Debug)]
pub struct DataBindingRuntime {
    pub(super) consumer: Option<DataConsumerHandle>,
    pub(super) source: Option<DataSourceHandle>,
    pub(super) columns: Vec<PreparedColumn>,
    pub(super) rows: Vec<DataRowId>,
    pub(super) availability: DataBindingAvailability,
    pub(super) evaluated_tick: Option<u64>,
    pub(super) dirty: bool,
    pub(super) needs_prepare: bool,
    pub(super) needs_evaluate: bool,
    pub(super) presentation: Option<(EntityId, u16, u64)>,
    // A one-frame work token, never an accumulated clock or an authored-value mirror.
    pub(super) pending_interpolation: Option<u64>,
}

impl std::fmt::Debug for PreparedColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedColumn")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Default for DataBindingRuntime {
    fn default() -> Self {
        Self {
            consumer: None,
            source: None,
            columns: Vec::new(),
            rows: Vec::new(),
            availability: DataBindingAvailability::Unavailable(
                DataBindingUnavailable::NotEvaluated,
            ),
            evaluated_tick: None,
            dirty: true,
            needs_prepare: true,
            needs_evaluate: true,
            presentation: None,
            pending_interpolation: None,
        }
    }
}

impl Clone for DataBindingRuntime {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for DataBindingRuntime {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl DataBindingRuntime {
    /// The sole presentation handoff flag. Observations cannot clear it.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub(super) fn invalidate(&mut self) {
        self.pending_interpolation = None;
        self.needs_prepare = true;
        self.needs_evaluate = true;
    }

    pub(super) fn unavailable(&mut self, reason: DataBindingUnavailable) {
        self.pending_interpolation = None;
        let availability = DataBindingAvailability::Unavailable(reason);
        self.dirty |= self.availability != availability || !self.rows.is_empty();
        self.availability = availability;
        self.rows.clear();
        self.columns.clear();
        self.needs_prepare = true;
    }

    pub(super) fn reset_rows(&mut self) {
        self.pending_interpolation = None;
        self.dirty |= !self.rows.is_empty();
        self.rows.clear();
        for column in &mut self.columns {
            column.values.clear();
            if let Some(interpolation) = &mut column.interpolation {
                interpolation.targets.clear();
                interpolation.active_rows.clear();
            }
        }
    }
}
