//! Retained GUI constraint evaluation into Surface paint and hit-test geometry.
//!
//! This module owns single-pass box layout for
//! Row, Column, Stack, Padding, Align, SizedBox and ScrollView containers,
//! leaf text/drawing/image evaluation through the shared headless interfaces,
//! retained paint construction through the Surface preparation boundary and
//! analytic hit testing in GUI coordinates.
//!
//! ## Coordinates
//!
//! Evaluation works in GUI logical units with a top-left origin, +X right and
//! +Y down, shared with the Surface content convention. The Surface rectangle
//! (`surface_size` metres, width by height) sets the root constraints; with a
//! finite positive `units_per_metre` factor `U`, the logical root extent is
//! `(W*U, H*U)` and logical `(x, y)` maps to Surface content `(x/U, y/U)`.
//! Only paint construction performs that division, through the shared
//! `.3` mapping helpers. There is no GUI axis flip or origin translation,
//! and camera motion never appears here: it invalidates projection and model
//! work only, never layout.
//!
//! `units_per_metre` is an evaluation input, not stored component state. The
//! current tree carries no stored lane for it, so callers pass an explicit
//! value ([`DEFAULT_UNITS_PER_METRE`] keeps logical units identical to
//! Surface metres). A stored lane can replace the parameter without changing
//! the evaluator shape.
//!
//! ## Pass structure
//!
//! One evaluation lays out the whole tree in a single pass. Row and Column
//! measure fixed children first, then distribute leftover space among flex
//! children, with no iterative cross-node solving. Every child then takes
//! its main-axis slot and painter position in tree order, moving its
//! measured subtree without remeasuring. Invalid or unbounded flex
//! constraints produce [`GuiLayoutDiagnostic`] records instead of solver
//! iterations. Layout properties cause reflow; visual translation/scale
//! move paint and hit regions together without remeasuring text;
//! paint-only edits (colour, opacity) rebuild paint records only.
//!
//! ## Margins
//!
//! A margin (`[top, right, bottom, left]`) is interpreted by the parent
//! container, so its effect depends on that container:
//!
//! - Row and Column reserve both main-axis margins around the child and
//!   shrink its remaining main-axis space. On the cross axis the leading
//!   margin offsets the child in addition to its alignment; the trailing
//!   margin is ignored, and neither margin shrinks the cross constraint or
//!   counts toward the container's cross extent.
//! - Stack shrinks each child's constraints by its margins on both axes,
//!   aligns the margin box, and fits its own extent to margin boxes.
//! - Padding, Align, SizedBox and ScrollView ignore child margins.
//! - The root node is offset by its top and left margins; its constraints
//!   and clip remain the Surface rectangle.
//!
//! Validation accepts any finite margin, including negative values. A
//! negative margin moves the child outward or lets it overlap siblings, and
//! in a Stack enlarges its constraints. Paint and hit regions remain
//! subject to ancestor clips.
//!
//! ## Alignment
//!
//! Align positions its single child by its own align lanes, defaulting to
//! the centre. Stack aligns each child's margin box within its settled
//! extent, and Row and Column align each child on the cross axis within the
//! largest child cross extent; both read the child's own align lanes and
//! default to the start. An Align node placed in a Stack, Row or Column is
//! therefore positioned by the same lanes that position its content.
//!
//! A container only knows a child's final slot after measuring the child's
//! whole subtree, so alignment records a deferred translation for that
//! child. A finishing pass applies the accumulated translations to every
//! descendant's rectangle and content origin in painter order, then
//! resolves clips from the final geometry: each ScrollView viewport moves
//! with its subtree and intersects the fixed clip it inherits. Aligned
//! subtrees therefore keep paint, semantic bounds and hit regions together,
//! including under scaled or reflected placement, without remeasuring text.
//!
//! ## Retention
//!
//! [`GuiLayoutCache`] retains one [`GuiEvaluatedView`] per root entity,
//! keyed by root incarnation, layout revision and evaluation tick. Structure,
//! layout, visual and paint inputs hash into four fingerprints: unchanged
//! frames do no work, visual-only edits rebuild geometry without remeasuring
//! text or reflowing, paint-only edits never remeasure text, and only
//! branches whose text cache key changed are remeasured. Insert, reorder and
//! remove change
//! the structure fingerprint and invalidate dependent output before reuse.
//! Committed control values join the structure fingerprint with their
//! revisions, so every routed or authored commit reflows and the retained
//! view always observes effective values. Computed rectangles are never
//! written back to component storage.
//!
//! ## Consumers
//!
//! Input routing (.6), skinning (.9) and semantic-tree readers (.13) consume
//! [`GuiEvaluatedView`] through [`GuiLayoutSystem`](super::GuiLayoutSystem).
//! Control payloads in the view carry effective (committed, revision-keyed)
//! values, so skins resolve checked/value variants and readers observe
//! actions without re-reading authoritative storage. Paint reaches the
//! renderer through the internal retained Surface preparation path, never
//! through self-issued client commands.

use super::super::tree::component::GuiNodeLanes;
use super::super::{
    GuiContainerKind, GuiControlValue, GuiNode, GuiNodeContent, GuiNodeId, GuiNodeStyle, GuiRoot,
};
use crate::EntityId;
use crate::services::asset_management::font::FontAsset;
use crate::services::asset_management::{AssetKey, AssetSource};
use crate::systems::surface::{
    GuiPrimitiveId, GuiPrimitivePart, GuiShapeFill, SurfaceClipRect, SurfaceGlyph,
    SurfacePrimitiveIdentity, SurfacePrimitiveStyle, SurfaceRenderPrimitive, SurfaceRenderResource,
    TextFont, TextLayout, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome,
    gui_logical_to_surface_content, intersect_surface_clips, measure_text,
    surface_content_to_gui_logical,
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "alignment_tests.rs"]
mod alignment_tests;
#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;
#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

// ---------------------------------------------------------------------------
// Small geometry helpers.
// ---------------------------------------------------------------------------

/// Finite positive value, or None for rejected scale and size inputs.
fn finite_positive(value: f32) -> Option<f32> {
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}

/// Whether a final-logical `[x, y, width, height]` rectangle contains a
/// point. Edges belong to the rectangle on the min side only, so abutting
/// siblings never both claim a shared edge. Zero-area rectangles hit nothing.
fn rect_contains_point(rect: [f32; 4], point: [f32; 2]) -> bool {
    rect[2] > 0.0
        && rect[3] > 0.0
        && point[0] >= rect[0]
        && point[0] < rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[1] + rect[3]
}

/// Whether an accumulated `[min_x, min_y, max_x, max_y]` clip contains a
/// point, with the same min-inclusive edge rule as rectangles.
fn clip_contains_point(clip: SurfaceClipRect, point: [f32; 2]) -> bool {
    point[0] >= clip[0] && point[0] < clip[2] && point[1] >= clip[1] && point[1] < clip[3]
}

/// Normalize a possibly mirrored rectangle into `[x, y, width, height]`
/// with non-negative extents. Negative visual scales mirror layout; the
/// retained rectangle stays canonical while hit testing and paint agree.
fn normalize_rect(origin: [f32; 2], size: [f32; 2]) -> [f32; 4] {
    let (x, w) = if size[0] >= 0.0 {
        (origin[0], size[0])
    } else {
        (origin[0] + size[0], -size[0])
    };
    let (y, h) = if size[1] >= 0.0 {
        (origin[1], size[1])
    } else {
        (origin[1] + size[1], -size[1])
    };
    [x, y, w, h]
}

// ---------------------------------------------------------------------------
// Input fingerprints: structure, layout, visual and paint lanes.
// ---------------------------------------------------------------------------

/// FNV-1a hasher over canonical input bytes. No external digest dependency;
/// fingerprints only drive dirty detection inside one runtime.
struct Fingerprint(u64);

impl Fingerprint {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325_u64)
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        // Canonicalize negative zero so equivalent inputs hash together.
        self.u32(if value == 0.0 {
            0
        } else {
            value.to_bits()
        });
    }

    fn string(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes(value.as_bytes());
    }

    fn finish(self) -> u64 {
        self.0
    }
}

fn hash_option_f32(hasher: &mut Fingerprint, value: Option<f32>) {
    match value {
        Some(value) => {
            hasher.u32(1);
            hasher.f32(value);
        }
        None => hasher.u32(0),
    }
}

fn hash_option_vec4(hasher: &mut Fingerprint, value: Option<[f32; 4]>) {
    match value {
        Some(value) => {
            hasher.u32(1);
            for lane in value {
                hasher.f32(lane);
            }
        }
        None => hasher.u32(0),
    }
}

fn hash_asset(hasher: &mut Fingerprint, source: Option<&AssetSource>) {
    match source {
        Some(source) => {
            hasher.u32(1);
            hasher.u32(u32::from(source.kind.0));
            hasher.string(&source.uri);
            hasher.u32(source.variant);
        }
        None => hasher.u32(0),
    }
}

/// Logical units per Surface metre used when the caller supplies none.
/// Keeps logical units identical to Surface metres.
pub const DEFAULT_UNITS_PER_METRE: f32 = 1.0;

/// Maximum tree depth followed during evaluation. Deeper subtrees are cut
/// with a diagnostic instead of recursing without bound.
pub const MAX_LAYOUT_DEPTH: usize = 128;

/// Rejected or degraded layout input, reported per node without aborting
/// the evaluation. Diagnostics never synthesize geometry: affected nodes are
/// marked unavailable so routing and paint skip them observably.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiLayoutDiagnostic {
    /// A flex child inside an unbounded main axis, or with a non-positive
    /// factor. The child keeps zero main-axis extent.
    UnboundedFlex {
        /// Degraded node.
        node: GuiNodeId,
    },
    /// A min/max/explicit bound was non-finite, negative or inverted. The
    /// bound is sanitized (dropped or clamped) and layout continues.
    InvalidConstraints {
        /// Degraded node, or None for root-level input.
        node: Option<GuiNodeId>,
        /// Which bound failed and how.
        detail: GuiConstraintError,
    },
    /// A text leaf whose font is not ready. Dependent output stays
    /// unavailable until the font resolves and the branch rebuilds.
    PendingText {
        /// Degraded node.
        node: GuiNodeId,
    },
    /// A leaf or control whose required asset is absent. Layout keeps any
    /// content-derived size; paint for the missing part is suppressed.
    MissingResource {
        /// Degraded node.
        node: GuiNodeId,
        /// Asset role that is absent (`"font"`, `"drawing"` or `"image"`).
        kind: &'static str,
    },
    /// A zero visual scale factor. The node and its subtree keep a zero
    /// rectangle and are skipped by paint and hit testing.
    SingularTransform {
        /// Degraded node.
        node: GuiNodeId,
    },
    /// Evaluation cut short: depth limit, missing node identity or an
    /// unsupported content reference.
    Unsupported {
        /// Degraded node, or None for root-level input.
        node: Option<GuiNodeId>,
        /// Stable machine-readable reason.
        detail: &'static str,
    },
}

/// How one constraint bound failed validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiConstraintError {
    /// Bound was non-finite.
    NonFinite,
    /// Bound was negative where only zero or positive is meaningful.
    Negative,
    /// Minimum exceeded maximum; the pair is clamped together.
    MinExceedsMax,
    /// Surface size or units-per-metre input was not finite and positive.
    InvalidRoot,
}

/// Headless evaluation inputs for one GUI root. The tree is borrowed from
/// authoritative component storage; nothing here retains component refs.
#[derive(Clone, Copy, Debug)]
pub struct GuiLayoutRequest<'a> {
    /// Authoritative root-local node tree with committed values.
    pub root: &'a GuiRoot,
    /// Surface-owned root component incarnation fencing reuse.
    pub root_incarnation: u64,
    /// Surface size in metres `[width, height]`; sets root constraints.
    pub surface_size: [f32; 2],
    /// Logical units per Surface metre; finite and positive.
    pub units_per_metre: f32,
    /// World tick identifying this evaluation.
    pub evaluation_tick: u64,
}

/// Font resolution for one text measurement, mirroring the readiness states
/// of the shared text interface without guessing metrics.
#[derive(Clone, Copy, Debug)]
pub enum GuiFontResolution<'a> {
    /// Immutable ready font with its exact asset incarnation key.
    Ready {
        /// Runtime slot handle identifying the font incarnation.
        key: AssetKey,
        /// Borrowed immutable decoded font metrics.
        font: &'a FontAsset,
    },
    /// Font not yet available; dependent output stays unavailable.
    Pending {
        /// Runtime slot handle the caller waits on.
        key: AssetKey,
    },
    /// No asset registered for the requested source.
    Missing,
}

/// Caller-supplied asset lookup bridging component asset references to the
/// immutable resources measurement and paint need. The evaluator never
/// touches Host services directly, so unit tests can resolve fixtures.
///
/// Subset rule: evaluation succeeds with the ready subset of asset-backed
/// sources. A pending or missing source degrades only its dependent nodes
/// (`PendingText`/`MissingResource` diagnostics, unavailable measurement,
/// suppressed paint, prior skin appearance retained) and never the whole
/// view; the same authored reference recovering later rebuilds only the
/// affected branches through the generation input below.
pub trait GuiResourceResolver {
    /// Resolve the font behind a text leaf's asset reference.
    fn text_font(&self, source: &AssetSource) -> GuiFontResolution<'_>;

