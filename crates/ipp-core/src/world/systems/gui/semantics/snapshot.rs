//! Semantic snapshots over the frozen GUI read interfaces.
//!
//! Snapshots combine authoritative inspection (structure, committed values
//! and revisions) with the retained evaluated view (bounds and states) and
//! the observed input focus. Nothing here touches committed state.

use std::collections::BTreeMap;

use super::types::{
    GuiSemanticFocus, GuiSemanticNode, GuiSemanticRole, GuiSemanticTree, actions_for_role,
    name_for_content, role_for_content,
};
use crate::{
    EntityId, GuiEvaluatedContent, GuiEvaluatedView, GuiInputEffectKind, GuiInspectResponse,
    GuiNodeId,
};

/// Build a semantic snapshot from inspection plus evaluated bounds.
///
/// Inspection supplies authoritative structure, committed values and
/// revisions; the evaluated view supplies bounds and states. Nodes
/// missing from the view are marked unavailable with a zero rectangle
/// instead of fabricating geometry. Entity or incarnation mismatch is an
/// explicit error; no transient cursor except observed focus influences
/// the result.
pub fn build_tree(
    inspect: &GuiInspectResponse,
    view: &GuiEvaluatedView,
) -> Result<GuiSemanticTree, String> {
    build_tree_with_focus(inspect, view, None)
}

/// Build a semantic snapshot with observed keyboard focus.
///
/// The focus lands in the tree only when its node is present with a
/// matching lifetime; stale focus never fabricates a target. Focus rides
/// the existing inspect plus input-focus reads through the normal client
/// boundary (no new protocol tags); it is excluded from persistence.
pub fn build_tree_with_focus(
    inspect: &GuiInspectResponse,
    view: &GuiEvaluatedView,
    focus: Option<GuiSemanticFocus>,
) -> Result<GuiSemanticTree, String> {
    if inspect.root_entity != view.entity {
        return Err("Semantic tree entity mismatch".into());
    }
    if inspect.root_incarnation != view.root_incarnation {
        return Err("Semantic tree incarnation mismatch".into());
    }
    let evaluated: BTreeMap<GuiNodeId, &crate::GuiEvaluatedNode> =
        view.nodes.iter().map(|node| (node.node, node)).collect();
    let mut nodes = Vec::with_capacity(inspect.nodes.len());
    for inspected in &inspect.nodes {
        let role = role_for_content(&inspected.content);
        // Degraded output (pending measurement, lifetime drift, absent
        // evaluation) carries a placeholder content by design; only a live
        // evaluated node can contradict the inspected declaration.
        let live = evaluated
            .get(&inspected.id)
            .is_some_and(|node| node.lifetime == inspected.lifetime && node.available);
        if live
            && let Some(expected) = evaluated_role_hint(inspected.id, view)
            && expected != role
        {
            return Err("Semantic role mismatches evaluated content".into());
        }
        let (bounds, enabled, visible, available) = match evaluated.get(&inspected.id) {
            Some(node) => {
                if node.lifetime != inspected.lifetime {
                    (node.rect, node.enabled, node.visible, false)
                } else {
                    (node.rect, node.enabled, node.visible, node.available)
                }
            }
            None => ([0.0, 0.0, 0.0, 0.0], true, false, false),
        };
        let actions = if enabled && visible && available {
            actions_for_role(role)
        } else {
            Vec::new()
        };
        nodes.push(GuiSemanticNode {
            id: inspected.id,
            lifetime: inspected.lifetime,
            parent: inspected.parent,
            role,
            name: name_for_content(&inspected.content),
            value: inspected.control_value.clone(),
            revision: inspected.control_revision,
            bounds,
            enabled,
            visible,
            available,
            actions,
        });
    }
    let focused = focus.filter(|focus| {
        nodes
            .iter()
            .any(|node| node.id == focus.id && node.lifetime == focus.lifetime)
    });
    Ok(GuiSemanticTree {
        entity: inspect.root_entity,
        root_incarnation: inspect.root_incarnation,
        evaluation_tick: view.evaluation_tick,
        nodes,
        focused,
    })
}

fn evaluated_role_hint(id: GuiNodeId, view: &GuiEvaluatedView) -> Option<GuiSemanticRole> {
    let node = view.nodes.iter().find(|node| node.node == id)?;
    let role = match &node.content {
        GuiEvaluatedContent::Container => GuiSemanticRole::Container,
        GuiEvaluatedContent::Text {
            ..
        } => GuiSemanticRole::Text,
        GuiEvaluatedContent::Drawing {
            ..
        } => GuiSemanticRole::Drawing,
        GuiEvaluatedContent::Image {
            ..
        } => GuiSemanticRole::Image,
        GuiEvaluatedContent::Button {
            ..
        } => GuiSemanticRole::Button,
        GuiEvaluatedContent::Checkbox {
            ..
        } => GuiSemanticRole::Checkbox,
        GuiEvaluatedContent::Slider {
            ..
        } => GuiSemanticRole::Slider,
        GuiEvaluatedContent::TextInput {
            ..
        } => GuiSemanticRole::TextInput,
    };
    Some(role)
}

