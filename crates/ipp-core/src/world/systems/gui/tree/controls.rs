//! Committed control values and their monotonic revisions.

use std::collections::BTreeMap;

use super::nodes::{
    GuiControlValue, GuiNodeContent, GuiNodeId, GuiNodes, MAX_NODES, MAX_TEXT_BYTES,
    initial_control_value_for, put_u32, read_f32, read_u32, take, validate_control_value,
};
use crate::components::schema::FieldError;

/// Slider-thumb edge as a fraction of the retained control height.
const SLIDER_THUMB_EDGE: f32 = 0.75;
/// Upper bound that leaves finite centre travel on narrow controls.
const SLIDER_THUMB_MAX_WIDTH: f32 = 0.75;

/// Shared slider geometry for paint and pointer-to-value routing.
///
/// The contained thumb travels by its center between `center_min` and
/// `center_max`. Keeping that interval in one helper prevents pointer-down at
/// a painted thumb center from changing the committed value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GuiSliderRail {
    thumb_edge: f32,
    center_min: f32,
    center_max: f32,
    top: f32,
    height: f32,
}

impl GuiSliderRail {
    /// Painted thumb rectangle for a normalized committed value.
    pub(crate) fn thumb_rect(self, fraction: f32) -> Option<[f32; 4]> {
        if !fraction.is_finite() {
            return None;
        }
        let center =
            self.center_min + fraction.clamp(0.0, 1.0) * (self.center_max - self.center_min);
        Some([
            center - self.thumb_edge * 0.5,
            self.top + (self.height - self.thumb_edge) * 0.5,
            self.thumb_edge,
            self.thumb_edge,
        ])
    }

    /// Normalized pointer value along the exact painted thumb-center rail.
    pub(crate) fn fraction_at(self, x: f32) -> Option<f32> {
        let travel = self.center_max - self.center_min;
        if !x.is_finite() || !(travel.is_finite() && travel > 0.0) {
            return None;
        }
        Some(((x - self.center_min) / travel).clamp(0.0, 1.0))
    }
}

/// Resolve the finite contained-thumb geometry for one retained slider rect.
pub(crate) fn slider_rail(rect: [f32; 4]) -> Option<GuiSliderRail> {
    if !rect.iter().all(|value| value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return None;
    }
    let thumb_edge = (rect[3] * SLIDER_THUMB_EDGE).min(rect[2] * SLIDER_THUMB_MAX_WIDTH);
    Some(GuiSliderRail {
        thumb_edge,
        center_min: rect[0] + thumb_edge * 0.5,
        center_max: rect[0] + rect[2] - thumb_edge * 0.5,
        top: rect[1],
        height: rect[3],
    })
}

/// One committed control value and the revision that produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlState {
    /// Committed value; None while a former control node has non-control content.
    pub value: GuiControlValue,
    /// Monotonic revision; insertion commits revision 1.
    pub revision: u32,
}

/// Committed values of every control node in one root, keyed by node identity.
///
/// Every checkbox, slider and text-input node has an entry. A node whose
/// content stops being a control keeps its entry with a None value, so its
/// revision stays monotonic for the whole node lifetime and a stale write
/// cannot apply to a later control on the same node. Entries are stored with
/// the node tree, so only the GUI System changes them for a live root
/// incarnation; generic field writes may supply them only with a new one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiControls {
    values: BTreeMap<GuiNodeId, GuiControlState>,
}

impl GuiControls {
    /// Committed state for one control node.
    pub fn get(&self, id: GuiNodeId) -> Option<&GuiControlState> {
        self.values.get(&id)
    }

    /// Committed states in node identity order.
    pub fn iter(&self) -> impl Iterator<Item = (GuiNodeId, &GuiControlState)> {
        self.values.iter().map(|(id, state)| (*id, state))
    }

    /// Start a newly inserted node's value from its content at revision 1.
    pub(in crate::world::systems::gui) fn insert_initial(
        &mut self,
        id: GuiNodeId,
        content: &GuiNodeContent,
    ) {
        let value = initial_control_value_for(content);
        if value != GuiControlValue::None {
            self.values.insert(
                id,
                GuiControlState {
                    value,
                    revision: 1,
                },
            );
        }
    }