    /// Resolve a drawing or bitmap leaf's asset reference to retained paint
    /// identity. Returns None while the asset is unavailable.
    fn surface_resource(&self, source: &AssetSource) -> Option<SurfaceRenderResource>;

    /// Decoded drawing view box `[min_x, min_y, max_x, max_y]`, when ready.
    ///
    /// Skin paint uses this narrow metadata hook only to fit a drawing-backed
    /// synthesized control part into its retained layout rectangle. Ordinary
    /// authored and layout-emitted drawings keep their source coordinates.
    fn drawing_view_box(&self, _source: &AssetSource) -> Option<[f32; 4]> {
        None
    }

    /// Generation of one asset source backing retained output, when known.
    /// The retained fast path mixes this into its layout fingerprint, so a
    /// readiness or replacement generation change reflows even when the
    /// authored reference is unchanged. The default reports nothing and
    /// preserves the previous fingerprint behavior; production resolvers
    /// report the runtime slot generation.
    fn resource_generation(&self, _source: &AssetSource) -> Option<u64> {
        None
    }
}

/// One retained evaluated node in painter order. Parent backgrounds precede
/// their children; reverse iteration is reverse-painter hit order.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiEvaluatedNode {
    /// Stable root-local node identity.
    pub node: GuiNodeId,
    /// Node lifetime fencing primitive and handle reuse.
    pub lifetime: u32,
    /// Depth below the root node; the root node itself is depth zero.
    pub depth: u32,
    /// Final logical rectangle `[x, y, width, height]`, including the node's
    /// own visual translation and scale. Paint and hit testing share it.
    pub rect: [f32; 4],
    /// Accumulated clip in final logical `[min_x, min_y, max_x, max_y]`
    /// coordinates, or None inside the unclipped root rectangle.
    pub clip: Option<SurfaceClipRect>,
    /// Evaluated content with retained measurement payloads.
    pub content: GuiEvaluatedContent,
    /// Effective interactivity from the authored `enabled` lane (default
    /// true). Disabled nodes keep their rectangle but are skipped by hit
    /// testing, activation and skin interaction resolution.
    pub enabled: bool,
    /// Effective visibility; false at zero opacity or when unavailable.
    pub visible: bool,
    /// False when measurement failed (pending font, missing resource,
    /// singular transform, invalid input). Unavailable nodes keep a zero
    /// rectangle and are skipped by paint and hit testing.
    pub available: bool,
    /// True when the accumulated clip is empty: paint is suppressed while
    /// the retained rectangle stays observable.
    pub paint_suppressed: bool,
    /// Visual translation in parent-final logical units.
    pub visual_offset: [f32; 2],
    /// Visual axis-aligned scale about the node origin.
    pub visual_scale: [f32; 2],
    /// Accumulated visual scale from the root through this node, signed per
    /// axis. Paint and pointer mapping divide baked final geometry back to
    /// node-local payloads through it; available nodes carry nonzero lanes.
    pub acc_scale: [f32; 2],
    /// Scroll content extents in logical units; set only by ScrollView.
    pub content_extents: Option<[f32; 2]>,
    /// Final-logical origin of the content box (node origin plus padding).
    /// Leaf paint anchors here; container backgrounds still use `rect`.
    pub content_origin: [f32; 2],
    /// Foreground and text colour; straight linear RGBA.
    pub color: [f32; 4],
    /// Background fill colour; None paints no box.
    pub background: Option<[f32; 4]>,
    /// Content opacity multiplier.
    pub opacity: f32,
}

/// Retained measurement payload per content kind.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiEvaluatedContent {
    /// Layout container; paint is its background only.
    Container,
    /// Measured text with the exact font incarnation it was shaped against.
    Text {
        /// Measured layout in ems, scaled to logical units by `font_size`.
        layout: TextLayout,
        /// Retained paint identity of the ready font.
        font: SurfaceRenderResource,
        /// Metres per em used for this measurement.
        font_size: f32,
        /// Measured string revision.
        text: String,
    },
    /// Vector drawing leaf.
    Drawing {
        /// Retained paint identity of the drawing asset, when available.
        drawing: Option<SurfaceRenderResource>,
    },
    /// Bitmap image leaf.
    Image {
        /// Retained paint identity of the bitmap asset, when available.
        bitmap: Option<SurfaceRenderResource>,
    },
    /// Button with its measured label.
    Button {
        /// Measured label layout in ems.
        layout: TextLayout,
        /// Retained paint identity of the ready font.
        font: SurfaceRenderResource,
        /// Metres per em used for this measurement.
        font_size: f32,
        /// Measured label revision.
        label: String,
    },
    /// Checkbox control with its effective value. The intrinsic box derives
    /// from the font size; the checked state is the committed value, since
    /// authored content carries the initial value only.
    Checkbox {
        /// Effective checked state from the committed revision.
        checked: bool,
        /// Committed revision that produced the value; zero when no
        /// committed value exists and the authored initial value applies.
        revision: u32,
    },
    /// Slider control with its effective value. The intrinsic bar derives
    /// from the font size; bounds and step stay authored lanes.
    Slider {
        /// Effective slider value from the committed revision.
        value: f32,
        /// Authored minimum value.
        min: f32,
        /// Authored maximum value.
        max: f32,
        /// Authored step increment, or 0.0 for continuous.
        step: f32,
        /// Committed revision that produced the value; zero when no
        /// committed value exists and the authored initial value applies.
        revision: u32,
    },
    /// Single-line text input with measured committed or placeholder text.
    TextInput {
        /// Measured layout in ems.
        layout: TextLayout,
        /// Retained paint identity of the ready font.
        font: SurfaceRenderResource,
        /// Metres per em used for this measurement.
        font_size: f32,
        /// Measured string revision (effective text or placeholder).
        text: String,
        /// Committed revision that produced the effective text; zero when
        /// no committed value exists and authored content applies.
        revision: u32,
    },
}

/// Read-only evaluated geometry for one GUI root, shared by hit testing,
/// semantics and rendering. Identified by root incarnation, layout revision
/// and evaluation tick; consumers must drop it when the incarnation moves.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiEvaluatedView {
    /// Root entity owning the evaluated tree.
    pub entity: EntityId,
    /// Root component incarnation this evaluation was built against.
    pub root_incarnation: u64,
    /// Advances on every reflow (structure or layout input change).
    pub layout_revision: u64,
    /// Advances when paint records change (reflow or paint-only edit).
    pub paint_revision: u64,
    /// World tick of the latest evaluation, including no-op refreshes.
    pub evaluation_tick: u64,
    /// Logical root rectangle `[x, y, width, height]`; layout never changes
    /// world poses, so this always starts at the Surface origin.
    pub root_bounds: [f32; 4],
    /// Logical units per Surface metre used by this evaluation.
    pub units_per_metre: f32,
    /// Evaluated nodes in painter order.
    pub nodes: Vec<GuiEvaluatedNode>,
    /// Diagnostics accumulated during the latest reflow.
    pub diagnostics: Vec<GuiLayoutDiagnostic>,
    /// Cumulative text remeasurements across evaluations.
    pub remeasure_count: u64,
    /// Cumulative reflows across evaluations.
    pub reflow_count: u64,
    /// False when root-level input (Surface size, units factor, missing
    /// tree) made evaluation impossible.
    pub available: bool,
}

/// Identity-only retained paint inventory for one evaluated node.
///
/// Skin reconciliation consumes this instead of cloning complete Surface
/// primitives merely to discover their stable named parts.
pub(crate) struct GuiSurfaceNodeParts<'a> {
    pub(crate) index: usize,
    pub(crate) node: &'a GuiEvaluatedNode,
    pub(crate) parts: [Option<GuiPrimitivePart>; 2],
}

impl GuiEvaluatedView {
    /// Topmost eligible node containing a final-logical point, traversing
    /// in reverse painter order within the shared clips. Returns None on a
    /// miss; never fabricates coordinates.
    pub fn hit_test(&self, point: [f32; 2]) -> Option<GuiHit> {
        if !self.available {
            return None;
        }

        for node in self.nodes.iter().rev() {
            if !node.available || !node.enabled || !node.visible {
                continue;
            }

            if !rect_contains_point(node.rect, point) {
                continue;
            }

            if let Some(clip) = node.clip
                && !clip_contains_point(clip, point)
            {
                continue;
            }

            return Some(GuiHit {
                node: node.node,
                lifetime: node.lifetime,
                position: point,
            });
        }

        None
    }

    /// Hit test from a Surface content-metre point, converting to GUI
    /// coordinates with this evaluation's units factor. Returns None when
    /// the conversion is invalid; never fabricates coordinates.
    pub fn hit_test_content(&self, content: [f32; 2]) -> Option<GuiHit> {
        let logical = surface_content_to_gui_logical(content, self.units_per_metre)?;
        self.hit_test(logical)
    }

    /// Retained part identities grouped with their already-resolved node.
    pub(crate) fn surface_part_inventory(&self) -> Vec<GuiSurfaceNodeParts<'_>> {
        let mut inventory = Vec::new();
        if !self.available || finite_positive(self.units_per_metre).is_none() {
            return inventory;
        }

        for (index, node) in self.nodes.iter().enumerate() {
            if !node.available || node.paint_suppressed {
                continue;
            }
            let base_geometry_available =
                gui_logical_to_surface_content([node.rect[0], node.rect[1]], self.units_per_metre)
                    .is_some()
                    && gui_logical_to_surface_content(node.content_origin, self.units_per_metre)
                        .is_some();

            let background = (base_geometry_available
                && node.background.is_some()
                && node.rect[2] > 0.0
                && node.rect[3] > 0.0)
                .then_some(GuiPrimitivePart::Background);
            let content = if base_geometry_available {
                match &node.content {
                    GuiEvaluatedContent::Text {
                        layout,
                        ..
                    }
                    | GuiEvaluatedContent::Button {
                        layout,
                        ..
                    }
                    | GuiEvaluatedContent::TextInput {
                        layout,
                        ..
                    } if !layout.glyphs.is_empty() => Some(GuiPrimitivePart::Label),
                    GuiEvaluatedContent::Drawing {
                        drawing: Some(_),
                    } => Some(GuiPrimitivePart::Icon),
                    GuiEvaluatedContent::Image {
                        bitmap: Some(_),
                    } if node.rect[2] > 0.0 && node.rect[3] > 0.0 => Some(GuiPrimitivePart::Icon),
                    _ => None,
                }
            } else {
                None
            };
            inventory.push(GuiSurfaceNodeParts {
                index,
                node,
                parts: [background, content],
            });
        }
        inventory
    }

    /// Ordered retained paint for the Surface preparation boundary, in
    /// painter order with per-primitive clips in Surface content metres.
    /// Primitives carry [`SurfacePrimitiveIdentity::Gui`] identity keyed by
    /// root incarnation, node lifetime and stable named part, disjoint from
    /// authored item identities.
    pub fn surface_primitives(&self) -> Vec<SurfaceRenderPrimitive> {
        let mut primitives = Vec::new();
        if !self.available {
            return primitives;
        }

        let Some(units) = finite_positive(self.units_per_metre) else {
            return primitives;
        };

        for node in &self.nodes {
            if !node.available || node.paint_suppressed {
                continue;
            }

            let Some(origin) = gui_logical_to_surface_content([node.rect[0], node.rect[1]], units)
            else {
                continue;
            };
            let Some(content_origin) = gui_logical_to_surface_content(node.content_origin, units)
            else {
                continue;
            };
            let clip = node.clip.and_then(|clip| {
                let min = gui_logical_to_surface_content([clip[0], clip[1]], units)?;
                let max = gui_logical_to_surface_content([clip[2], clip[3]], units)?;
                Some([min[0], min[1], max[0], max[1]])
            });

            // Background box first so leaf content paints above it.
            if let Some(color) = node.background
                && node.rect[2] > 0.0
                && node.rect[3] > 0.0
            {
                primitives.push(SurfaceRenderPrimitive::Box {
                    style: SurfacePrimitiveStyle {
                        identity: SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
                            root_incarnation: self.root_incarnation,
                            node: node.node,
                            lifetime: node.lifetime,
                            part: GuiPrimitivePart::Background,
                        }),
                        position: origin,
                        scale: [1.0, 1.0],
                        color,
                        opacity: node.opacity,
                        clip,
                    },
                    size: [node.rect[2] / units, node.rect[3] / units],
                    corner_radius: [0.0, 0.0],
                    border_width: 0.0,
                    border_color: [0.0, 0.0, 0.0, 0.0],
                    fill: GuiShapeFill::Solid(color),
                    glow: None,
                });
            }

            primitives.extend(node_content_primitive(
                self.root_incarnation,
                node,
                content_origin,
                clip,
                units,
            ));
        }

        primitives
    }
}

