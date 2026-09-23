//! Control revisions and committed text-input text.
//!
//! Committed checkbox and slider values are `node_data` row properties of the
//! root; this record holds what rows cannot: the revision fence of every
//! control and the committed string of a text input.

use std::collections::BTreeMap;

use super::nodes::{
    GuiControlValue, GuiNodeData, GuiNodeId, GuiNodes, MAX_NODES, MAX_TEXT_BYTES, put_string,
    put_u32, read_string, read_u32, take,
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
    /// Filled track from the rail's left edge to the committed thumb center.
    ///
    /// The track spans the whole control rectangle, so the fill starts where
    /// the track starts rather than at the first thumb center; at the minimum
    /// value it still reaches under half the thumb.
    pub(crate) fn fill_rect(self, fraction: f32, height: f32) -> Option<[f32; 4]> {
        if !fraction.is_finite() || !height.is_finite() || height <= 0.0 {
            return None;
        }
        let half_thumb = self.thumb_edge * 0.5;
        Some([
            self.center_min - half_thumb,
            self.top + (self.height - height) * 0.5,
            half_thumb + fraction.clamp(0.0, 1.0) * (self.center_max - self.center_min),
            height,
        ])
    }

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

/// One committed control value and the revision that produced it, as read
/// from the root: checkbox and slider values come from `node_data` rows and
/// text from the control record.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlState {
    /// Committed value; None while a former control node has non-control data.
    pub value: GuiControlValue,
    /// Monotonic revision; insertion commits revision 1.
    pub revision: u32,
}

/// Control record of one node: its revision and, for a text input, the
/// committed text.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiControlEntry {
    /// Monotonic revision; insertion commits revision 1.
    pub revision: u32,
    /// Committed text; present exactly for text-input nodes.
    pub text: Option<String>,
}

/// Control records of one root, keyed by node identity.
///
/// Every checkbox, slider and text-input node has an entry. A node whose
/// data stops being a control keeps its entry, so its revision stays
/// monotonic for the whole node lifetime and a stale write cannot apply to a
/// later control on the same node. Entries are stored with the node tree, so
/// only the GUI System changes them for a live root incarnation; generic
/// field writes may supply them only with a new one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiControls {
    values: BTreeMap<GuiNodeId, GuiControlEntry>,
}

impl GuiControls {
    /// Control record of one node.
    pub fn get(&self, id: GuiNodeId) -> Option<&GuiControlEntry> {
        self.values.get(&id)
    }

    /// Control records in node identity order.
    pub fn iter(&self) -> impl Iterator<Item = (GuiNodeId, &GuiControlEntry)> {
        self.values.iter().map(|(id, entry)| (*id, entry))
    }

    /// Start a newly inserted control node at revision 1.
    pub(in crate::world::systems::gui) fn insert_initial(
        &mut self,
        id: GuiNodeId,
        data: &GuiNodeData,
    ) {
        if data.is_control() {
            self.values.insert(
                id,
                GuiControlEntry {
                    revision: 1,
                    text: initial_text(data),
                },
            );
        }
    }

    /// Establish `data`'s initial control state after a kind change: the next
    /// revision for a node that already has a record, revision 1 for a node
    /// becoming a control for the first time.
    pub(in crate::world::systems::gui) fn restart(
        &mut self,
        id: GuiNodeId,
        data: &GuiNodeData,
    ) -> Result<(), FieldError> {
        match self.values.get_mut(&id) {
            Some(entry) => {
                entry.revision = next_revision(entry.revision)?;
                entry.text = initial_text(data);
            }
            None => self.insert_initial(id, data),
        }
        Ok(())
    }

    /// Advance a control's revision when the caller observed the current one.
    pub(in crate::world::systems::gui) fn advance(
        &mut self,
        id: GuiNodeId,
        expected_revision: u32,
    ) -> Result<&mut GuiControlEntry, FieldError> {
        let entry = self.values.get_mut(&id).ok_or(FieldError::WrongType)?;
        if entry.revision != expected_revision {
            return Err(FieldError::WrongType);
        }
        entry.revision = next_revision(entry.revision)?;
        Ok(entry)
    }

    /// Drop the record of a removed node.
    pub(in crate::world::systems::gui) fn remove(&mut self, id: GuiNodeId) {
        self.values.remove(&id);
    }

    /// Every control node has a record, text is present exactly for text
    /// inputs, and every record belongs to a live node.
    pub(in crate::world::systems::gui) fn validate_for(
        &self,
        nodes: &GuiNodes,
    ) -> Result<(), FieldError> {
        for node in nodes.as_slice() {
            match self.values.get(&node.id) {
                Some(entry) => {
                    let text_input = matches!(node.data, GuiNodeData::TextInput { .. });
                    if entry.text.is_some() != text_input
                        || entry
                            .text
                            .as_ref()
                            .is_some_and(|text| text.len() > MAX_TEXT_BYTES)
                    {
                        return Err(FieldError::WrongType);
                    }
                }
                None if node.data.is_control() => return Err(FieldError::WrongType),
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
        for (id, entry) in &self.values {
            put_u32(output, id.0);
            put_u32(output, entry.revision);
            match &entry.text {
                None => output.push(0),
                Some(text) => {
                    output.push(1);
                    put_string(output, text);
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
            let text = match take(input, 1)?[0] {
                0 => None,
                1 => Some(read_string(input)?),
                _ => return Err(FieldError::WrongType),
            };
            values.insert(
                GuiNodeId(id),
                GuiControlEntry {
                    revision,
                    text,
                },
            );
        }
        Ok(Self {
            values,
        })
    }
}

fn initial_text(data: &GuiNodeData) -> Option<String> {
    match data {
        GuiNodeData::TextInput {
            text,
            ..
        } => Some(text.clone()),
        _ => None,
    }
}

fn next_revision(revision: u32) -> Result<u32, FieldError> {
    revision.checked_add(1).ok_or(FieldError::WrongType)
}
