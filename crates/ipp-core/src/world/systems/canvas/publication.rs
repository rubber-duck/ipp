use crate::services::asset_management::AssetKey;
use crate::{EntityId, OutputRef, WorldAttachmentToken};
use std::sync::Arc;

/// Final Canvas-local `[min_x, min_y, max_x, max_y]` bounds in top-left, Y-down space.
/// Minimum edges are inclusive and maximum edges exclusive; inverted or zero-area
/// rectangles are empty. Clips are already intersected, never transformed again.
pub type CanvasClip = [f32; 4];

/// Exact ordinary component lifetime within the publication's World.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasTarget {
    /// Generational ordinary entity identity.
    pub entity: EntityId,
    /// Compiled component identity.
    pub component: u16,
    /// Exact effective component incarnation.
    pub incarnation: u64,
}

/// Stable part identities independent of generated primitives and painter order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(missing_docs)]
pub enum CanvasPart {
    Content,
    Background,
    Fill,
    Label,
    Icon,
    FocusRing,
    Caret,
    Selection,
    Composition,
    ScrollTrackX,
    ScrollThumbX,
    ScrollTrackY,
    ScrollThumbY,
}

/// Primitive identity additionally scoped by its containing exact Canvas OutputRef.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasPrimitiveId {
    /// Producing entity and component lifetime.
    pub target: CanvasTarget,
    /// Named subsystem part; glyphs remain inside their compact run.
    pub part: CanvasPart,
}

/// Resolved visual inputs shared by raw and GUI-generated primitives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasPrimitiveStyle {
    /// Stable identity, unrelated to the entry's current ordinal.
    pub identity: CanvasPrimitiveId,
    /// Final Canvas-local logical origin.
    pub position: [f32; 2],
    /// Signed visual scale applied to local geometry.
    pub scale: [f32; 2],
    /// Straight linear RGBA tint, the product of every ancestor's tint.
    pub color: [f32; 4],
    /// Accumulated opacity; multiply alpha exactly once.
    pub opacity: f32,
    /// Fully intersected logical clip; empty clips remain empty.
    pub clip: CanvasClip,
}

impl CanvasPrimitiveStyle {
    /// Straight tint of one glyph: its optional colour times the inherited tint.
    pub fn glyph_tint(&self, glyph: &CanvasGlyph) -> [f32; 4] {
        let mut tint = self.color;
        if let Some(color) = glyph.color {
            for (lane, color) in tint.iter_mut().zip(color) {
                *lane *= color;
            }
        }
        tint
    }
}

/// Original font glyph and explicitly retained logical placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasGlyph {
    /// Original glyph identity in the exact font resource.
    pub glyph_id: u32,
    /// Local logical baseline origin before the primitive visual transform.
    pub position: [f32; 2],
    /// Optional straight linear RGBA multiplied by the primitive tint, not its opacity.
    pub color: Option<[f32; 4]>,
}

/// Bounded shape fill in local logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum CanvasShapeFill {
    Solid([f32; 4]),
    LinearGradient {
        start: [f32; 2],
        end: [f32; 2],
        start_color: [f32; 4],
        end_color: [f32; 4],
    },
    RadialGradient {
        center: [f32; 2],
        radius: f32,
        start_color: [f32; 4],
        end_color: [f32; 4],
    },
}

/// Localized paint-only glow; its radius never extends hit bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct CanvasShapeGlow {
    pub color: [f32; 4],
    pub intensity: f32,
    pub radius: f32,
    pub falloff: f32,
}

/// Immutable ready paint with no component or source-name lookups.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum CanvasPrimitive {
    /// Original font contours normalized by the renderer; font_size is logical units per em.
    Glyphs {
        style: CanvasPrimitiveStyle,
        font: AssetKey,
        font_size: f32,
        glyphs: Arc<[CanvasGlyph]>,
    },
    /// Original drawing layers normalized at the shared source/preparation boundary.
    Drawing {
        style: CanvasPrimitiveStyle,
        drawing: AssetKey,
    },
    /// Full UV range with top-left origin; size is logical extent before visual scale.
    Bitmap {
        style: CanvasPrimitiveStyle,
        bitmap: AssetKey,
        size: [f32; 2],
    },
    /// Size precedes visual scale; corner radii and border width are final logical lengths.
    Box {
        style: CanvasPrimitiveStyle,
        size: [f32; 2],
        corner_radius: [f32; 2],
        border_width: f32,
        border_color: [f32; 4],
        fill: CanvasShapeFill,
        glow: Option<CanvasShapeGlow>,
    },
}