/// Analytic hit on one evaluated node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiHit {
    /// Hit node identity.
    pub node: GuiNodeId,
    /// Node lifetime fencing reuse after removal and recreation.
    pub lifetime: u32,
    /// Final-logical hit position that selected this node.
    pub position: [f32; 2],
}

/// Ordinary scene picking geometry explicitly marked as a GUI input
/// blocker, already resolved to a World-space distance by the geometry
/// queries. Visual occlusion alone never blocks; only explicit blockers
/// arrive here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiBlockerHit {
    /// World-space distance from the ray origin to the blocker.
    pub distance: f32,
    /// Blocking scene entity; breaks distance ties deterministically.
    pub entity: EntityId,
}

/// Routing decision comparing one panel hit against explicit scene
/// blockers in World-space distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuiPanelResolution {
    /// The panel hit stands: no blocker is nearer, and a coincident
    /// blocker loses ties only when farther, never when equal.
    Panel(GuiHit),
    /// An explicit blocker wins the distance compare.
    Blocked {
        /// Winning blocker entity.
        entity: EntityId,
    },
    /// No live panel was hit; captured input stays on its panel under the
    /// input system's own scope and is never re-decided here.
    Miss,
}

/// Compare a panel hit at a World-space distance against explicit scene
/// blockers. The nearer surface wins; stable entity identity breaks
/// blocker ties, and a coincident explicit blocker takes precedence over
/// the panel. A missing or non-finite panel distance is an observable miss,
/// never a fabricated panel hit.
pub fn resolve_panel_hit(
    panel_distance: Option<f32>,
    gui_hit: Option<GuiHit>,
    blockers: &[GuiBlockerHit],
) -> GuiPanelResolution {
    let (Some(distance), Some(hit)) = (panel_distance, gui_hit) else {
        return GuiPanelResolution::Miss;
    };
    if !distance.is_finite() {
        return GuiPanelResolution::Miss;
    }

    // Deterministic minimum over (distance, entity): nearer wins, stable
    // entity identity breaks blocker ties, and a coincident explicit
    // blocker takes precedence over the panel.
    let mut winner: Option<&GuiBlockerHit> = None;
    for blocker in blockers {
        if !blocker.distance.is_finite() {
            continue;
        }

        let better = match winner {
            None => true,
            Some(current) => {
                (blocker.distance, blocker.entity) < (current.distance, current.entity)
            }
        };
        if better {
            winner = Some(blocker);
        }
    }

    match winner {
        Some(blocker) if blocker.distance <= distance => GuiPanelResolution::Blocked {
            entity: blocker.entity,
        },
        _ => GuiPanelResolution::Panel(hit),
    }
}
// ---------------------------------------------------------------------------
// Paint construction for one retained node.
// ---------------------------------------------------------------------------

/// Leaf content primitive for one evaluated node. The background box is
/// emitted by the caller; this covers glyphs, drawings and bitmaps. Text
/// glyph positions are relative to the content-box top-left and retain their
/// per-line baseline offsets when scaled from ems to Surface metres. Glyph
/// and drawing payloads stay in node-local metres with the accumulated visual
/// scale applied through the shared style, so asymmetric scales reach the
/// renderer while box and bitmap geometry arrives baked into final coordinates.
fn node_content_primitive(
    root_incarnation: u64,
    node: &GuiEvaluatedNode,
    origin: [f32; 2],
    clip: Option<SurfaceClipRect>,
    units: f32,
) -> Option<SurfaceRenderPrimitive> {
    let identity = SurfacePrimitiveIdentity::Gui(GuiPrimitiveId {
        root_incarnation,
        node: node.node,
        lifetime: node.lifetime,
        part: match &node.content {
            GuiEvaluatedContent::Text {
                ..
            }
            | GuiEvaluatedContent::Button {
                ..
            }
            | GuiEvaluatedContent::TextInput {
                ..
            } => GuiPrimitivePart::Label,
            GuiEvaluatedContent::Drawing {
                ..
            }
            | GuiEvaluatedContent::Image {
                ..
            } => GuiPrimitivePart::Icon,
            GuiEvaluatedContent::Container
            | GuiEvaluatedContent::Checkbox {
                ..
            }
            | GuiEvaluatedContent::Slider {
                ..
            } => GuiPrimitivePart::Label,
        },
    });
    let mut style = SurfacePrimitiveStyle {
        identity,
        position: origin,
        scale: node.acc_scale,
        color: node.color,
        opacity: node.opacity,
        clip,
    };

    match &node.content {
        GuiEvaluatedContent::Text {
            layout,
            font,
            font_size,
            ..
        }
        | GuiEvaluatedContent::Button {
            layout,
            font,
            font_size,
            ..
        }
        | GuiEvaluatedContent::TextInput {
            layout,
            font,
            font_size,
            ..
        } => {
            if layout.glyphs.is_empty() {
                return None;
            }

            Some(SurfaceRenderPrimitive::Glyphs {
                style,
                font: font.clone(),
                font_size: *font_size,
                glyphs: layout
                    .glyphs
                    .iter()
                    .map(|glyph| SurfaceGlyph {
                        glyph_id: glyph.glyph_id,
                        position: [glyph.position[0] * font_size, glyph.position[1] * font_size],
                        color: None,
                    })
                    .collect(),
            })
        }
        GuiEvaluatedContent::Drawing {
            drawing,
        } => drawing
            .clone()
            .map(|drawing| SurfaceRenderPrimitive::Drawing {
                style,
                drawing,
            }),
        GuiEvaluatedContent::Image {
            bitmap,
        } => {
            let bitmap = bitmap.clone()?;
            if node.rect[2] <= 0.0 || node.rect[3] <= 0.0 {
                return None;
            }

            // Bitmap extents arrive baked from the retained rectangle, so
            // the shared style carries no additional visual scale here.
            style.scale = [1.0, 1.0];
            Some(SurfaceRenderPrimitive::Bitmap {
                style,
                bitmap,
                size: [node.rect[2] / units, node.rect[3] / units],
            })
        }
        GuiEvaluatedContent::Container
        | GuiEvaluatedContent::Checkbox {
            ..
        }
        | GuiEvaluatedContent::Slider {
            ..
        } => None,
    }
}

// ---------------------------------------------------------------------------
// Constraint evaluation.
// ---------------------------------------------------------------------------

/// Sanitized per-node constraints in node-local logical units. Unbounded
/// maxima are infinite; minima default to zero.
#[derive(Clone, Copy, Debug)]
struct Constraints {
    min_w: f32,
    min_h: f32,
    max_w: f32,
    max_h: f32,
}

impl Constraints {
    fn loose(max_w: f32, max_h: f32) -> Self {
        Self {
            min_w: 0.0,
            min_h: 0.0,
            max_w,
            max_h,
        }
    }

    fn clamp_width(&self, value: f32) -> f32 {
        value.clamp(self.min_w, self.max_w)
    }

    fn clamp_height(&self, value: f32) -> f32 {
        value.clamp(self.min_h, self.max_h)
    }
}

/// Owned text cache key for retained measurement. Covers the measured
/// string, the exact font incarnation, the line policy, the width
/// constraint and the font size; never camera or paint state.
#[derive(Clone, Debug, PartialEq)]
struct OwnedTextKey {
    text: String,
    font: AssetKey,
    line_policy: TextLinePolicy,
    max_width_bits: u32,
    font_size_bits: u32,
}

/// Retained measurement for one text leaf across evaluations.
#[derive(Clone, Debug)]
struct RetainedText {
    key: OwnedTextKey,
    layout: TextLayout,
}

/// Evaluation-only placement facts kept parallel to the evaluator's records.
///
/// Containers only learn a child's final slot after measuring its subtree,
/// so alignment defers the subtree translation here. The finishing pass
/// applies accumulated translations to descendants and then resolves clips.
#[derive(Clone, Copy, Debug, Default)]
struct DeferredPlacement {
    /// Final-logical translation of this record and its complete subtree.
    shift: [f32; 2],
    /// ScrollView viewport in final-logical coordinates before any deferred
    /// translation; its descendants intersect their inherited clip with it.
    viewport: Option<SurfaceClipRect>,
    /// Placeholder for a node that could not lay out; paint stays suppressed.
    placeholder: bool,
}

/// Single-pass constraint evaluator. Fixed children measure first, then
/// flex children share leftover space, and both keep their tree-order
/// slots; diagnostics replace iteration.
struct Evaluator<'a, 'r> {
    root: &'a GuiRoot,
    resolver: &'r dyn GuiResourceResolver,
    units: f32,
    diagnostics: Vec<GuiLayoutDiagnostic>,
    nodes: Vec<GuiEvaluatedNode>,
    /// Deferred placement per record, index-aligned with `nodes`.
    placements: Vec<DeferredPlacement>,
    texts: BTreeMap<GuiNodeId, RetainedText>,
    remeasured: u64,
}

/// Zero-area placeholder for unavailable nodes.
const ZERO_SIZE: [f32; 2] = [0.0, 0.0];

/// Sanitize one optional bound lane: non-finite and negative values are
/// dropped with a diagnostic instead of flowing into layout.
fn sanitized_bound(
    value: Option<f32>,
    node: Option<GuiNodeId>,
    diagnostics: &mut Vec<GuiLayoutDiagnostic>,
) -> Option<f32> {
    let value = value?;
    if !value.is_finite() {
        diagnostics.push(GuiLayoutDiagnostic::InvalidConstraints {
            node,
            detail: GuiConstraintError::NonFinite,
        });
        return None;
    }
    if value < 0.0 {
        diagnostics.push(GuiLayoutDiagnostic::InvalidConstraints {
            node,
            detail: GuiConstraintError::Negative,
        });
        return None;
    }

    Some(value)
}

/// Authored size lanes are denominated in local metres, matching the style
/// documentation; evaluation scales them into logical units by the
/// evaluation's units factor. Scalar lanes (flex, align, colour, opacity,
/// font size) are unit-free or carry their own scale and pass through.
fn styled(root: &GuiRoot, id: GuiNodeId, units: f32) -> GuiNodeStyle {
    let mut style = root.style(id).unwrap_or_default();
    for value in [
        &mut style.width,
        &mut style.height,
        &mut style.min_width,
        &mut style.min_height,
        &mut style.max_width,
        &mut style.max_height,
    ]
    .into_iter()
    .filter_map(|lane| lane.as_mut())
    {
        *value *= units;
    }
    if let Some(padding) = style.padding.as_mut() {
        for value in padding.iter_mut() {
            *value *= units;
        }
    }
    if let Some(margin) = style.margin.as_mut() {
        for value in margin.iter_mut() {
            *value *= units;
        }
    }

    style
}

/// Visual translation in logical units with unit-free scale.
fn visual_scaled(root: &GuiRoot, id: GuiNodeId, units: f32) -> ([f32; 2], [f32; 2]) {
    let (offset, scale) = root.visual_transform(id);
    ([offset[0] * units, offset[1] * units], scale)
}

/// Committed (effective) control value and its revision for one node.
/// Authored content carries the initial value only: routed input and
/// explicit resets commit through [`GuiControls`](super::GuiControls) without
/// rewriting authored content, so measurement, paint and hit testing must
/// observe the committed revision or they would show stale state. Returns
/// None while no committed value exists and the authored initial applies.
fn effective_control(root: &GuiRoot, id: GuiNodeId) -> Option<(GuiControlValue, u32)> {
    root.control_state(id)
        .map(|state| (state.value.clone(), state.revision))
}

/// Clamp a settled size into pre-sanitized min/max lanes. An inverted pair
/// is clamped together with a diagnostic rather than solved.
fn apply_min_max(
    size: f32,
    min: Option<f32>,
    max: Option<f32>,
    node: Option<GuiNodeId>,
    diagnostics: &mut Vec<GuiLayoutDiagnostic>,
) -> f32 {
    let min = min.unwrap_or(0.0);
    let max = max.unwrap_or(f32::INFINITY);
    if min > max {
        diagnostics.push(GuiLayoutDiagnostic::InvalidConstraints {
            node,
            detail: GuiConstraintError::MinExceedsMax,
        });
        return min;
    }

    size.clamp(min, max)
}

