//! The World canvas's logical extent and density: Canvas System state, set at
//! World creation and changed by a sparse System command at the mutation boundary.

use crate::ErrorReason;

/// Logical extent and units-per-metre density of a World's canvas.
///
/// A Surface presentation derives the extent from its physical size and this
/// density; a root viewport uses its own CSS-pixel extent and ignores density.
/// The authored extent applies while the canvas is not presented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasState {
    /// Logical width and height, each finite and positive.
    pub extent: [f32; 2],
    /// Logical units per physical Surface metre, finite and positive.
    pub units_per_metre: f32,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            extent: [1.0, 1.0],
            units_per_metre: 1.0,
        }
    }
}

impl CanvasState {
    /// Every value finite and positive; an invalid state has no effect anywhere.
    pub fn validate(self) -> Result<Self, ErrorReason> {
        if [self.extent[0], self.extent[1], self.units_per_metre]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
        {
            Ok(self)
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Sparse Canvas System command. Omitted values keep their current values; an
/// update with an invalid value has no effect and is reported as a diagnostic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasStateUpdate {
    /// Optional replacement logical extent.
    pub extent: Option<[f32; 2]>,
    /// Optional replacement density.
    pub units_per_metre: Option<f32>,
}

impl CanvasStateUpdate {
    /// The state after this update, or the refusal that leaves `current` unchanged.
    pub fn applied_to(self, current: CanvasState) -> Result<CanvasState, ErrorReason> {
        CanvasState {
            extent: self.extent.unwrap_or(current.extent),
            units_per_metre: self.units_per_metre.unwrap_or(current.units_per_metre),
        }
        .validate()
    }
}

/// The one record of the Canvas inspection collection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasStateRecord {
    /// Committed Canvas System state.
    pub state: CanvasState,
    /// Last evaluated logical extent; absent before the first evaluation.
    pub evaluated: Option<CanvasEvaluatedExtent>,
}

/// The logical extent the Canvas System last evaluated with, after presentation
/// constraints, and the tick of that evaluation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasEvaluatedExtent {
    /// Evaluated logical width and height.
    pub extent: [f32; 2],
    /// Frame whose evaluation used this extent.
    pub tick: u64,
}

/// Validate a World creation's Canvas state against its System selection.
pub(crate) fn validate_creation_state(
    state: CanvasState,
    selected: &[crate::systems::SystemId],
) -> Result<(), ErrorReason> {
    state.validate()?;
    let selects_canvas = selected.contains(&super::CanvasSystem::ID);
    if !selects_canvas {
        return Err(ErrorReason::UnsupportedDependency);
    }

    Ok(())
}

/// Encode the Canvas System's persistent payload: extent then density, as
/// little-endian `f32` values.
pub(super) fn encode_persistent(state: CanvasState) -> Vec<u8> {
    [state.extent[0], state.extent[1], state.units_per_metre]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// Decode a persistent payload; a wrong length or an invalid value fails the load.
pub(super) fn decode_persistent(bytes: &[u8]) -> Result<CanvasState, String> {
    let bytes: &[u8; PERSISTENT_BYTES] = bytes
        .try_into()
        .map_err(|_| "Invalid Canvas state length")?;
    let value = |index: usize| {
        let mut word = [0; 4];
        word.copy_from_slice(&bytes[index * 4..index * 4 + 4]);
        f32::from_le_bytes(word)
    };
    CanvasState {
        extent: [value(0), value(1)],
        units_per_metre: value(2),
    }
    .validate()
    .map_err(|_| "Invalid Canvas state value".into())
}

const PERSISTENT_BYTES: usize = 12;

impl crate::WorldContext<'_> {
    /// Queue a sparse Canvas state update at the ordered mutation boundary.
    pub fn enqueue_canvas_state_update(
        &mut self,
        update: CanvasStateUpdate,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(super::CanvasSystem::ID, 0, update)
    }

    /// Committed Canvas System state and the last evaluated extent; refused
    /// for a World that does not select the Canvas System.
    pub fn canvas_state(&self) -> Result<CanvasStateRecord, ErrorReason> {
        let canvas = self
            .system::<super::CanvasSystem>(super::CanvasSystem::ID)
            .ok_or(ErrorReason::UnsupportedDependency)?;
        Ok(CanvasStateRecord {
            state: canvas.state.canvas,
            evaluated: canvas.state.evaluated,
        })
    }
}

impl crate::world::World {
    /// Seed an unpublished World's validated creation state.
    pub(crate) fn seed_canvas_state(&mut self, state: CanvasState) {
        if let Some(canvas) = self.system_mut::<super::CanvasSystem>(super::CanvasSystem::ID) {
            canvas.state.canvas = state;
        }
    }
}

impl crate::systems::SystemRuntimeAccess<'_> {
    /// Committed Canvas System state for GUI layout, which evaluates before the
    /// Canvas System it feeds. Only a command at the mutation boundary changes
    /// this state, so reading it here does not depend on evaluation order.
    pub(in crate::world::systems) fn canvas_state(&self) -> Option<CanvasState> {
        let any: &dyn std::any::Any = self
            .instances
            .before
            .iter()
            .chain(self.instances.after.iter())
            .find(|instance| instance.id == super::CanvasSystem::ID)?
            .system
            .as_ref();
        any.downcast_ref::<super::CanvasSystem>()
            .map(|canvas| canvas.state.canvas)
    }
}