impl CanvasPrimitive {
    /// Shared evaluated identity, placement, colour and clipping.
    pub fn style(&self) -> &CanvasPrimitiveStyle {
        match self {
            Self::Glyphs {
                style,
                ..
            }
            | Self::Drawing {
                style,
                ..
            }
            | Self::Bitmap {
                style,
                ..
            }
            | Self::Box {
                style,
                ..
            } => style,
        }
    }

    /// Exact immutable resource retained by the publication, if any.
    pub fn resource(&self) -> Option<AssetKey> {
        match self {
            Self::Glyphs {
                font,
                ..
            } => Some(*font),
            Self::Drawing {
                drawing,
                ..
            } => Some(*drawing),
            Self::Bitmap {
                bitmap,
                ..
            } => Some(*bitmap),
            Self::Box {
                ..
            } => None,
        }
    }
}

/// A nested Surface's one evaluated mapping, shared by composition and paint.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasAttachmentSlot {
    /// Join inside the same WorldPublication by anchor, token and containing Canvas selection.
    pub anchor: EntityId,
    /// Exact Surface component lifetime.
    pub surface_incarnation: u64,
    /// Exact applied attachment write; join only to the same completed edge token.
    pub token: WorldAttachmentToken,
    /// Authored physical rectangle; presentation scaling does not rewrite it.
    pub physical_extent: [f64; 2],
    /// Final logical position of the centred Surface-local origin.
    pub position: [f64; 2],
    /// Logical units per Surface-local metre; Y is negative for a front view.
    pub scale: [f64; 2],
    /// Parent Canvas clip, independent of the child's layout constraints.
    pub clip: CanvasClip,
    /// Effective opacity accumulated through this containing Canvas, including the anchor.
    /// Compose with the inherited World factor per child primitive, never as isolated group alpha.
    /// Cached presentation must retain these same per-primitive semantics without applying it twice.
    /// Zero suppresses paint and hit eligibility, not attachment identity or availability.
    pub opacity: f32,
}

impl CanvasAttachmentSlot {
    /// Physical Surface-local point to evaluated parent Canvas logical position.
    pub fn to_canvas(&self, point: [f64; 2]) -> [f64; 2] {
        [
            self.position[0] + self.scale[0] * point[0],
            self.position[1] + self.scale[1] * point[1],
        ]
    }

    /// Exact inverse used when entering this nested Surface's presentation.
    pub fn from_canvas(&self, point: [f64; 2]) -> Option<[f64; 2]> {
        if !point
            .iter()
            .chain(&self.position)
            .all(|value| value.is_finite())
            || self
                .scale
                .iter()
                .any(|value| !value.is_finite() || *value == 0.0)
        {
            return None;
        }
        Some([
            (point[0] - self.position[0]) / self.scale[0],
            (point[1] - self.position[1]) / self.scale[1],
        ])
    }

    /// Centred, Y-up physical affine derived from the same logical paint mapping.
    pub fn parent_affine(&self, logical_extent: [f32; 2], units_per_metre: f32) -> [f64; 16] {
        let units = f64::from(units_per_metre);
        let center = logical_extent.map(|extent| f64::from(extent) / 2.0);
        [
            self.scale[0] / units,
            0.0,
            0.0,
            0.0,
            0.0,
            -self.scale[1] / units,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            (self.position[0] - center[0]) / units,
            (center[1] - self.position[1]) / units,
            0.0,
            1.0,
        ]
    }
}

/// A retained painter slot; nested attachments keep their position among primitives.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum CanvasPaintEntry {
    Primitive {
        /// Local payload revision, including exact resource identity; excludes visual style.
        geometry_revision: u64,
        /// Complete primitive/style revision, including placement, tint, opacity and clip.
        material_revision: u64,
        primitive: CanvasPrimitive,
    },
    Attachment(CanvasAttachmentSlot),
}