impl<'a, 'r> Evaluator<'a, 'r> {
    /// Lay out one node. `constraints` arrive in node-local logical units,
    /// `slot_final` is the node's provisional layout origin in final-logical
    /// coordinates (before its own visual offset and any deferred alignment
    /// translation), and `acc` accumulates the ancestors' visual scales per
    /// axis. Clips resolve after placement. Returns the node's node-local
    /// logical size.
    fn visit(
        &mut self,
        id: GuiNodeId,
        constraints: Constraints,
        slot_final: [f32; 2],
        acc: [f32; 2],
        depth: u32,
    ) -> [f32; 2] {
        let Some(node) = self.root.nodes().node(id) else {
            self.diagnostics.push(GuiLayoutDiagnostic::Unsupported {
                node: Some(id),
                detail: "missing-node",
            });
            return ZERO_SIZE;
        };
        if depth > MAX_LAYOUT_DEPTH as u32 {
            self.push_unavailable(
                node,
                slot_final,
                depth,
                GuiLayoutDiagnostic::Unsupported {
                    node: Some(id),
                    detail: "max-depth",
                },
            );
            return ZERO_SIZE;
        }

        let style = styled(self.root, id, self.units);
        let (offset, scale) = visual_scaled(self.root, id, self.units);
        if scale[0] == 0.0 || scale[1] == 0.0 {
            self.push_unavailable(
                node,
                slot_final,
                depth,
                GuiLayoutDiagnostic::SingularTransform {
                    node: id,
                },
            );
            return ZERO_SIZE;
        }

        let final_origin = [slot_final[0] + offset[0], slot_final[1] + offset[1]];
        let acc_total = [acc[0] * scale[0], acc[1] * scale[1]];
        let padding = style.padding.unwrap_or([0.0; 4]);

        // Sanitize explicit and min/max lanes once: explicit and max lanes
        // cap the content box before children measure (so wrapping text and
        // scroll viewports observe them), while min lanes only lift the
        // settled size afterwards.
        let explicit_w = sanitized_bound(style.width, Some(id), &mut self.diagnostics);
        let explicit_h = sanitized_bound(style.height, Some(id), &mut self.diagnostics);
        let min_w = sanitized_bound(style.min_width, Some(id), &mut self.diagnostics);
        let min_h = sanitized_bound(style.min_height, Some(id), &mut self.diagnostics);
        let max_w = sanitized_bound(style.max_width, Some(id), &mut self.diagnostics);
        let max_h = sanitized_bound(style.max_height, Some(id), &mut self.diagnostics);
        // Node-level fill extents: explicit and max lanes cap the box
        // before padding is removed, so containers fill the box they were
        // given rather than the shrunken content area.
        let fill = [
            [
                constraints.max_w,
                explicit_w.unwrap_or(f32::INFINITY),
                max_w.unwrap_or(f32::INFINITY),
            ]
            .into_iter()
            .reduce(f32::min)
            .unwrap_or(f32::INFINITY),
            [
                constraints.max_h,
                explicit_h.unwrap_or(f32::INFINITY),
                max_h.unwrap_or(f32::INFINITY),
            ]
            .into_iter()
            .reduce(f32::min)
            .unwrap_or(f32::INFINITY),
        ];
        let content_constraints = Constraints {
            min_w: 0.0,
            min_h: 0.0,
            max_w: (fill[0] - padding[1] - padding[3]).max(0.0),
            max_h: (fill[1] - padding[0] - padding[2]).max(0.0),
        };

        let content_origin_final = [
            final_origin[0] + padding[3] * acc_total[0],
            final_origin[1] + padding[0] * acc_total[1],
        ];
        let child_base = self.nodes.len();
        let outcome = self.layout_content(
            node,
            &style,
            content_constraints,
            fill,
            content_origin_final,
            acc_total,
            depth,
        );

        // Apply explicit size and min/max lanes around the content size.
        let mut size = outcome.size;
        if let Some(width) = explicit_w {
            size[0] = width;
        }
        if let Some(height) = explicit_h {
            size[1] = height;
        }
        size[0] = apply_min_max(size[0], min_w, max_w, Some(id), &mut self.diagnostics);
        size[1] = apply_min_max(size[1], min_h, max_h, Some(id), &mut self.diagnostics);
        size[0] = constraints.clamp_width(size[0]);
        size[1] = constraints.clamp_height(size[1]);

        let available = outcome.available;
        // The retained rectangle carries the full accumulated visual chain
        // about the node origin, so paint, bounds, clips and hits share one
        // geometry. Flow siblings keep observing unscaled layout sizes, and
        // measurement stays in node-local units, so visual edits never
        // remeasure or reflow.
        let rect = normalize_rect(
            final_origin,
            [size[0] * acc_total[0], size[1] * acc_total[1]],
        );

        // Clip and paint suppression resolve after deferred alignment moves
        // this subtree into its final slot.
        let record = GuiEvaluatedNode {
            node: id,
            lifetime: node.lifetime,
            depth,
            rect,
            clip: None,
            content: outcome.content,
            enabled: style.enabled,
            visible: available && style.opacity > 0.0,
            available,
            paint_suppressed: false,
            visual_offset: offset,
            visual_scale: scale,
            acc_scale: acc_total,
            content_extents: outcome.content_extents,
            content_origin: content_origin_final,
            color: style.color,
            background: style.background_color,
            opacity: style.opacity,
        };
        // Painter order needs parents before children, but content sizes
        // only settle after children lay out. Insert this record before the
        // child records this visit just pushed.
        self.nodes.insert(child_base, record);
        self.placements.insert(
            child_base,
            DeferredPlacement {
                viewport: outcome.viewport,
                ..DeferredPlacement::default()
            },
        );

        size
    }

    /// Lay out node content in node-local logical units. Returns the local
    /// content size plus payload; the caller applies explicit and min/max
    /// lanes and retains the record.
    #[allow(clippy::too_many_arguments)]
    fn layout_content(
        &mut self,
        node: &GuiNode,
        style: &GuiNodeStyle,
        constraints: Constraints,
        fill: [f32; 2],
        final_origin: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        match &node.content {
            GuiNodeContent::Container(kind) => self.layout_container(
                node,
                style,
                *kind,
                constraints,
                fill,
                final_origin,
                acc_total,
                depth,
            ),
            GuiNodeContent::Text(text) => {
                let outcome = self.measure_leaf(
                    node.id,
                    text,
                    TextLinePolicy::Multiline,
                    style,
                    constraints.max_w,
                );
                ContentOutcome::leaf(outcome, self.units, constraints)
            }
            GuiNodeContent::Drawing => {
                let resource = style
                    .asset
                    .as_ref()
                    .and_then(|source| self.resolver.surface_resource(source));
                if resource.is_none() {
                    self.diagnostics.push(GuiLayoutDiagnostic::MissingResource {
                        node: node.id,
                        kind: "drawing",
                    });
                }

                ContentOutcome {
                    size: ZERO_SIZE,
                    content: GuiEvaluatedContent::Drawing {
                        drawing: resource,
                    },
                    available: true,
                    content_extents: None,
                    viewport: None,
                }
            }
            GuiNodeContent::Image {
                size,
            } => {
                let resource = style
                    .asset
                    .as_ref()
                    .and_then(|source| self.resolver.surface_resource(source));
                if resource.is_none() {
                    self.diagnostics.push(GuiLayoutDiagnostic::MissingResource {
                        node: node.id,
                        kind: "image",
                    });
                }

                let logical = [size[0] * self.units, size[1] * self.units];
                ContentOutcome {
                    size: [
                        constraints.clamp_width(logical[0]),
                        constraints.clamp_height(logical[1]),
                    ],
                    content: GuiEvaluatedContent::Image {
                        bitmap: resource,
                    },
                    available: true,
                    content_extents: None,
                    viewport: None,
                }
            }
            GuiNodeContent::Button {
                label,
            } => {
                let outcome = self.measure_leaf(
                    node.id,
                    label,
                    TextLinePolicy::SingleLine,
                    style,
                    constraints.max_w,
                );
                ContentOutcome::leaf(outcome, self.units, constraints).map_content(
                    |layout, font, font_size, text| GuiEvaluatedContent::Button {
                        layout,
                        font,
                        font_size,
                        label: text,
                    },
                )
            }
            GuiNodeContent::Checkbox {
                checked,
            } => {
                let em = style.font_size * self.units;
                let edge = 1.4 * em;
                let (checked, revision) = match effective_control(self.root, node.id) {
                    Some((GuiControlValue::Bool(committed), revision)) => (committed, revision),
                    _ => (*checked, 0),
                };
                ContentOutcome {
                    size: [
                        constraints.clamp_width(edge),
                        constraints.clamp_height(edge),
                    ],
                    content: GuiEvaluatedContent::Checkbox {
                        checked,
                        revision,
                    },
                    available: true,
                    content_extents: None,
                    viewport: None,
                }
            }
            GuiNodeContent::Slider {
                value,
                min,
                max,
                step,
            } => {
                let em = style.font_size * self.units;
                let size = [8.0 * em, 1.4 * em];
                let (value, revision) = match effective_control(self.root, node.id) {
                    Some((GuiControlValue::Scalar(committed), revision)) => (committed, revision),
                    _ => (*value, 0),
                };
                ContentOutcome {
                    size: [
                        constraints.clamp_width(size[0]),
                        constraints.clamp_height(size[1]),
                    ],
                    content: GuiEvaluatedContent::Slider {
                        value,
                        min: *min,
                        max: *max,
                        step: *step,
                        revision,
                    },
                    available: true,
                    content_extents: None,
                    viewport: None,
                }
            }
            GuiNodeContent::TextInput {
                text,
                placeholder,
            } => {
                let (effective, revision) = match effective_control(self.root, node.id) {
                    Some((GuiControlValue::Text(committed), revision)) => (committed, revision),
                    _ => (text.clone(), 0),
                };
                let measured = if effective.is_empty() {
                    placeholder
                } else {
                    &effective
                };
                let outcome = self.measure_leaf(
                    node.id,
                    measured,
                    TextLinePolicy::SingleLine,
                    style,
                    constraints.max_w,
                );
                ContentOutcome::leaf(outcome, self.units, constraints).map_content(
                    |layout, font, font_size, text| GuiEvaluatedContent::TextInput {
                        layout,
                        font,
                        font_size,
                        text,
                        revision,
                    },
                )
            }
        }
    }

    /// Measure one text leaf, reusing the retained layout when its cache
    /// key is unchanged. Paint-only edits never reach measurement.
    fn measure_leaf(
        &mut self,
        id: GuiNodeId,
        text: &str,
        policy: TextLinePolicy,
        style: &GuiNodeStyle,
        max_width: f32,
    ) -> LeafOutcome {
        let Some(source) = style.asset.as_ref() else {
            self.diagnostics.push(GuiLayoutDiagnostic::MissingResource {
                node: id,
                kind: "font",
            });
            return LeafOutcome::unavailable();
        };
        let em_to_logical = style.font_size * self.units;
        if !(em_to_logical.is_finite() && em_to_logical > 0.0) {
            self.diagnostics
                .push(GuiLayoutDiagnostic::InvalidConstraints {
                    node: Some(id),
                    detail: GuiConstraintError::NonFinite,
                });
            return LeafOutcome::unavailable();
        }

        let max_width_ems = if max_width.is_finite() {
            TextMaxWidth::Ems((max_width / em_to_logical).max(0.001))
        } else {
            TextMaxWidth::Unbounded
        };

        let resolution = self.resolver.text_font(source);
        let (key, font) = match resolution {
            GuiFontResolution::Ready {
                key,
                font,
            } => (key, font),
            GuiFontResolution::Pending {
                ..
            } => {
                self.diagnostics.push(GuiLayoutDiagnostic::PendingText {
                    node: id,
                });
                return LeafOutcome::unavailable();
            }
            GuiFontResolution::Missing => {
                self.diagnostics.push(GuiLayoutDiagnostic::MissingResource {
                    node: id,
                    kind: "font",
                });
                return LeafOutcome::unavailable();
            }
        };

        let request = TextMeasureRequest::new(
            text,
            TextFont::Ready {
                key,
                font,
            },
            style.font_size,
            policy,
            max_width_ems,
        );
        let Ok(request) = request else {
            self.diagnostics
                .push(GuiLayoutDiagnostic::InvalidConstraints {
                    node: Some(id),
                    detail: GuiConstraintError::NonFinite,
                });
            return LeafOutcome::unavailable();
        };

        let cache_key = request.cache_key();
        let owned = OwnedTextKey {
            text: text.to_owned(),
            font: cache_key.font,
            line_policy: policy,
            max_width_bits: cache_key.max_width_bits,
            font_size_bits: cache_key.font_size_bits,
        };
        if let Some(retained) = self.texts.get(&id)
            && retained.key == owned
        {
            let layout = retained.layout.clone();
            let resource = SurfaceRenderResource {
                key,
                source: source.clone(),
            };
            return LeafOutcome::measured(layout, resource, style.font_size, text.to_owned());
        }

        let TextOutcome::Measured(layout) = measure_text(&request) else {
            self.diagnostics.push(GuiLayoutDiagnostic::PendingText {
                node: id,
            });
            return LeafOutcome::unavailable();
        };
        self.remeasured += 1;
        self.texts.insert(
            id,
            RetainedText {
                key: owned,
                layout: layout.clone(),
            },
        );
        LeafOutcome::measured(
            layout,
            SurfaceRenderResource {
                key,
                source: source.clone(),
            },
            style.font_size,
            text.to_owned(),
        )
    }

