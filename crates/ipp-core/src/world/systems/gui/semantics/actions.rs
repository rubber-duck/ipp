//! Revision- and identity-fenced semantic action translation.
//!
//! Actions resolve against a snapshot into fenced commands; the input
//! command boundary revalidates ownership, eligibility, role, value and
//! revision before admitting any side effect.

use super::types::{
    GuiSemanticAction, GuiSemanticActionCommand, GuiSemanticActionError, GuiSemanticActionKind,
    GuiSemanticNode, GuiSemanticTree,
};
use crate::{GuiInputTarget, GuiNodeId};

/// Whether a semantic action addressed to `expected` is stale.
///
/// Stale operations report a conflict instead of silently overwriting
/// newer input, mirroring the revision-gated control gate.
pub fn is_stale_revision(node: &GuiSemanticNode, expected: u32) -> bool {
    node.revision != expected
}

/// Resolve one semantic action against a snapshot into a fenced command.
///
/// The returned command preserves the exact observed target and action in one
/// envelope. The input command boundary revalidates ownership, eligibility,
/// role, value and revision before it admits any side effect.
pub fn action_command(
    tree: &GuiSemanticTree,
    id: GuiNodeId,
    expected_revision: u32,
    action: GuiSemanticAction,
) -> Result<GuiSemanticActionCommand, GuiSemanticActionError> {
    let node = tree
        .node(id)
        .ok_or(GuiSemanticActionError::UnknownNode(id))?;
    let supported = match &action {
        GuiSemanticAction::Press => node.actions.contains(&GuiSemanticActionKind::Press),
        GuiSemanticAction::Toggle => node.actions.contains(&GuiSemanticActionKind::Toggle),
        GuiSemanticAction::SetScalar(_) => node.actions.contains(&GuiSemanticActionKind::SetScalar),
        GuiSemanticAction::SetText(_) => node.actions.contains(&GuiSemanticActionKind::SetText),
        GuiSemanticAction::Focus => node.actions.contains(&GuiSemanticActionKind::Focus),
    };
    if !supported {
        return Err(GuiSemanticActionError::UnsupportedAction {
            node: id,
        });
    }
    if is_stale_revision(node, expected_revision) {
        return Err(GuiSemanticActionError::StaleRevision {
            node: id,
            expected: expected_revision,
            found: node.revision,
        });
    }
    Ok(GuiSemanticActionCommand {
        target: GuiInputTarget {
            entity: tree.entity,
            root_incarnation: tree.root_incarnation,
            node: id,
            lifetime: node.lifetime,
        },
        expected_revision,
        action,
    })
}