/// Nodes whose semantic identity, structure, state or supported actions changed.
///
/// Added and removed nodes are reported by their presence in exactly one
/// tree. Evaluated bounds and the tree evaluation tick are intentionally
/// excluded, so pure movement never appears here. Focus is diffed separately
/// by comparing [`GuiSemanticTree::focused`].
pub fn changed_nodes(old: &GuiSemanticTree, new: &GuiSemanticTree) -> Vec<GuiNodeId> {
    let mut changes = Vec::new();
    let old_by_id: BTreeMap<GuiNodeId, &GuiSemanticNode> =
        old.nodes.iter().map(|node| (node.id, node)).collect();
    let new_by_id: BTreeMap<GuiNodeId, &GuiSemanticNode> =
        new.nodes.iter().map(|node| (node.id, node)).collect();
    let root_changed = old.entity != new.entity || old.root_incarnation != new.root_incarnation;
    for node in &new.nodes {
        match old_by_id.get(&node.id) {
            Some(previous) => {
                if root_changed || semantic_node_changed(previous, node) {
                    changes.push(node.id);
                }
            }
            None => changes.push(node.id),
        }
    }
    for node in &old.nodes {
        if !new_by_id.contains_key(&node.id) {
            changes.push(node.id);
        }
    }
    changes.sort();
    changes.dedup();
    changes
}

fn semantic_node_changed(previous: &GuiSemanticNode, current: &GuiSemanticNode) -> bool {
    previous.lifetime != current.lifetime
        || previous.parent != current.parent
        || previous.role != current.role
        || previous.name != current.name
        || previous.value != current.value
        || previous.revision != current.revision
        || previous.enabled != current.enabled
        || previous.visible != current.visible
        || previous.available != current.available
        || previous.actions != current.actions
}

/// Whether one committed effect should refresh a semantic snapshot.
///
/// `ButtonPressed` and `ControlCommitted` name durable control outcomes;
/// `FocusChanged` moves the observed focus the tree now carries. Hover
/// and scroll effects report cursors with no snapshot fields and never
/// refresh it.
pub fn effect_refreshes_semantics(kind: &GuiInputEffectKind) -> bool {
    matches!(
        kind,
        GuiInputEffectKind::ButtonPressed { .. }
            | GuiInputEffectKind::ControlCommitted { .. }
            | GuiInputEffectKind::FocusChanged { .. }
    )
}

impl crate::WorldContext<'_> {
    /// Bounded lifetime/revision-fenced semantic snapshot for one panel.
    ///
    /// Reads the authoritative inspect snapshot, the retained evaluated
    /// view and the observed input focus through the normal boundary;
    /// transient state other than focus never appears, and nothing here
    /// touches committed state. Entity, incarnation and role mismatches
    /// are explicit errors, never fabricated geometry.
    pub fn gui_semantic_snapshot(
        &self,
        entity: EntityId,
        max_depth: u32,
        limit: u32,
    ) -> Result<GuiSemanticTree, String> {
        let inspect = self
            .inspect_gui(entity, None, max_depth, limit)
            .map_err(|error| error.to_string())?;
        self.gui_semantic_tree_from_inspect(entity, inspect)
    }

    /// Fresh authoritative semantic record for one action target.
    ///
    /// Unlike the public bounded tree snapshot, target resolution starts at
    /// the addressed node and therefore does not reject a live node merely
    /// because it appears beyond a traversal page or depth bound. Identity,
    /// evaluated eligibility, role, value and revision still come from the
    /// same inspect/layout owners as the public tree.
    pub fn gui_semantic_action_target(
        &self,
        entity: EntityId,
        node: GuiNodeId,
    ) -> Result<GuiSemanticTree, String> {
        let inspect = self
            .inspect_gui(entity, Some(node), 1, 1)
            .map_err(|error| match error {
                crate::ErrorReason::InvalidValue => {
                    format!("semantic action unknown node {}", node.0)
                }
                error => error.to_string(),
            })?;
        self.gui_semantic_tree_from_inspect(entity, inspect)
    }

    fn gui_semantic_tree_from_inspect(
        &self,
        entity: EntityId,
        inspect: GuiInspectResponse,
    ) -> Result<GuiSemanticTree, String> {
        let layout = self
            .system::<crate::GuiLayoutSystem>(crate::GuiLayoutSystem::ID)
            .ok_or_else(|| "GUI layout unavailable".to_string())?;
        let view = layout
            .view(entity)
            .ok_or_else(|| "GUI view unavailable".to_string())?;
        let focus = self
            .gui_input_focus()
            .filter(|focus| focus.target.entity == entity)
            .map(|focus| GuiSemanticFocus {
                id: focus.target.node,
                lifetime: focus.target.lifetime,
            });
        build_tree_with_focus(&inspect, view, focus)
    }
}