    /// Push an unavailable placeholder record for a node that cannot lay
    /// out. The zero rectangle keeps paint and hit testing quiet while the
    /// diagnostic names the cause.
    fn push_unavailable(
        &mut self,
        node: &GuiNode,
        slot_final: [f32; 2],
        depth: u32,
        diagnostic: GuiLayoutDiagnostic,
    ) {
        self.diagnostics.push(diagnostic);
        let style = self.root.style(node.id).unwrap_or_default();
        let enabled = style.enabled;
        self.placements.push(DeferredPlacement {
            placeholder: true,
            ..DeferredPlacement::default()
        });
        self.nodes.push(GuiEvaluatedNode {
            node: node.id,
            lifetime: node.lifetime,
            depth,
            rect: [slot_final[0], slot_final[1], 0.0, 0.0],
            clip: None,
            content: GuiEvaluatedContent::Container,
            enabled,
            visible: false,
            available: false,
            paint_suppressed: true,
            visual_offset: [0.0, 0.0],
            visual_scale: [1.0, 1.0],
            acc_scale: [1.0, 1.0],
            content_extents: None,
            content_origin: [slot_final[0], slot_final[1]],
            color: style.color,
            background: style.background_color,
            opacity: style.opacity,
        });
    }
}

/// Local content outcome: node-local logical size, retained payload,
/// availability, optional scroll content extents and, for a ScrollView, its
/// provisional final-logical viewport clip.
struct ContentOutcome {
    size: [f32; 2],
    content: GuiEvaluatedContent,
    available: bool,
    content_extents: Option<[f32; 2]>,
    viewport: Option<SurfaceClipRect>,
}

impl ContentOutcome {
    fn leaf(outcome: LeafOutcome, units: f32, constraints: Constraints) -> Self {
        match outcome {
            LeafOutcome::Measured {
                layout,
                font,
                font_size,
                text,
            } => {
                // Em extents scale to logical units by metres-per-em and
                // the evaluation's units factor.
                let em_to_logical = font_size * units;
                let size = [
                    constraints.clamp_width(layout.size[0] * em_to_logical),
                    constraints.clamp_height(layout.size[1] * em_to_logical),
                ];
                Self {
                    size,
                    content: GuiEvaluatedContent::Text {
                        layout,
                        font,
                        font_size,
                        text,
                    },
                    available: true,
                    content_extents: None,
                    viewport: None,
                }
            }
            LeafOutcome::Unavailable => Self {
                size: ZERO_SIZE,
                content: GuiEvaluatedContent::Container,
                available: false,
                content_extents: None,
                viewport: None,
            },
        }
    }

    /// Reinterpret a measured text leaf as a control payload.
    fn map_content(
        self,
        map: impl FnOnce(TextLayout, SurfaceRenderResource, f32, String) -> GuiEvaluatedContent,
    ) -> Self {
        match self.content {
            GuiEvaluatedContent::Text {
                layout,
                font,
                font_size,
                text,
            } => Self {
                content: map(layout, font, font_size, text),
                ..self
            },
            _ => self,
        }
    }
}

/// Measured or unavailable text leaf result.
enum LeafOutcome {
    Measured {
        layout: TextLayout,
        font: SurfaceRenderResource,
        font_size: f32,
        text: String,
    },
    Unavailable,
}

impl LeafOutcome {
    fn measured(
        layout: TextLayout,
        font: SurfaceRenderResource,
        font_size: f32,
        text: String,
    ) -> Self {
        Self::Measured {
            layout,
            font,
            font_size,
            text,
        }
    }

    fn unavailable() -> Self {
        Self::Unavailable
    }
}

// ---------------------------------------------------------------------------
// Container layout: Row, Column, Stack, Padding, Align, SizedBox, ScrollView.
// ---------------------------------------------------------------------------

impl<'a, 'r> Evaluator<'a, 'r> {
    /// Lay out a container's children in node-local logical units.
    /// `origin_final` is the content-box origin in final-logical
    /// coordinates; `acc_total` includes this node's own visual scale.
    #[allow(clippy::too_many_arguments)]
    fn layout_container(
        &mut self,
        node: &GuiNode,
        style: &GuiNodeStyle,
        kind: GuiContainerKind,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        match kind {
            GuiContainerKind::Row => self.layout_flex(
                node,
                true,
                constraints,
                fill,
                origin_final,
                acc_total,
                depth,
            ),
            GuiContainerKind::Column => self.layout_flex(
                node,
                false,
                constraints,
                fill,
                origin_final,
                acc_total,
                depth,
            ),
            GuiContainerKind::Stack => {
                self.layout_stack(node, constraints, fill, origin_final, acc_total, depth)
            }
            GuiContainerKind::Padding => {
                self.layout_single(node, constraints, fill, origin_final, acc_total, depth)
            }
            GuiContainerKind::Align => self.layout_align(
                node,
                style,
                constraints,
                fill,
                origin_final,
                acc_total,
                depth,
            ),
            GuiContainerKind::SizedBox => {
                self.layout_sized_box(node, style, constraints, origin_final, acc_total, depth)
            }
            GuiContainerKind::ScrollView => {
                self.layout_scroll(node, constraints, fill, origin_final, acc_total, depth)
            }
        }
    }

    /// Final-logical coordinates of a content-box-local point.
    fn place(origin_final: [f32; 2], local: [f32; 2], acc_total: [f32; 2]) -> [f32; 2] {
        [
            origin_final[0] + local[0] * acc_total[0],
            origin_final[1] + local[1] * acc_total[1],
        ]
    }

    /// Visit one child, returning its node-local size and the index of the
    /// record it retained, or None when the child produced no record.
    fn visit_child(
        &mut self,
        id: GuiNodeId,
        constraints: Constraints,
        slot_final: [f32; 2],
        acc: [f32; 2],
        depth: u32,
    ) -> ([f32; 2], Option<usize>) {
        let index = self.nodes.len();
        let size = self.visit(id, constraints, slot_final, acc, depth);
        let retained = self
            .nodes
            .get(index)
            .is_some_and(|record| record.node == id);

        (size, retained.then_some(index))
    }

    /// Defer moving a child's complete subtree by a content-box-local
    /// alignment offset. The finishing pass applies the translation to
    /// every descendant and resolves clips against the final geometry, so
    /// alignment never leaves descendants behind their aligned parent.
    fn align_subtree(&mut self, index: Option<usize>, local: [f32; 2], acc_total: [f32; 2]) {
        let Some(placement) = index.and_then(|index| self.placements.get_mut(index)) else {
            return;
        };

        placement.shift[0] += local[0] * acc_total[0];
        placement.shift[1] += local[1] * acc_total[1];
    }

    /// Outer margins of one child in logical units.
    fn child_margin(&self, id: GuiNodeId) -> [f32; 4] {
        styled(self.root, id, self.units).margin.unwrap_or([0.0; 4])
    }

    /// Positive flex factor of one child, or None for fixed children.
    /// Non-finite or negative factors diagnose and behave as fixed.
    fn child_flex(&mut self, id: GuiNodeId) -> Option<f32> {
        let flex = self.root.style(id).and_then(|style| style.flex)?;
        if !flex.is_finite() || flex < 0.0 {
            self.diagnostics
                .push(GuiLayoutDiagnostic::InvalidConstraints {
                    node: Some(id),
                    detail: if flex.is_finite() {
                        GuiConstraintError::Negative
                    } else {
                        GuiConstraintError::NonFinite
                    },
                });
            return None;
        }

        if flex > 0.0 {
            Some(flex)
        } else {
            None
        }
    }

    /// Row or Column layout. Fixed children measure first against remaining
    /// space; flex children then share the leftover proportionally. Every
    /// child then occupies its main-axis slot in tree order, so a flex
    /// spacer between fixed children separates them. Cross children align
    /// by their own align lane within the largest child cross extent,
    /// defaulting to the start.
    #[allow(clippy::too_many_arguments)]
    fn layout_flex(
        &mut self,
        node: &GuiNode,
        horizontal: bool,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let (max_main, max_cross) = if horizontal {
            (constraints.max_w, constraints.max_h)
        } else {
            (constraints.max_h, constraints.max_w)
        };

        // Classify children in tree order without measuring yet.
        let children: Vec<(GuiNodeId, Option<f32>)> = node
            .children
            .iter()
            .map(|&child| (child, self.child_flex(child)))
            .collect();
        let flex_total: f32 = children.iter().filter_map(|(_, flex)| *flex).sum();

        let records_base = self.nodes.len();
        let mut cursor = 0.0;
        let mut cross = 0.0f32;
        let mut available = true;
        let mut placed: Vec<Option<FlexPlacement>> = Vec::new();
        placed.resize_with(children.len(), || None);

        // Fixed children first, bounded by remaining main-axis space so
        // wrapping text observes the space it will actually occupy.
        for (position, &(child, _)) in children
            .iter()
            .enumerate()
            .filter(|(_, (_, flex))| flex.is_none())
        {
            let margin = self.child_margin(child);
            let (margin_main_start, margin_main_end, margin_cross_start) = if horizontal {
                (margin[3], margin[1], margin[0])
            } else {
                (margin[0], margin[2], margin[3])
            };
            let remaining = if max_main.is_finite() {
                (max_main - cursor - margin_main_start - margin_main_end).max(0.0)
            } else {
                f32::INFINITY
            };
            let child_constraints = if horizontal {
                Constraints::loose(remaining, max_cross)
            } else {
                Constraints::loose(max_cross, remaining)
            };
            let slot_local = if horizontal {
                [cursor + margin_main_start, margin_cross_start]
            } else {
                [margin_cross_start, cursor + margin_main_start]
            };
            let first_record = self.nodes.len();
            let (size, index) = self.visit_child(
                child,
                child_constraints,
                Self::place(origin_final, slot_local, acc_total),
                acc_total,
                depth + 1,
            );
            let (main, cross_size) = if horizontal {
                (size[0], size[1])
            } else {
                (size[1], size[0])
            };
            let extent = margin_main_start + main + margin_main_end;
            placed[position] = Some(FlexPlacement {
                child,
                index,
                records: first_record..self.nodes.len(),
                measured_at: cursor,
                extent,
                cross: cross_size,
            });
            cursor += extent;
            cross = cross.max(cross_size);
            available &= self.child_available(child);
        }

        // Flex children share whatever main-axis space is left. Leftover
        // space is clamped at zero: over-constrained flex children keep
        // their min-clamped share instead of going negative.
        let leftover = if max_main.is_finite() {
            (max_main - cursor).max(0.0)
        } else {
            0.0
        };
        for (position, &(child, flex)) in children.iter().enumerate() {
            let Some(flex) = flex else {
                continue;
            };
            let margin = self.child_margin(child);
            let (margin_main_start, margin_main_end, margin_cross_start) = if horizontal {
                (margin[3], margin[1], margin[0])
            } else {
                (margin[0], margin[2], margin[3])
            };
            let share = if max_main.is_finite() && flex_total > 0.0 {
                leftover * flex / flex_total
            } else {
                // Unbounded main axis: flex has no meaning. Diagnose once
                // per child and keep zero main-axis extent.
                self.diagnostics.push(GuiLayoutDiagnostic::UnboundedFlex {
                    node: child,
                });
                0.0
            };
            let share = (share - margin_main_start - margin_main_end).max(0.0);
            let child_constraints = if horizontal {
                Constraints {
                    min_w: share,
                    min_h: 0.0,
                    max_w: share,
                    max_h: max_cross,
                }
            } else {
                Constraints {
                    min_w: 0.0,
                    min_h: share,
                    max_w: max_cross,
                    max_h: share,
                }
            };
            let slot_local = if horizontal {
                [cursor + margin_main_start, margin_cross_start]
            } else {
                [margin_cross_start, cursor + margin_main_start]
            };
            let first_record = self.nodes.len();
            let (size, index) = self.visit_child(
                child,
                child_constraints,
                Self::place(origin_final, slot_local, acc_total),
                acc_total,
                depth + 1,
            );
            let (main, cross_size) = if horizontal {
                (size[0], size[1])
            } else {
                (size[1], size[0])
            };
            let extent = margin_main_start + main + margin_main_end;
            placed[position] = Some(FlexPlacement {
                child,
                index,
                records: first_record..self.nodes.len(),
                measured_at: cursor,
                extent,
                cross: cross_size,
            });
            cursor += extent;
            cross = cross.max(cross_size);
            available &= self.child_available(child);
        }

        // Measurement visited fixed children before flex ones; restore tree
        // order for painter order and main-axis slots without remeasuring.
        let mut placed: Vec<FlexPlacement> = placed.into_iter().flatten().collect();
        self.order_child_records(records_base, &mut placed);

        // Each child's whole subtree moves from its measured slot to its
        // tree-order slot, and on the cross axis by its own align lane.
        let mut slot = 0.0;
        for placement in placed {
            let child_style = self.root.style(placement.child).unwrap_or_default();
            let factor = if horizontal {
                align_factor(child_style.align_y, -1.0)
            } else {
                align_factor(child_style.align_x, -1.0)
            };
            let main_shift = slot - placement.measured_at;
            let cross_shift = factor * (cross - placement.cross).max(0.0);
            let local = if horizontal {
                [main_shift, cross_shift]
            } else {
                [cross_shift, main_shift]
            };
            self.align_subtree(placement.index, local, acc_total);
            slot += placement.extent;
        }

        let (main_size, cross_size) = if horizontal {
            (cursor, cross)
        } else {
            (cross, cursor)
        };
        let size = [
            fill_or_fit(main_size, fill[0]),
            fill_or_fit(cross_size, fill[1]),
        ];
        ContentOutcome {
            size,
            content: GuiEvaluatedContent::Container,
            available,
            content_extents: None,
            viewport: None,
        }
    }

