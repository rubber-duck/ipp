//! Headless GUI semantic tree for machine clients.
//!
//! This module consumes only the frozen GUI read interfaces:
//! [`GuiEvaluatedContent`](crate::GuiEvaluatedContent) committed
//! payloads, [`GuiControlState`](crate::GuiControlState) `{value, revision}`
//! (observed here through `inspect_gui` control fields), `inspect_gui`
//! snapshots ([`GuiInspectResponse`](crate::GuiInspectResponse)) and
//! [`GuiInputEffectKind`](crate::GuiInputEffectKind) committed effects.
//! It never reads authoritative storage, the input router, the layout
//! cache or skin internals directly.
//!
//! The semantic tree is the machine-client contract: autonomous agents
//! observe and actuate GUI through semantic snapshots and actions, not by
//! synthesizing pointer input or scraping paint. Snapshots carry revisions
//! alongside values, bounds and states so observe-act loops can detect
//! change and address actions to a known revision. Semantic invariance
//! holds across visual-only change: node and part identity, revision
//! monotonicity, invalidation before reuse and conflict-instead-of-overwrite
//! survive reskin and relayout; record layouts themselves follow the
//! pre-stabilization policy.
//!
//! Observed keyboard focus appears in the tree so observe-act loops can
//! target the focused control; every other transient state stays out:
//! caret, selection, provisional composition, hover, press cursors, scroll
//! offsets, pending envelopes and playback have no fields here and never
//! persist.
//!
//! Layout: [`types`] holds roles, nodes, trees, queries and actions;
//! [`snapshot`] builds snapshots and diffs over inspection plus the
//! retained evaluated view; [`actions`] translates snapshot actions into
//! fenced input commands. The host-session adapter lives with its owner in
//! `ipp-host-session` (`gui_semantics.rs`); the wire framing lives in
//! `ipp-protocol` (`codec/semantics.rs`). See also the [GUI
//! architecture](../../../../../../../docs/architecture/gui.md).

pub mod actions;
pub mod snapshot;
pub mod types;

pub use actions::{action_command, is_stale_revision};
pub use snapshot::{build_tree, build_tree_with_focus, changed_nodes, effect_refreshes_semantics};
pub use types::{
    GuiSemanticAction, GuiSemanticActionCommand, GuiSemanticActionError, GuiSemanticActionKind,
    GuiSemanticActionRequest, GuiSemanticFocus, GuiSemanticNode, GuiSemanticRole,
    GuiSemanticSnapshotQuery, GuiSemanticTree, actions_for_role, name_for_content,
    role_for_content,
};

#[cfg(test)]
mod semantics_tests;
