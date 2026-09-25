//! Headless GUI semantic types: roles, nodes, trees, queries and actions.
//!
//! These types are the machine-client contract without any evaluation or
//! transport behavior; see [`super`] for ownership,
//! [`super::snapshot`] for snapshots and
//! [`super::actions`] for action translation.

use crate::{EntityId, GuiControlValue, GuiInputTarget, GuiNodeData, GuiNodeId};

/// Semantic role of one GUI node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiSemanticRole {
    /// Layered or directional layout container.
    Container,
    /// Static text leaf.
    Text,
    /// Vector drawing leaf.
    Drawing,
    /// Bitmap image leaf.
    Image,
    /// Momentary button.
    Button,
    /// Toggle checkbox.
    Checkbox,
    /// Ranged slider.
    Slider,
    /// Single-line text input.
    TextInput,
    /// Scrolling viewport over its content.
    ScrollView,
}

/// Supported headless action kinds for one semantic node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiSemanticActionKind {
    /// Complete a momentary button press.
    Press,
    /// Toggle a checkbox.
    Toggle,
    /// Commit a slider scalar.
    SetScalar,
    /// Commit input text.
    SetText,
    /// Move keyboard focus to this control.
    Focus,
}

/// One headless semantic node with revision-keyed value and bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSemanticNode {
    /// Stable root-local identity.
    pub id: GuiNodeId,
    /// Logical parent, or None for the root.
    pub parent: Option<GuiNodeId>,
    /// Semantic role derived from structural content.
    pub role: GuiSemanticRole,
    /// Human-readable name: button labels, text leaves and text-input
    /// placeholders; None otherwise.
    pub name: Option<String>,
    /// Committed value; None-equivalent for non-controls.
    pub value: GuiControlValue,
    /// Committed revision; zero for nodes that were never controls.
    pub revision: u32,
    /// Evaluated final-logical bounds `[x, y, width, height]`.
    pub bounds: [f32; 4],
    /// Effective interactivity from evaluation.
    pub enabled: bool,
    /// Effective visibility from evaluation.
    pub visible: bool,
    /// False while measurement failed; unavailable nodes are skipped by
    /// interaction.
    pub available: bool,
    /// Whether this node is a focus scope: keyboard traversal from a focused
    /// descendant stays within its subtree.
    pub focus_scope: bool,
    /// Supported headless actions.
    pub actions: Vec<GuiSemanticActionKind>,
    /// Committed scroll position of an evaluated ScrollView; None for other
    /// roles and for ScrollViews layout has not evaluated.
    pub scroll: Option<GuiSemanticScroll>,
}

/// Committed scroll position of one ScrollView in its local logical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiSemanticScroll {
    /// Committed offset per axis.
    pub offset: [f32; 2],
    /// Largest offset per axis; zero where the content fits.
    pub max_offset: [f32; 2],
}

/// Observed keyboard focus for one semantic snapshot.
///
/// Transient but machine-visible: focus appears in the tree so observe-act
/// loops can target the focused control, while persistence still excludes
/// it. Lifetime fences removal and recreation like every other identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiSemanticFocus {
    /// Focused node identity.
    pub id: GuiNodeId,
}

/// Headless semantic snapshot of one panel.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSemanticTree {
    /// Panel entity.
    pub entity: EntityId,
    /// Root component incarnation the snapshot was built against.
    pub root_incarnation: u64,
    /// World tick of the evaluation supplying bounds.
    pub evaluation_tick: u64,
    /// Semantic nodes in inspection order.
    pub nodes: Vec<GuiSemanticNode>,
    /// Observed keyboard focus, if any. Transient: visible here, never
    /// persisted.
    pub focused: Option<GuiSemanticFocus>,
}