    /// Rearrange the contiguous record ranges that sibling visits pushed
    /// after `base` into the order of `placed`, moving each child's complete
    /// subtree with its deferred placement and updating its record index.
    fn order_child_records(&mut self, base: usize, placed: &mut [FlexPlacement]) {
        if placed
            .windows(2)
            .all(|pair| pair[0].records.start <= pair[1].records.start)
        {
            return;
        }

        let mut nodes: Vec<_> = self.nodes.drain(base..).map(Some).collect();
        let mut deferred: Vec<_> = self.placements.drain(base..).map(Some).collect();
        for placement in placed {
            let start = self.nodes.len();
            let range = placement.records.start - base..placement.records.end - base;
            self.nodes.extend(
                nodes[range.clone()]
                    .iter_mut()
                    .map(|record| record.take().expect("record moves once")),
            );
            self.placements.extend(
                deferred[range]
                    .iter_mut()
                    .map(|record| record.take().expect("placement moves once")),
            );
            placement.index = placement.index.map(|_| start);
            placement.records = start..self.nodes.len();
        }
    }

    /// Whether a just-laid-out child record is available. Looks up the
    /// record pushed by the matching visit; defaults to unavailable when
    /// the child produced no record.
    fn child_available(&self, id: GuiNodeId) -> bool {
        self.nodes
            .iter()
            .rev()
            .find(|node| node.node == id)
            .is_none_or(|node| node.available)
    }

    /// Stack layout: every child observes the content box minus its margins
    /// and aligns its margin box within the settled extent, moving its whole
    /// subtree. The stack fits outer child sizes.
    fn layout_stack(
        &mut self,
        node: &GuiNode,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let mut extent = [0.0f32, 0.0];
        let mut available = true;
        let mut placed: Vec<(GuiNodeId, Option<usize>, [f32; 2])> = Vec::new();
        for &child in &node.children {
            let margin = self.child_margin(child);
            let outer = [margin[3] + margin[1], margin[0] + margin[2]];
            let (size, index) = self.visit_child(
                child,
                Constraints::loose(
                    (constraints.max_w - outer[0]).max(0.0),
                    (constraints.max_h - outer[1]).max(0.0),
                ),
                Self::place(origin_final, [margin[3], margin[0]], acc_total),
                acc_total,
                depth + 1,
            );
            let size = [(size[0] + outer[0]).max(0.0), (size[1] + outer[1]).max(0.0)];
            extent[0] = extent[0].max(size[0]);
            extent[1] = extent[1].max(size[1]);
            placed.push((child, index, size));
            available &= self.child_available(child);
        }

        let size = [
            fill_or_fit(extent[0], fill[0]),
            fill_or_fit(extent[1], fill[1]),
        ];

        // Children were visited at their leading margins; alignment only
        // adds the offset of the margin box within the settled extent.
        for (child, index, child_size) in placed {
            let child_style = self.root.style(child).unwrap_or_default();
            let fx = align_factor(child_style.align_x, -1.0);
            let fy = align_factor(child_style.align_y, -1.0);
            let local = [
                fx * (size[0] - child_size[0]).max(0.0),
                fy * (size[1] - child_size[1]).max(0.0),
            ];
            self.align_subtree(index, local, acc_total);
        }

        ContentOutcome {
            size,
            content: GuiEvaluatedContent::Container,
            available,
            content_extents: None,
            viewport: None,
        }
    }

    /// Padding and single-child pass-through: the first child fills the
    /// content box; extra children are ignored with a diagnostic.
    fn layout_single(
        &mut self,
        node: &GuiNode,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let mut children = node.children.iter();
        let Some(&child) = children.next() else {
            return ContentOutcome {
                size: [fill_or_fit(0.0, fill[0]), fill_or_fit(0.0, fill[1])],
                content: GuiEvaluatedContent::Container,
                available: true,
                content_extents: None,
                viewport: None,
            };
        };
        if children.next().is_some() {
            self.diagnostics.push(GuiLayoutDiagnostic::Unsupported {
                node: Some(node.id),
                detail: "extra-padding-child",
            });
        }

        let size = self.visit(
            child,
            Constraints::loose(constraints.max_w, constraints.max_h),
            origin_final,
            acc_total,
            depth + 1,
        );
        let available = self.child_available(child);
        ContentOutcome {
            size: [fill_or_fit(size[0], fill[0]), fill_or_fit(size[1], fill[1])],
            content: GuiEvaluatedContent::Container,
            available,
            content_extents: None,
            viewport: None,
        }
    }

    /// Align: the single child keeps its intrinsic size and its whole
    /// subtree is positioned within the filled content box by the node's own
    /// align lanes, defaulting to the center.
    #[allow(clippy::too_many_arguments)]
    fn layout_align(
        &mut self,
        node: &GuiNode,
        style: &GuiNodeStyle,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let size = [fill_or_fit(0.0, fill[0]), fill_or_fit(0.0, fill[1])];
        let mut children = node.children.iter();
        let Some(&child) = children.next() else {
            return ContentOutcome {
                size,
                content: GuiEvaluatedContent::Container,
                available: true,
                content_extents: None,
                viewport: None,
            };
        };

        let (child_size, index) = self.visit_child(
            child,
            Constraints::loose(
                size[0].min(constraints.max_w),
                size[1].min(constraints.max_h),
            ),
            origin_final,
            acc_total,
            depth + 1,
        );
        let fx = align_factor(style.align_x, 0.0);
        let fy = align_factor(style.align_y, 0.0);
        let local = [
            fx * (size[0] - child_size[0]).max(0.0),
            fy * (size[1] - child_size[1]).max(0.0),
        ];
        self.align_subtree(index, local, acc_total);

        ContentOutcome {
            size,
            content: GuiEvaluatedContent::Container,
            available: self.child_available(child),
            content_extents: None,
            viewport: None,
        }
    }

    /// SizedBox: explicit lanes size the box; an unspecified axis fits the
    /// single child when present. The child observes tight box constraints.
    fn layout_sized_box(
        &mut self,
        node: &GuiNode,
        style: &GuiNodeStyle,
        constraints: Constraints,
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let explicit_w = sanitized_bound(style.width, Some(node.id), &mut self.diagnostics);
        let explicit_h = sanitized_bound(style.height, Some(node.id), &mut self.diagnostics);
        let child = node.children.first().copied();

        // Measure the child under explicit bounds when both axes are set so
        // intrinsic content (text) can wrap; otherwise measure loose and
        // fit the open axis to the child.
        let probe = Constraints::loose(
            explicit_w.unwrap_or(constraints.max_w),
            explicit_h.unwrap_or(constraints.max_h),
        );
        let (child_size, available) = match child {
            Some(child) => {
                let size = self.visit(child, probe, origin_final, acc_total, depth + 1);
                (size, self.child_available(child))
            }
            None => (ZERO_SIZE, true),
        };

        let size = [
            explicit_w.unwrap_or(child_size[0]),
            explicit_h.unwrap_or(child_size[1]),
        ];
        ContentOutcome {
            size,
            content: GuiEvaluatedContent::Container,
            available,
            content_extents: None,
            viewport: None,
        }
    }

    /// ScrollView: the viewport fills its constraints while the single
    /// content child measures with an unbounded height (vertical scroll).
    /// The viewport clips its content once placement resolves; extents are
    /// retained for the scrolling behavior that owns offsets.
    fn layout_scroll(
        &mut self,
        node: &GuiNode,
        constraints: Constraints,
        fill: [f32; 2],
        origin_final: [f32; 2],
        acc_total: [f32; 2],
        depth: u32,
    ) -> ContentOutcome {
        let viewport = [fill_or_fit(0.0, fill[0]), fill_or_fit(0.0, fill[1])];

        // Viewport rectangle in provisional final-logical coordinates. It
        // moves with any alignment of this subtree, and the finishing pass
        // intersects it with the fixed clip the ScrollView inherits.
        let viewport_clip = normalize_clip([
            origin_final[0],
            origin_final[1],
            origin_final[0] + viewport[0] * acc_total[0],
            origin_final[1] + viewport[1] * acc_total[1],
        ]);
        let mut children = node.children.iter();
        let Some(&child) = children.next() else {
            return ContentOutcome {
                size: viewport,
                content: GuiEvaluatedContent::Container,
                available: true,
                content_extents: Some(ZERO_SIZE),
                viewport: viewport_clip,
            };
        };
        if children.next().is_some() {
            self.diagnostics.push(GuiLayoutDiagnostic::Unsupported {
                node: Some(node.id),
                detail: "extra-scroll-child",
            });
        }

        let content_size = self.visit(
            child,
            Constraints {
                min_w: 0.0,
                min_h: 0.0,
                max_w: viewport[0].min(constraints.max_w),
                max_h: f32::INFINITY,
            },
            origin_final,
            acc_total,
            depth + 1,
        );

        ContentOutcome {
            size: viewport,
            content: GuiEvaluatedContent::Container,
            available: self.child_available(child),
            content_extents: Some(content_size),
            viewport: viewport_clip,
        }
    }

    /// Apply deferred alignment and resolve clips in painter order.
    ///
    /// Records are in pre-order, so each record's parent is the nearest
    /// preceding record one level shallower. A record moves by the sum of
    /// its own and its ancestors' deferred translations. It inherits its
    /// parent's clip for its children, narrowed by the parent's translated
    /// ScrollView viewport, and the root inherits `root_clip`. An empty
    /// intersection stays an explicit empty clip so the subtree suppresses
    /// paint and hits instead of escaping its ancestors.
    fn resolve_placement(&mut self, root_clip: Option<SurfaceClipRect>) {
        // (depth, accumulated translation, clip inherited by children)
        let mut ancestors: Vec<(u32, [f32; 2], Option<SurfaceClipRect>)> = Vec::new();
        for (record, placement) in self.nodes.iter_mut().zip(&self.placements) {
            while ancestors
                .last()
                .is_some_and(|(depth, _, _)| *depth >= record.depth)
            {
                ancestors.pop();
            }
            let (inherited, clip) = ancestors
                .last()
                .map_or(([0.0, 0.0], root_clip), |(_, shift, clip)| (*shift, *clip));
            let shift = [
                inherited[0] + placement.shift[0],
                inherited[1] + placement.shift[1],
            ];

            record.rect[0] += shift[0];
            record.rect[1] += shift[1];
            record.content_origin[0] += shift[0];
            record.content_origin[1] += shift[1];
            record.clip = clip;
            record.paint_suppressed = placement.placeholder
                || clip.is_some_and(crate::systems::surface::surface_clip_is_empty);

            let viewport = placement.viewport.map(|viewport| {
                [
                    viewport[0] + shift[0],
                    viewport[1] + shift[1],
                    viewport[2] + shift[0],
                    viewport[3] + shift[1],
                ]
            });
            let children_clip = match (clip, viewport) {
                (Some(outer), Some(inner)) => Some(nested_clip(outer, inner)),
                (outer, None) => outer,
                (None, inner) => inner,
            };
            ancestors.push((record.depth, shift, children_clip));
        }
    }
}

/// One measured Row or Column child awaiting its tree-order slot and
/// cross-axis alignment.
struct FlexPlacement {
    child: GuiNodeId,
    /// Index of the child's own record, when it retained one.
    index: Option<usize>,
    /// Records this child's visit pushed, parent first.
    records: std::ops::Range<usize>,
    /// Main-axis offset of the margin box while measuring.
    measured_at: f32,
    /// Main-axis margin-box extent.
    extent: f32,
    cross: f32,
}

/// Intersect a nested viewport with its inherited clip. Unlike
/// [`intersect_surface_clips`], a disjoint or empty result stays an explicit
/// empty clip, so descendants remain suppressed rather than unclipped.
fn nested_clip(outer: SurfaceClipRect, inner: SurfaceClipRect) -> SurfaceClipRect {
    intersect_surface_clips(outer, inner).unwrap_or([
        outer[0].max(inner[0]),
        outer[1].max(inner[1]),
        outer[0].max(inner[0]),
        outer[1].max(inner[1]),
    ])
}

/// Normalize possibly mirrored clip bounds into `[min_x, min_y, max_x,
/// max_y]` order. Returns None for non-finite bounds.
fn normalize_clip(clip: SurfaceClipRect) -> Option<SurfaceClipRect> {
    if !clip.iter().all(|value| value.is_finite()) {
        return None;
    }

    Some([
        clip[0].min(clip[2]),
        clip[1].min(clip[3]),
        clip[0].max(clip[2]),
        clip[1].max(clip[3]),
    ])
}

/// Fill bounded axes to their maximum, fit unbounded axes to content.
fn fill_or_fit(content: f32, max: f32) -> f32 {
    if max.is_finite() {
        max.max(0.0)
    } else {
        content.max(0.0)
    }
}

