//! State owned directly by one GUI world system instance.

use super::input::GuiInputTarget;
use super::tree::GuiTreeIndex;
use super::tree::nodes::{GuiControlValue, GuiNodeId};
use crate::EntityId;
use std::collections::{BTreeMap, BTreeSet};

/// Retained [`GuiSystem`](super::system::GuiSystem) state: the root whose
/// structural fields the system is currently committing, if any, the derived
/// child order of every live root, and the accepted external control
/// replacements awaiting input-side publication.
#[derive(Default)]
pub struct GuiSystemState {
    /// Root whose structural fields this System is currently committing.
    pub(super) committing: Option<EntityId>,
    /// Derived child order per live root, reconstructible from its rows.
    pub(super) trees: BTreeMap<EntityId, GuiTreeIndex>,
    /// Tree changes operations wrote since the last commit, applied to
    /// `trees` from committed storage.
    pub(super) tree_changes: BTreeMap<EntityId, GuiTreeChange>,
    /// Accepted external replacements in commit order, retained until the
    /// frame that observed them finishes.
    pub(super) external_commits: Vec<GuiExternalCommit>,
    /// Sequence number of the first retained external replacement.
    pub(super) external_commit_base: u64,
}

/// Tree change of one root observed by operations before a commit.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum GuiTreeChange {
    /// Nodes whose `parent` or `order` was written.
    Nodes(BTreeSet<GuiNodeId>),
    /// The whole tree table was written.
    Rebuild,
}

/// One accepted external [`GuiCommand::SetControlValue`](super::GuiCommand)
/// replacement, pinned at commit for the input system to publish and to
/// fence the focused text against.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::world::systems::gui) struct GuiExternalCommit {
    /// Session that supplied the replacement.
    pub session: u64,
    /// Frame whose mutation boundary applied it.
    pub tick: u64,
    /// Replaced control node.
    pub target: GuiInputTarget,
    /// Committed value.
    pub value: GuiControlValue,
    /// Revision the replacement produced.
    pub revision: u32,
    /// Runtime logical ancestor path, root-first including the target.
    pub path: Vec<GuiNodeId>,
}