impl GuiSemanticTree {
    /// Find one semantic node by identity.
    pub fn node(&self, id: GuiNodeId) -> Option<&GuiSemanticNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Number of semantic nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree holds no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Role for one node kind.
pub fn role_for_data(data: &GuiNodeData) -> GuiSemanticRole {
    match data {
        GuiNodeData::Container(crate::GuiContainerKind::ScrollView) => GuiSemanticRole::ScrollView,
        GuiNodeData::Container(_) => GuiSemanticRole::Container,
        GuiNodeData::Text(_) => GuiSemanticRole::Text,
        GuiNodeData::Drawing => GuiSemanticRole::Drawing,
        GuiNodeData::Image => GuiSemanticRole::Image,
        GuiNodeData::Button {
            ..
        } => GuiSemanticRole::Button,
        GuiNodeData::Checkbox => GuiSemanticRole::Checkbox,
        GuiNodeData::Slider => GuiSemanticRole::Slider,
        GuiNodeData::TextInput {
            ..
        } => GuiSemanticRole::TextInput,
    }
}

/// Human-readable name for one node's authored strings, if any.
pub fn name_for_data(data: &GuiNodeData) -> Option<String> {
    match data {
        GuiNodeData::Button {
            label,
        } => Some(label.clone()),
        GuiNodeData::Text(text) => Some(text.clone()),
        GuiNodeData::TextInput {
            placeholder,
            ..
        } if !placeholder.is_empty() => Some(placeholder.clone()),
        _ => None,
    }
}

/// Supported headless actions for one role.
pub fn actions_for_role(role: GuiSemanticRole) -> Vec<GuiSemanticActionKind> {
    match role {
        GuiSemanticRole::Button => vec![GuiSemanticActionKind::Press],
        GuiSemanticRole::Checkbox => {
            vec![GuiSemanticActionKind::Toggle, GuiSemanticActionKind::Focus]
        }
        GuiSemanticRole::Slider => vec![
            GuiSemanticActionKind::SetScalar,
            GuiSemanticActionKind::Focus,
        ],
        GuiSemanticRole::TextInput => {
            vec![GuiSemanticActionKind::SetText, GuiSemanticActionKind::Focus]
        }
        GuiSemanticRole::Container
        | GuiSemanticRole::ScrollView
        | GuiSemanticRole::Text
        | GuiSemanticRole::Drawing
        | GuiSemanticRole::Image => Vec::new(),
    }
}

/// Machine-client semantic action for one node.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiSemanticAction {
    /// Complete a momentary button press through one atomically admitted
    /// semantic action. Momentary presses do not move focus or commit a value;
    /// the `ButtonPressed` effect reports the outcome through observations.
    Press,
    /// Toggle a checkbox through the revision-gated control policy.
    Toggle,
    /// Commit a slider scalar through the revision-gated control policy.
    SetScalar(f32),
    /// Commit input text through the revision-gated control policy.
    SetText(String),
    /// Move keyboard focus to this control.
    Focus,
}

/// One revision- and identity-fenced semantic action admitted atomically by
/// the input system. This remains internal; the public wire action is stable.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSemanticActionCommand {
    /// Full evaluated identity observed by the semantic snapshot.
    pub target: GuiInputTarget,
    /// Caller-observed committed revision.
    pub expected_revision: u32,
    /// Exact semantic operation to revalidate and route.
    pub action: GuiSemanticAction,
}

/// Why a semantic action was refused instead of dispatched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuiSemanticActionError {
    /// No node with this identity exists in the snapshot.
    UnknownNode(GuiNodeId),
    /// The committed revision moved since the snapshot; observe again
    /// instead of overwriting newer input.
    StaleRevision {
        /// Node the stale action addressed.
        node: GuiNodeId,
        /// Revision the caller addressed.
        expected: u32,
        /// Current revision carried by the fresh snapshot.
        found: u32,
    },
    /// The action is not advertised for this node's role.
    UnsupportedAction {
        /// Node the unsupported action addressed.
        node: GuiNodeId,
    },
}

/// Bounded machine-client snapshot query for one panel.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSemanticSnapshotQuery {
    /// Panel entity owning the tree.
    pub entity: EntityId,
    /// Maximum tree depth to traverse (1..=32, like inspection).
    pub max_depth: u32,
    /// Maximum number of nodes to return (1..=256, like inspection).
    pub limit: u32,
}

/// Machine-client action addressed to one snapshot node.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSemanticActionRequest {
    /// Panel entity owning the tree.
    pub entity: EntityId,
    /// Root component incarnation observed by the caller.
    pub root_incarnation: u64,
    /// Snapshot node identity.
    pub node: GuiNodeId,
    /// Caller-observed committed revision the action addresses.
    pub expected_revision: u32,
    /// Action to dispatch through the validated control policy.
    pub action: GuiSemanticAction,
}