/// Map an align lane in `-1.0..=1.0` to a `0.0..=1.0` interpolation factor.
/// Missing lanes take the caller-supplied default; out-of-range values
/// clamp instead of failing layout.
fn align_factor(value: Option<f32>, default: f32) -> f32 {
    match value {
        Some(value) if value.is_finite() => ((value + 1.0) / 2.0).clamp(0.0, 1.0),
        _ => ((default + 1.0) / 2.0).clamp(0.0, 1.0),
    }
}

// ---------------------------------------------------------------------------
// Input fingerprints.
// ---------------------------------------------------------------------------

/// Hash one node's geometry-affecting structure and effective text.
///
/// Checkbox/slider values and control revisions do not move geometry. They
/// live in the state/paint fingerprint so frequent interaction refreshes the
/// evaluated payload without reflow. Text-input content remains structural
/// because its measured text can change intrinsic size.
fn hash_node_structure(hasher: &mut Fingerprint, root: &GuiRoot, id: GuiNodeId) {
    let Some(node) = root.nodes().node(id) else {
        return;
    };
    hasher.u32(node.id.0);
    hasher.u32(node.lifetime);
    hasher.u32(node.parent.map(|parent| parent.0).unwrap_or(u32::MAX));
    hasher.u32(node.children.len() as u32);
    for child in &node.children {
        hasher.u32(child.0);
    }

    match &node.content {
        GuiNodeContent::Container(kind) => {
            hasher.u32(0);
            hasher.u32(*kind as u32);
        }
        GuiNodeContent::Text(text) => {
            hasher.u32(1);
            hasher.string(text);
        }
        GuiNodeContent::Drawing => hasher.u32(2),
        GuiNodeContent::Image {
            size,
        } => {
            hasher.u32(3);
            hasher.f32(size[0]);
            hasher.f32(size[1]);
        }
        GuiNodeContent::Button {
            label,
        } => {
            hasher.u32(4);
            hasher.string(label);
        }
        GuiNodeContent::Checkbox {
            ..
        } => {
            hasher.u32(5);
        }
        GuiNodeContent::Slider {
            ..
        } => {
            hasher.u32(6);
        }
        GuiNodeContent::TextInput {
            text,
            placeholder,
        } => {
            hasher.u32(7);
            let effective = match effective_control(root, id) {
                Some((GuiControlValue::Text(committed), _)) => committed,
                _ => text.clone(),
            };
            hasher.string(&effective);
            hasher.string(placeholder);
        }
    }
}

/// Hash layout-affecting style lanes: explicit and min/max sizes, flex,
/// alignment, padding, margins, font size, asset identity and resource
/// generations. Visual translation/scale live in the visual
/// lane below, and colour and opacity stay paint-only, so neither visual
/// nor paint edits remeasure text or reflow layout.
fn hash_node_layout(
    hasher: &mut Fingerprint,
    style: &GuiNodeStyle,
    resolver: &dyn GuiResourceResolver,
) {
    hash_option_f32(hasher, style.width);
    hash_option_f32(hasher, style.height);
    hash_option_f32(hasher, style.min_width);
    hash_option_f32(hasher, style.min_height);
    hash_option_f32(hasher, style.max_width);
    hash_option_f32(hasher, style.max_height);
    hash_option_f32(hasher, style.flex);
    hash_option_f32(hasher, style.align_x);
    hash_option_f32(hasher, style.align_y);
    hash_option_vec4(hasher, style.padding);
    hash_option_vec4(hasher, style.margin);
    hasher.f32(style.font_size);
    hash_asset(hasher, style.asset.as_ref());
    // Readiness and replacement generations invalidate retained
    // measurement even when the authored reference is unchanged.
    match style
        .asset
        .as_ref()
        .and_then(|source| resolver.resource_generation(source))
    {
        Some(generation) => {
            hasher.u32(1);
            hasher.u64(generation);
        }
        None => hasher.u32(0),
    }
}

/// Hash visual-only lanes: translation and scale move paint and hit regions
/// together without remeasuring text or reflowing layout.
fn hash_node_visual(hasher: &mut Fingerprint, lanes: &GuiNodeLanes) {
    hasher.f32(lanes.position[0]);
    hasher.f32(lanes.position[1]);
    hasher.f32(lanes.scale[0]);
    hasher.f32(lanes.scale[1]);
}

/// Hash paint-only lanes over the layout hash.
fn hash_node_paint(
    hasher: &mut Fingerprint,
    root: &GuiRoot,
    id: GuiNodeId,
    style: &GuiNodeStyle,
    resolver: &dyn GuiResourceResolver,
) {
    for lane in style.color {
        hasher.f32(lane);
    }
    hash_option_vec4(hasher, style.background_color);
    hasher.f32(style.opacity);
    hasher.u32(u32::from(style.enabled));
    hash_control_state(hasher, root, id);
    hash_node_part_paint(hasher, root, id, resolver);
}

/// Hash non-geometric control payload and revisions for retained state refresh.
fn hash_control_state(hasher: &mut Fingerprint, root: &GuiRoot, id: GuiNodeId) {
    let Some(node) = root.nodes().node(id) else {
        return;
    };
    match &node.content {
        GuiNodeContent::Checkbox {
            checked,
        } => {
            hasher.u32(1);
            hasher.u32(u32::from(*checked));
        }
        GuiNodeContent::Slider {
            value,
            min,
            max,
            step,
        } => {
            hasher.u32(2);
            hasher.f32(*value);
            hasher.f32(*min);
            hasher.f32(*max);
            hasher.f32(*step);
        }
        GuiNodeContent::TextInput {
            ..
        } => hasher.u32(3),
        _ => hasher.u32(0),
    }
    match root.controls().get(id) {
        Some(state) => {
            hasher.u32(1);
            hasher.u32(state.revision);
            match &state.value {
                GuiControlValue::None => hasher.u32(0),
                GuiControlValue::Bool(value) => {
                    hasher.u32(1);
                    hasher.u32(u32::from(*value));
                }
                GuiControlValue::Scalar(value) => {
                    hasher.u32(2);
                    hasher.f32(*value);
                }
                GuiControlValue::Text(value) => {
                    hasher.u32(3);
                    hasher.string(value);
                }
            }
        }
        None => hasher.u32(0),
    }
}

/// Hash named skin-part lanes into the paint fingerprint.
///
/// Skins resolve at render preparation from live part properties and
/// input cursors, outside retained layout evaluation. Without these
/// lanes a reskin leaves every fingerprint still and needs an unrelated
/// trigger to repaint. All part states hash together: any part edit
/// invalidates paint while cursors select the visible state, so paint
/// rebuilds without remeasuring text or reflowing layout. Each lane hashes
/// its name, storage type and stored bytes, read through one ordered scan.
fn hash_node_part_paint(
    hasher: &mut Fingerprint,
    root: &GuiRoot,
    id: GuiNodeId,
    resolver: &dyn GuiResourceResolver,
) {
    let prefix = super::super::tree::component::node_parts_prefix(id);
    for (lane, descriptor) in root.part_lanes(&prefix) {
        hasher.string(lane);
        hasher.u32(descriptor.kind as u32);
        if descriptor.kind == crate::DynamicPropertyKind::Asset {
            let source = root.properties.descriptor_asset(descriptor);
            hash_asset(hasher, source);
            match source.and_then(|source| resolver.resource_generation(source)) {
                Some(generation) => {
                    hasher.u32(1);
                    hasher.u64(generation);
                }
                None => hasher.u32(0),
            }
            continue;
        }

        let start = descriptor.offset as usize;
        match root
            .properties
            .buffer()
            .get(start..start + descriptor.kind.byte_len())
        {
            Some(bytes) => hasher.bytes(bytes),
            None => hasher.u32(u32::MAX),
        }
    }
}