    /// Keep a compatible committed value across content edits. A same-kind
    /// control edit that would invalidate runtime-owned state is rejected
    /// without mutation; changing roles establishes the new role's initial
    /// state at the next revision.
    pub(in crate::world::systems::gui) fn reconcile_content(
        &mut self,
        id: GuiNodeId,
        content: &GuiNodeContent,
    ) -> Result<(), FieldError> {
        let Some(state) = self.values.get_mut(&id) else {
            self.insert_initial(id, content);
            return Ok(());
        };
        if validate_control_value(content, &state.value).is_ok() {
            return Ok(());
        }
        let same_control_kind = matches!(
            (&state.value, content),
            (GuiControlValue::Bool(_), GuiNodeContent::Checkbox { .. })
                | (GuiControlValue::Scalar(_), GuiNodeContent::Slider { .. })
                | (GuiControlValue::Text(_), GuiNodeContent::TextInput { .. })
        );
        if same_control_kind {
            return Err(FieldError::WrongType);
        }
        state.value = initial_control_value_for(content);
        state.revision = next_revision(state.revision)?;
        Ok(())
    }

    /// Commit a value when the caller observed the current revision.
    pub(in crate::world::systems::gui) fn set(
        &mut self,
        id: GuiNodeId,
        content: &GuiNodeContent,
        expected_revision: u32,
        value: GuiControlValue,
    ) -> Result<u32, FieldError> {
        if value == GuiControlValue::None {
            return Err(FieldError::WrongType);
        }
        validate_control_value(content, &value)?;
        let state = self.values.get_mut(&id).ok_or(FieldError::WrongType)?;
        if state.revision != expected_revision {
            return Err(FieldError::WrongType);
        }
        state.revision = next_revision(state.revision)?;
        state.value = value;
        Ok(state.revision)
    }

    /// Drop the values of removed nodes.
    pub(in crate::world::systems::gui) fn remove(&mut self, id: GuiNodeId) {
        self.values.remove(&id);
    }

    /// Every control node has one compatible committed value; retained
    /// entries of former controls belong to live nodes and hold None.
    pub(in crate::world::systems::gui) fn validate_for(
        &self,
        nodes: &GuiNodes,
    ) -> Result<(), FieldError> {
        for node in nodes.as_slice() {
            let control = initial_control_value_for(&node.content) != GuiControlValue::None;
            match self.values.get(&node.id) {
                Some(state) => validate_control_value(&node.content, &state.value)?,
                None if control => return Err(FieldError::WrongType),
                None => {}
            }
        }
        if self.values.keys().all(|id| nodes.node(*id).is_some()) {
            Ok(())
        } else {
            Err(FieldError::WrongType)
        }
    }

    pub(in crate::world::systems::gui) fn encode(&self, output: &mut Vec<u8>) {
        put_u32(output, self.values.len() as u32);
        for (id, state) in &self.values {
            put_u32(output, id.0);
            put_u32(output, state.revision);
            match &state.value {
                GuiControlValue::None => output.push(0),
                GuiControlValue::Bool(value) => {
                    output.push(1);
                    output.push(u8::from(*value));
                }
                GuiControlValue::Scalar(value) => {
                    output.push(2);
                    output.extend(value.to_le_bytes());
                }
                GuiControlValue::Text(value) => {
                    output.push(3);
                    put_u32(output, value.len() as u32);
                    output.extend(value.as_bytes());
                }
            }
        }
    }

    pub(in crate::world::systems::gui) fn decode(input: &mut &[u8]) -> Result<Self, FieldError> {
        let count = read_u32(input)? as usize;
        if count > MAX_NODES {
            return Err(FieldError::WrongType);
        }
        let mut values = BTreeMap::new();
        let mut previous = 0;
        for _ in 0..count {
            let id = read_u32(input)?;
            let revision = read_u32(input)?;
            if id <= previous || revision == 0 {
                return Err(FieldError::WrongType);
            }
            previous = id;
            let value = match take(input, 1)?[0] {
                0 => GuiControlValue::None,
                1 => match take(input, 1)?[0] {
                    0 => GuiControlValue::Bool(false),
                    1 => GuiControlValue::Bool(true),
                    _ => return Err(FieldError::WrongType),
                },
                2 => GuiControlValue::Scalar(read_f32(input)?),
                3 => {
                    let length = read_u32(input)? as usize;
                    if length > MAX_TEXT_BYTES {
                        return Err(FieldError::WrongType);
                    }
                    let text = std::str::from_utf8(take(input, length)?)
                        .map_err(|_| FieldError::WrongType)?;
                    GuiControlValue::Text(text.into())
                }
                _ => return Err(FieldError::WrongType),
            };
            values.insert(
                GuiNodeId(id),
                GuiControlState {
                    value,
                    revision,
                },
            );
        }
        Ok(Self {
            values,
        })
    }
}

fn next_revision(revision: u32) -> Result<u32, FieldError> {
    revision.checked_add(1).ok_or(FieldError::WrongType)
}
