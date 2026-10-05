//! Consumer references are borrowed inputs for exactly one pending World frame.

use super::{DataBindingPresentationConsumer, DataBindingRuntime, interpolation};
use crate::{DynamicProperties, ErrorReason, systems::SystemUpdateContext};

/// Outputs requiring a consumer reference for current pending interpolation work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataBindingInterpolationRequest {
    /// Named percentage outputs without an authored numeric reference.
    pub outputs: Vec<String>,
}

/// One temporary numeric reference, in the output's projected units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DataBindingInterpolationReference<'a> {
    /// Exact output name from the pending request.
    pub output: &'a str,
    /// Finite nonnegative magnitude. Zero holds without earning motion.
    pub maximum: f64,
}

/// Reference failures never mutate displayed values or consume the pending step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataBindingInterpolationError {
    /// The registered World, binding or consumer lifetime is no longer live.
    Consumer(ErrorReason),
    /// A pending output has no supplied reference.
    MissingReference {
        /// Exact pending output.
        output: String,
    },
    /// More than one reference names the same output.
    DuplicateReference {
        /// Repeated output name.
        output: String,
    },
    /// A reference names an output that does not need a consumer reference.
    UnexpectedReference {
        /// Unrequested output name.
        output: String,
    },
    /// A reference is negative or nonfinite.
    InvalidReference {
        /// Output carrying the invalid reference.
        output: String,
    },
}

impl SystemUpdateContext<'_, '_> {
    /// Observe pending work after projection and before presentation's dirty skip.
    /// Settled/fixed-only bindings return None without allocating a request.
    pub fn data_binding_interpolation_request(
        &self,
        consumer: DataBindingPresentationConsumer,
    ) -> Result<Option<DataBindingInterpolationRequest>, ErrorReason> {
        self.world.validate_data_binding_consumer(consumer)?;
        let index = consumer.entity.index() as usize;
        let runtime =
            if consumer.binding_component == crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING {
                &self
                    .world
                    .world
                    .components
                    .buffer_data_source_binding(index)
                    .ok_or(ErrorReason::MissingComponent)?
                    .runtime
            } else {
                &self
                    .world
                    .world
                    .components
                    .streaming_data_source_binding(index)
                    .ok_or(ErrorReason::MissingComponent)?
                    .runtime
            };
        if runtime.pending_interpolation != Some(self.world.world.tick.saturating_add(1)) {
            return Ok(None);
        }
        Ok(Some(DataBindingInterpolationRequest {
            outputs: runtime
                .columns
                .iter()
                .filter(|column| interpolation::needs_reference(column))
                .map(|column| column.name.clone())
                .collect(),
        }))
    }

    /// Advance the shared displayed view using this frame's Host delta at most once.
    /// End all pre-step view borrows first. All references validate before mutation.
    /// This API exists only in an active System callback, never on WorldContext/client.
    pub fn advance_data_binding_interpolation(
        &mut self,
        consumer: DataBindingPresentationConsumer,
        references: &[DataBindingInterpolationReference<'_>],
    ) -> Result<bool, DataBindingInterpolationError> {
        self.world
            .validate_data_binding_consumer(consumer)
            .map_err(DataBindingInterpolationError::Consumer)?;
        let tick = self.world.world.tick.saturating_add(1);
        let dt = self.dt();
        let index = consumer.entity.index() as usize;
        if consumer.binding_component == crate::ComponentValue::BUFFER_DATA_SOURCE_BINDING {
            let binding = self
                .world
                .world
                .components
                .buffer_data_source_binding_mut(index)
                .ok_or(DataBindingInterpolationError::Consumer(
                    ErrorReason::MissingComponent,
                ))?;
            advance(
                &binding.properties,
                &mut binding.runtime,
                references,
                tick,
                dt,
            )
        } else {
            let binding = self
                .world
                .world
                .components
                .streaming_data_source_binding_mut(index)
                .ok_or(DataBindingInterpolationError::Consumer(
                    ErrorReason::MissingComponent,
                ))?;
            advance(
                &binding.properties,
                &mut binding.runtime,
                references,
                tick,
                dt,
            )
        }
    }
}

fn advance(
    properties: &DynamicProperties,
    runtime: &mut DataBindingRuntime,
    references: &[DataBindingInterpolationReference<'_>],
    tick: u64,
    dt: f64,
) -> Result<bool, DataBindingInterpolationError> {
    if runtime.pending_interpolation != Some(tick) {
        return Ok(false);
    }
    for (index, reference) in references.iter().enumerate() {
        let error =
            if references[..index]
                .iter()
                .any(|previous| previous.output == reference.output)
            {
                Some(DataBindingInterpolationError::DuplicateReference {
                    output: reference.output.into(),
                })
            } else if !runtime.columns.iter().any(|column| {
                column.name == reference.output && interpolation::needs_reference(column)
            }) {
                Some(DataBindingInterpolationError::UnexpectedReference {
                    output: reference.output.into(),
                })
            } else if !reference.maximum.is_finite() || reference.maximum < 0.0 {
                Some(DataBindingInterpolationError::InvalidReference {
                    output: reference.output.into(),
                })
            } else {
                None
            };
        if let Some(error) = error {
            return Err(error);
        }
    }
    for column in runtime
        .columns
        .iter()
        .filter(|column| interpolation::needs_reference(column))
    {
        if !references
            .iter()
            .any(|reference| reference.output == column.name)
        {
            return Err(DataBindingInterpolationError::MissingReference {
                output: column.name.clone(),
            });
        }
    }
    runtime.pending_interpolation = None;
    let mut changed = false;
    for column in &mut runtime.columns {
        let reference = references
            .iter()
            .find(|reference| reference.output == column.name)
            .map(|reference| reference.maximum);
        changed |= interpolation::advance(column, properties, dt, reference);
    }
    runtime.dirty |= changed;
    runtime.evaluated_tick = Some(tick);
    Ok(changed)
}

#[cfg(test)]
#[path = "interpolation_step_tests.rs"]
mod tests;