#[cfg(test)]
std::thread_local! {
    static FINGERPRINT_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Input fingerprint passes on this thread since the previous call.
#[cfg(test)]
pub(super) fn take_fingerprint_passes() -> usize {
    FINGERPRINT_PASSES.with(|passes| passes.replace(0))
}

/// Compute the four input fingerprints for one root in storage order.
fn fingerprints(
    root: &GuiRoot,
    surface_size: [f32; 2],
    units: f32,
    resolver: &dyn GuiResourceResolver,
) -> (u64, u64, u64, u64) {
    #[cfg(test)]
    FINGERPRINT_PASSES.with(|passes| passes.set(passes.get() + 1));

    let mut structure = Fingerprint::new();
    let mut layout = Fingerprint::new();
    let mut visual = Fingerprint::new();
    let mut paint = Fingerprint::new();
    structure.u32(root.nodes().next_node_id());
    structure.u32(root.nodes().root_node().map(|id| id.0).unwrap_or(u32::MAX));
    for node in root.nodes().as_slice() {
        hash_node_structure(&mut structure, root, node.id);
        let Some(lanes) = root.node_lanes(node.id) else {
            continue;
        };
        hash_node_layout(&mut layout, &lanes.style, resolver);
        hash_node_visual(&mut visual, &lanes);
        hash_node_paint(&mut paint, root, node.id, &lanes.style, resolver);
    }
    let structure = structure.finish();
    layout.u64(structure);
    layout.f32(surface_size[0]);
    layout.f32(surface_size[1]);
    layout.f32(units);
    let layout = layout.finish();
    visual.u64(structure);
    let visual = visual.finish();
    paint.u64(layout);

    (structure, layout, visual, paint.finish())
}

/// Run one full constraint pass, reusing retained text measurements.
/// Reflows and visual-only refreshes share this pass; only the caller
/// decides which revisions advance. Text cache keys exclude visual lanes,
/// so visual-only refreshes remeasure nothing.
fn evaluate_tree(
    request: &GuiLayoutRequest<'_>,
    resolver: &dyn GuiResourceResolver,
    texts: BTreeMap<GuiNodeId, RetainedText>,
) -> (
    Vec<GuiEvaluatedNode>,
    Vec<GuiLayoutDiagnostic>,
    u64,
    BTreeMap<GuiNodeId, RetainedText>,
) {
    let units = request.units_per_metre;
    let logical = [
        request.surface_size[0] * units,
        request.surface_size[1] * units,
    ];
    let mut evaluator = Evaluator {
        root: request.root,
        resolver,
        units,
        diagnostics: Vec::new(),
        nodes: Vec::new(),
        placements: Vec::new(),
        texts,
        remeasured: 0,
    };

    if let Some(root_id) = request.root.nodes().root_node() {
        let margin = styled(request.root, root_id, units)
            .margin
            .unwrap_or([0.0; 4]);
        evaluator.visit(
            root_id,
            Constraints::loose(logical[0], logical[1]),
            [margin[3], margin[0]],
            [1.0, 1.0],
            0,
        );
    } else if !request.root.nodes().is_empty() {
        evaluator
            .diagnostics
            .push(GuiLayoutDiagnostic::Unsupported {
                node: None,
                detail: "missing-root-node",
            });
    }

    evaluator.resolve_placement(Some([0.0, 0.0, logical[0], logical[1]]));

    (
        evaluator.nodes,
        evaluator.diagnostics,
        evaluator.remeasured,
        evaluator.texts,
    )
}

// ---------------------------------------------------------------------------
// Retained per-root evaluation.
// ---------------------------------------------------------------------------

/// Retained evaluation state for one root entity across frames.
struct RetainedGuiRoot {
    root_incarnation: u64,
    layout_revision: u64,
    paint_revision: u64,
    /// Advances with every paint revision except an in-place translation,
    /// which moves geometry without changing any record's other fields.
    content_revision: u64,
    /// Translatable geometry of each record at the latest full evaluation,
    /// index-aligned with the view's nodes. Translation always starts from
    /// here, so repeated visual edits do not accumulate rounding.
    base: Vec<RetainedGeometry>,
    struct_fp: u64,
    layout_fp: u64,
    visual_fp: u64,
    paint_fp: u64,
    texts: BTreeMap<GuiNodeId, RetainedText>,
    remeasure_count: u64,
    reflow_count: u64,
    view: GuiEvaluatedView,
}

/// Record geometry that a pure visual translation moves.
#[derive(Clone, Copy)]
struct RetainedGeometry {
    origin: [f32; 2],
    content_origin: [f32; 2],
    visual_offset: [f32; 2],
}

fn retained_geometry(nodes: &[GuiEvaluatedNode]) -> Vec<RetainedGeometry> {
    nodes
        .iter()
        .map(|node| RetainedGeometry {
            origin: [node.rect[0], node.rect[1]],
            content_origin: node.content_origin,
            visual_offset: node.visual_offset,
        })
        .collect()
}

/// Apply changed visual translations to retained geometry without a layout
/// pass. A translation moves its node and whole subtree by the same amount:
/// sizes, alignment and measurement never observe it. Returns false, leaving
/// the view for a full evaluation to replace, when any scale changed, an
/// unavailable node's translation changed, or a moved subtree would move a
/// ScrollView viewport (and so descendant clips).
fn translate_in_place(
    root: &GuiRoot,
    units: f32,
    base: &[RetainedGeometry],
    nodes: &mut [GuiEvaluatedNode],
) -> bool {
    if base.len() != nodes.len() {
        return false;
    }

    // (depth, accumulated translation from the latest full evaluation)
    let mut ancestors: Vec<(u32, [f32; 2])> = Vec::new();
    for (record, base) in nodes.iter_mut().zip(base) {
        while ancestors
            .last()
            .is_some_and(|(depth, _)| *depth >= record.depth)
        {
            ancestors.pop();
        }
        let inherited = ancestors.last().map_or([0.0, 0.0], |(_, shift)| *shift);
        let (offset, scale) = visual_scaled(root, record.node, units);
        // Placeholders ignore their own translation, so only unchanged
        // offsets keep them valid.
        if scale != record.visual_scale || (!record.available && offset != base.visual_offset) {
            return false;
        }

        let shift = [
            inherited[0] + (offset[0] - base.visual_offset[0]),
            inherited[1] + (offset[1] - base.visual_offset[1]),
        ];
        if shift != [0.0, 0.0] && record.content_extents.is_some() {
            return false;
        }

        let origin = [base.origin[0] + shift[0], base.origin[1] + shift[1]];
        let content_origin = [
            base.content_origin[0] + shift[0],
            base.content_origin[1] + shift[1],
        ];
        if !origin
            .iter()
            .chain(&content_origin)
            .all(|value| value.is_finite())
        {
            return false;
        }

        record.rect[0] = origin[0];
        record.rect[1] = origin[1];
        record.content_origin = content_origin;
        record.visual_offset = offset;
        ancestors.push((record.depth, shift));
    }

    true
}

/// Retained constraint evaluation for every live GUI root. Unchanged frames
/// do no work, paint-only edits never remeasure text, and structure or
/// layout edits reflow with per-text cache reuse.
#[derive(Default)]
pub struct GuiLayoutCache {
    roots: BTreeMap<EntityId, RetainedGuiRoot>,
}

impl GuiLayoutCache {
    /// Evaluate one root, returning its retained read-only view. The
    /// evaluation tick advances on every call, including no-op refreshes;
    /// revisions advance only on reflow or paint change.
    pub fn evaluate(
        &mut self,
        entity: EntityId,
        request: &GuiLayoutRequest<'_>,
        resolver: &dyn GuiResourceResolver,
    ) -> &GuiEvaluatedView {
        let logical = [
            request.surface_size[0] * request.units_per_metre,
            request.surface_size[1] * request.units_per_metre,
        ];
        let root_valid = request
            .surface_size
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
            && finite_positive(request.units_per_metre).is_some()
            && logical
                .iter()
                .all(|value| value.is_finite() && *value > 0.0);
        if !root_valid {
            let retained = self.roots.entry(entity).or_insert_with(|| {
                RetainedGuiRoot::fresh(entity, request.root_incarnation, request.units_per_metre)
            });
            retained.struct_fp = u64::MAX;
            retained.layout_fp = u64::MAX;
            retained.visual_fp = u64::MAX;
            retained.paint_fp = u64::MAX;
            if retained.view.available {
                retained.paint_revision = retained.paint_revision.saturating_add(1);
                retained.content_revision = retained.content_revision.saturating_add(1);
                retained.view.paint_revision = retained.paint_revision;
            }
            retained.view.available = false;
            retained.view.evaluation_tick = request.evaluation_tick;
            retained.view.root_bounds = [0.0; 4];
            retained.view.diagnostics = vec![GuiLayoutDiagnostic::InvalidConstraints {
                node: None,
                detail: GuiConstraintError::InvalidRoot,
            }];
            return &retained.view;
        }

        let (struct_fp, layout_fp, visual_fp, paint_fp) = fingerprints(
            request.root,
            request.surface_size,
            request.units_per_metre,
            resolver,
        );

        // A new incarnation drops every retained measurement before reuse,
        // so removed node identities never retarget older layouts. Counters
        // and revisions stay cumulative across incarnations; only cached
        // geometry is discarded.
        let incarnation_changed = self
            .roots
            .get(&entity)
            .is_none_or(|retained| retained.root_incarnation != request.root_incarnation);
        if incarnation_changed {
            match self.roots.get_mut(&entity) {
                Some(retained) => {
                    retained.root_incarnation = request.root_incarnation;
                    retained.struct_fp = u64::MAX;
                    retained.layout_fp = u64::MAX;
                    retained.visual_fp = u64::MAX;
                    retained.paint_fp = u64::MAX;
                    retained.texts.clear();
                    retained.view.nodes.clear();
                    retained.view.diagnostics.clear();
                    retained.view.available = true;
                    retained.view.root_incarnation = request.root_incarnation;
                    retained.view.units_per_metre = request.units_per_metre;
                }
                None => {
                    self.roots.insert(
                        entity,
                        RetainedGuiRoot::fresh(
                            entity,
                            request.root_incarnation,
                            request.units_per_metre,
                        ),
                    );
                }
            }
        }

        let retained = self.roots.get_mut(&entity).expect("inserted retained root");
        retained.view.evaluation_tick = request.evaluation_tick;

        if !incarnation_changed
            && retained.struct_fp == struct_fp
            && retained.layout_fp == layout_fp
        {
            // No reflow. Translation-only edits move retained geometry in
            // place; other visual edits rebuild it without remeasuring text or
            // advancing reflow counters; paint-only edits refresh retained
            // colours without touching measurement.
            if retained.visual_fp != visual_fp {
                if translate_in_place(
                    request.root,
                    request.units_per_metre,
                    &retained.base,
                    &mut retained.view.nodes,
                ) {
                    retained.visual_fp = visual_fp;
                    retained.paint_revision += 1;
                    retained.view.paint_revision = retained.paint_revision;
                    if retained.paint_fp != paint_fp {
                        refresh_paint(request.root, &mut retained.view);
                        retained.paint_fp = paint_fp;
                        retained.content_revision += 1;
                    }
                    return &retained.view;
                }

                let (nodes, diagnostics, remeasured, texts) =
                    evaluate_tree(request, resolver, std::mem::take(&mut retained.texts));
                retained.texts = texts;
                retained.remeasure_count += remeasured;
                retained.struct_fp = struct_fp;
                retained.layout_fp = layout_fp;
                retained.visual_fp = visual_fp;
                retained.paint_fp = paint_fp;
                retained.paint_revision += 1;
                retained.content_revision += 1;
                retained.base = retained_geometry(&nodes);
                let view = &mut retained.view;
                view.nodes = nodes;
                view.diagnostics = diagnostics;
                view.paint_revision = retained.paint_revision;
                view.remeasure_count = retained.remeasure_count;
            } else if retained.paint_fp != paint_fp {
                refresh_paint(request.root, &mut retained.view);
                retained.paint_fp = paint_fp;
                retained.paint_revision += 1;
                retained.content_revision += 1;
                retained.view.paint_revision = retained.paint_revision;
            }

            return &retained.view;
        }

        let (nodes, diagnostics, remeasured, texts) =
            evaluate_tree(request, resolver, std::mem::take(&mut retained.texts));

        retained.struct_fp = struct_fp;
        retained.layout_fp = layout_fp;
        retained.visual_fp = visual_fp;
        retained.paint_fp = paint_fp;
        retained.texts = texts;
        retained.remeasure_count += remeasured;
        retained.reflow_count += 1;
        retained.layout_revision += 1;
        retained.paint_revision += 1;
        retained.content_revision += 1;
        retained.base = retained_geometry(&nodes);
        let view = GuiEvaluatedView {
            entity,
            root_incarnation: request.root_incarnation,
            layout_revision: retained.layout_revision,
            paint_revision: retained.paint_revision,
            evaluation_tick: request.evaluation_tick,
            root_bounds: [0.0, 0.0, logical[0], logical[1]],
            units_per_metre: request.units_per_metre,
            nodes,
            diagnostics,
            remeasure_count: retained.remeasure_count,
            reflow_count: retained.reflow_count,
            available: true,
        };
        retained.view = view;
        &retained.view
    }

    /// Read-only retained view for one root entity, if ever evaluated.
    pub fn view(&self, entity: EntityId) -> Option<&GuiEvaluatedView> {
        self.roots.get(&entity).map(|retained| &retained.view)
    }

    /// Current paint revision for one root entity, if ever evaluated.
    /// Render preparation consumes this to rebuild primitives only when
    /// paint actually changed.
    pub fn paint_revision(&self, entity: EntityId) -> Option<u64> {
        self.roots
            .get(&entity)
            .map(|retained| retained.paint_revision)
    }

    /// Current content revision for one root entity: it follows the paint
    /// revision except across in-place translations, so consumers that do
    /// not read translated geometry can skip those frames.
    pub fn content_revision(&self, entity: EntityId) -> Option<u64> {
        self.roots
            .get(&entity)
            .map(|retained| retained.content_revision)
    }

    /// Whether `entity` holds retained output for this root incarnation.
    pub(crate) fn is_current(&self, entity: EntityId, root_incarnation: u64) -> bool {
        self.roots
            .get(&entity)
            .is_some_and(|retained| retained.root_incarnation == root_incarnation)
    }

    /// Advance the evaluation tick of an unchanged root without evaluating.
    pub(crate) fn touch(&mut self, entity: EntityId, evaluation_tick: u64) {
        if let Some(retained) = self.roots.get_mut(&entity) {
            retained.view.evaluation_tick = evaluation_tick;
        }
    }

    /// Drop one root's retained output, invalidating it before reuse.
    pub fn remove_entity(&mut self, entity: EntityId) -> bool {
        self.roots.remove(&entity).is_some()
    }

    /// Drop retained output for every entity outside the live set.
    pub fn retain_entities(&mut self, live: &BTreeSet<EntityId>) {
        self.roots.retain(|entity, _| live.contains(entity));
    }

    /// Entities with retained output, in ascending order.
    pub fn entities(&self) -> Vec<EntityId> {
        self.roots.keys().copied().collect()
    }

    /// Number of roots with retained output.
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// Whether any root output is retained.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

impl RetainedGuiRoot {
    fn fresh(entity: EntityId, root_incarnation: u64, units_per_metre: f32) -> Self {
        Self {
            root_incarnation,
            layout_revision: 0,
            paint_revision: 0,
            content_revision: 0,
            base: Vec::new(),
            struct_fp: u64::MAX,
            layout_fp: u64::MAX,
            visual_fp: u64::MAX,
            paint_fp: u64::MAX,
            texts: BTreeMap::new(),
            remeasure_count: 0,
            reflow_count: 0,
            view: GuiEvaluatedView {
                entity,
                root_incarnation,
                layout_revision: 0,
                paint_revision: 0,
                evaluation_tick: 0,
                root_bounds: [0.0, 0.0, 0.0, 0.0],
                units_per_metre,
                nodes: Vec::new(),
                diagnostics: Vec::new(),
                remeasure_count: 0,
                reflow_count: 0,
                available: true,
            },
        }
    }
}

/// Refresh retained paint lanes after a paint-only edit. Measurement,
/// rectangles and clips are untouched.
fn refresh_paint(root: &GuiRoot, view: &mut GuiEvaluatedView) {
    for node in &mut view.nodes {
        if let Some(style) = root.style(node.node) {
            node.color = style.color;
            node.background = style.background_color;
            node.opacity = style.opacity;
            node.enabled = style.enabled;
            node.visible = node.available && style.opacity > 0.0;
        }
        let Some(live) = root.nodes().node(node.node) else {
            continue;
        };
        match (&mut node.content, &live.content) {
            (
                GuiEvaluatedContent::Checkbox {
                    checked,
                    revision,
                },
                GuiNodeContent::Checkbox {
                    checked: authored,
                },
            ) => match effective_control(root, node.node) {
                Some((GuiControlValue::Bool(value), current)) => {
                    *checked = value;
                    *revision = current;
                }
                _ => {
                    *checked = *authored;
                    *revision = 0;
                }
            },
            (
                GuiEvaluatedContent::Slider {
                    value,
                    min,
                    max,
                    step,
                    revision,
                },
                GuiNodeContent::Slider {
                    value: authored,
                    min: current_min,
                    max: current_max,
                    step: current_step,
                },
            ) => {
                *min = *current_min;
                *max = *current_max;
                *step = *current_step;
                match effective_control(root, node.node) {
                    Some((GuiControlValue::Scalar(current), current_revision)) => {
                        *value = current;
                        *revision = current_revision;
                    }
                    _ => {
                        *value = *authored;
                        *revision = 0;
                    }
                }
            }
            (
                GuiEvaluatedContent::TextInput {
                    text,
                    revision,
                    ..
                },
                GuiNodeContent::TextInput {
                    text: authored,
                    ..
                },
            ) => match effective_control(root, node.node) {
                Some((GuiControlValue::Text(current), current_revision)) => {
                    *text = current;
                    *revision = current_revision;
                }
                _ => {
                    *text = authored.clone();
                    *revision = 0;
                }
            },
            _ => {}
        }
    }
}
