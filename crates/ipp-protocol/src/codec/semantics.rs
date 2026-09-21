//! Bounded GUI semantic snapshot and action framing.
//!
//! Snapshots carry revisions alongside values, bounds and states so
//! observe-act loops can detect change and address actions to a known
//! revision. Text values are encoded whole or rejected at the shared GUI
//! source limit; a prefix must never masquerade as an authoritative value.

use super::{ProtocolError, Reader, Writer};
use crate::MAX_MESSAGE_BYTES;
use ipp_core::{
    EntityId, GuiControlValue, GuiNodeId, GuiSemanticAction, GuiSemanticActionKind,
    GuiSemanticActionRequest, GuiSemanticNode, GuiSemanticRole, GuiSemanticSnapshotQuery,
    GuiSemanticTree,
};

impl Reader<'_> {
    pub(super) fn gui_semantic_snapshot_query(
        &mut self,
    ) -> Result<GuiSemanticSnapshotQuery, ProtocolError> {
        let bytes = self.bytes()?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 1 {
            return Err(ProtocolError::Malformed("GUI semantic query version"));
        }
        let entity = EntityId::from_bits(r.u64()?);
        if entity.to_bits() == 0 {
            return Err(ProtocolError::Malformed("GUI semantic entity"));
        }
        let query = GuiSemanticSnapshotQuery {
            entity,
            max_depth: r.u32()?,
            limit: r.u32()?,
        };
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed(
                "trailing GUI semantic query bytes",
            ));
        }
        Ok(query)
    }

    pub(super) fn gui_semantic_action(
        &mut self,
    ) -> Result<GuiSemanticActionRequest, ProtocolError> {
        let bytes = self.bytes_bounded(MAX_MESSAGE_BYTES)?;
        let mut r = Reader {
            bytes: &bytes,
            at: 0,
        };
        if r.u8()? != 2 {
            return Err(ProtocolError::Malformed("GUI semantic action version"));
        }
        let entity = EntityId::from_bits(r.u64()?);
        let root_incarnation = r.u64()?;
        let node = GuiNodeId(r.u32()?);
        if entity.to_bits() == 0 || node.0 == 0 {
            return Err(ProtocolError::Malformed("GUI semantic target"));
        }
        let request = GuiSemanticActionRequest {
            entity,
            root_incarnation,
            node,
            lifetime: r.u32()?,
            expected_revision: r.u32()?,
            action: match r.u8()? {
                0 => GuiSemanticAction::Press,
                1 => GuiSemanticAction::Toggle,
                2 => GuiSemanticAction::SetScalar(r.f32()?),
                3 => GuiSemanticAction::SetText(r.string()?),
                4 => GuiSemanticAction::Focus,
                _ => return Err(ProtocolError::Malformed("GUI semantic action kind")),
            },
        };
        if r.at != bytes.len() {
            return Err(ProtocolError::Malformed(
                "trailing GUI semantic action bytes",
            ));
        }
        Ok(request)
    }
}

impl Writer {
    pub(super) fn gui_semantic_snapshot_response(
        &mut self,
        tree: &GuiSemanticTree,
    ) -> Result<(), ProtocolError> {
        let inner = encode_gui_semantic_snapshot_inner(tree)?;
        self.count(inner.len(), MAX_MESSAGE_BYTES)?;
        self.raw(&inner)?;
        Ok(())
    }
}

/// Inner payload for one correlated snapshot: version, panel identity,
/// bounded nodes, then the observed focus.
fn encode_gui_semantic_snapshot_inner(tree: &GuiSemanticTree) -> Result<Vec<u8>, ProtocolError> {
    if tree.entity.to_bits() == 0 {
        return Err(ProtocolError::Malformed("GUI semantic entity"));
    }
    let mut w = Writer(Vec::new());
    w.u8(1)?;
    w.u64(tree.entity.to_bits())?;
    w.u64(tree.root_incarnation)?;
    w.u64(tree.evaluation_tick)?;
    w.count(tree.nodes.len(), 256)?;
    for node in &tree.nodes {
        write_semantic_node(&mut w, node)?;
    }
    match &tree.focused {
        None => w.u8(0)?,
        Some(focus) => {
            if focus.id.0 == 0 {
                return Err(ProtocolError::Malformed("GUI semantic focus"));
            }
            w.u8(1)?;
            w.u32(focus.id.0)?;
            w.u32(focus.lifetime)?;
        }
    }
    Ok(w.0)
}

fn write_semantic_node(w: &mut Writer, node: &GuiSemanticNode) -> Result<(), ProtocolError> {
    if node.id.0 == 0 {
        return Err(ProtocolError::Malformed("GUI semantic node"));
    }
    w.u32(node.id.0)?;
    w.u32(node.parent.map(|parent| parent.0).unwrap_or(0))?;
    w.u32(node.lifetime)?;
    w.u8(match node.role {
        GuiSemanticRole::Container => 0,
        GuiSemanticRole::Text => 1,
        GuiSemanticRole::Drawing => 2,
        GuiSemanticRole::Image => 3,
        GuiSemanticRole::Button => 4,
        GuiSemanticRole::Checkbox => 5,
        GuiSemanticRole::Slider => 6,
        GuiSemanticRole::TextInput => 7,
    })?;
    match &node.name {
        None => w.u8(0)?,
        Some(name) => {
            w.u8(1)?;
            w.string(name)?;
        }
    }
    match &node.value {
        GuiControlValue::None => w.u8(0)?,
        GuiControlValue::Bool(value) => {
            w.u8(1)?;
            w.u8(u8::from(*value))?;
        }
        GuiControlValue::Scalar(value) => {
            w.u8(2)?;
            w.f32(*value)?;
        }
        GuiControlValue::Text(text) => {
            w.u8(3)?;
            w.string(text)?;
        }
    }
    w.u32(node.revision)?;
    for bound in node.bounds {
        w.f32(bound)?;
    }
    w.u8(u8::from(node.enabled))?;
    w.u8(u8::from(node.visible))?;
    w.u8(u8::from(node.available))?;
    if node.actions.len() > 5 {
        return Err(ProtocolError::Malformed("GUI semantic actions"));
    }
    w.u8(node.actions.len() as u8)?;
    for action in &node.actions {
        w.u8(match action {
            GuiSemanticActionKind::Press => 0,
            GuiSemanticActionKind::Toggle => 1,
            GuiSemanticActionKind::SetScalar => 2,
            GuiSemanticActionKind::SetText => 3,
            GuiSemanticActionKind::Focus => 4,
        })?;
    }
    Ok(())
}