/// Logical scroll axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum CanvasAxis {
    Horizontal,
    Vertical,
}

/// Evaluated hit operation; GUI control interpretation stays in its owning System.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum CanvasHitKind {
    Entity,
    ScrollTrack {
        axis: CanvasAxis,
    },
    ScrollThumb {
        axis: CanvasAxis,
    },
    Attachment {
        anchor: EntityId,
        token: WorldAttachmentToken,
    },
}

/// Immutable routing geometry supplied after local evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasHit {
    /// Exact ordinary entity/component incarnation.
    pub target: CanvasTarget,
    /// Local action or nested output classification.
    pub kind: CanvasHitKind,
    /// Painter ordinal, traversed in reverse for pointer targeting.
    pub paint_order: u32,
    /// Final logical rectangle in minimum/maximum form.
    pub bounds: CanvasClip,
    /// Fully intersected ancestor clip.
    pub clip: CanvasClip,
    /// Logical entity origin for inverse pointer mapping.
    pub position: [f32; 2],
    /// Accumulated signed visual scale.
    pub scale: [f32; 2],
    /// Evaluated visibility, availability and enabled eligibility.
    pub eligible: bool,
    /// Committed logical ancestry, root-first and including this target.
    pub ancestry: Arc<[EntityId]>,
}

impl CanvasHit {
    /// Minimum-inclusive, maximum-exclusive geometry shared with Canvas clipping.
    pub fn contains(&self, point: [f32; 2]) -> bool {
        self.eligible
            && [self.bounds, self.clip].iter().all(|bounds| {
                point[0] >= bounds[0]
                    && point[1] >= bounds[1]
                    && point[0] < bounds[2]
                    && point[1] < bounds[3]
            })
    }
}

/// Local interaction requests; the composed router additionally checks path/gate eligibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct CanvasInteractionPriority {
    pub focused: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub captured: bool,
}

impl CanvasInteractionPriority {
    /// Whether current eligible local state requires direct presentation.
    pub fn requires_direct(self) -> bool {
        self.focused || self.hovered || self.pressed || self.captured
    }
}

/// Concrete retained Canvas output. The Host publication owns source tick and resource leases.
#[derive(Clone, Debug)]
pub struct CanvasPublication {
    /// The World canvas output this publication belongs to.
    pub selection: OutputRef,
    /// Evaluated root extent in logical units.
    pub logical_extent: [f32; 2],
    /// Logical units per Surface metre; DPR is a separate view input.
    pub units_per_metre: f32,
    /// Conservative layout/geometry revision within this output incarnation, not a frame tick.
    pub layout_revision: u64,
    /// Paint/order/slot-token revision within this output incarnation.
    pub paint_revision: u64,
    /// Exact resource identity/readiness revision within this output incarnation.
    pub resource_revision: u64,
    /// Hit/traversal/attachment-token revision within this output incarnation.
    pub input_revision: u64,
    /// Immutable ordered entries sharing unchanged primitive storage.
    pub entries: Arc<[Arc<CanvasPaintEntry>]>,
    /// Evaluated hit records; raw content creates no GUI control behavior.
    pub hits: Arc<[CanvasHit]>,
    /// Current local interaction priority, filtered by the composed router before use.
    pub interaction: CanvasInteractionPriority,
    pub(crate) resources: Arc<[AssetKey]>,
}

impl PartialEq for CanvasPublication {
    fn eq(&self, other: &Self) -> bool {
        self.selection == other.selection
            && self.logical_extent == other.logical_extent
            && self.units_per_metre == other.units_per_metre
            && self.layout_revision == other.layout_revision
            && self.paint_revision == other.paint_revision
            && self.resource_revision == other.resource_revision
            && self.input_revision == other.input_revision
            && self.interaction == other.interaction
    }
}

impl CanvasPublication {
    /// Exact resources passed to the generic publication lease builder.
    pub fn resources(&self) -> impl Iterator<Item = AssetKey> + '_ {
        self.resources.iter().copied()
    }
}
