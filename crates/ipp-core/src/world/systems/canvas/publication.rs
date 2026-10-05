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
    /// Compact runtime-generated Plot primitive identity.
    Plot(u32),
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
    Track,
    Ticks,
    /// The Icon of a control's focus part after the first, such as a range's
    /// upper thumb or a colour control's rail thumb: each focus part paints
    /// its own.
    PartIcon(u8),
    /// A numeric text input's decrement part.
    Decrement,
    /// A numeric text input's increment part.
    Increment,
    /// The mark in a numeric text input's decrement part.
    DecrementMark,
    /// The mark in a numeric text input's increment part.
    IncrementMark,
    /// The Track of a control's focus part after the first, such as a colour
    /// control's hue or alpha rail.
    PartTrack(u8),
    /// A colour control's marker on its saturation-value field.
    Marker,
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
    /// Opaque plane ID in the completed publication. Painter order follows
    /// destination logical priority; Surface separation uses the plane's offset.
    pub layer: u32,
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
///
/// Gradients interpolate their straight linear stops premultiplied in linear light.
/// The colour fields instead evaluate the HSV colour model on sRGB-encoded values and
/// decode each point to linear, so the displayed pixel is the sRGB colour the model
/// gives there; their colours are opaque, and the style's tint and opacity multiply
/// them as they do every fill.
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
    /// The hue circle at full saturation and value along the axis from `start` to
    /// `end`, measured as a linear gradient's: red at `start`, through yellow, green,
    /// cyan, blue and magenta, to red again a whole turn later at `end`, and constant
    /// beyond either point.
    Hue {
        start: [f32; 2],
        end: [f32; 2],
    },
    /// The saturation-value field of one hue over the box rectangle in its own
    /// orientation: saturation rises from zero at the left edge to one at the right
    /// and value from zero at the bottom edge to one at the top, so the top-left
    /// corner is white, the top-right the pure hue and the bottom edge black. The
    /// sRGB colour at a point is `value * mix(white, hue colour, saturation)`.
    SaturationValue {
        /// Turns from red, taken modulo one turn.
        hue: f32,
    },
    /// The custom paint of the `CanvasPaint` component lifetime `paint`, which the
    /// publication's [`CanvasPublication::paints`] resolves, called with `color`:
    /// the straight linear RGBA the box would otherwise fill with solidly, and
    /// fills with while the renderer cannot use the paint.
    Paint {
        color: [f32; 4],
        paint: CanvasTarget,
    },
}

impl CanvasShapeFill {
    /// The colour a custom paint receives in place of this fill: a solid fill's
    /// colour, a gradient's start colour, and white for the colour fields, whose
    /// colour the tint gives.
    pub fn paint_color(&self) -> [f32; 4] {
        match *self {
            Self::Solid(color)
            | Self::Paint {
                color,
                ..
            } => color,
            Self::LinearGradient {
                start_color,
                ..
            }
            | Self::RadialGradient {
                start_color,
                ..
            } => start_color,
            Self::Hue {
                ..
            }
            | Self::SaturationValue {
                ..
            } => [1.0; 4],
        }
    }
}

/// One `CanvasPaint` component as a publication carries it beside the boxes it
/// fills.
///
/// Property writes and animation replace this record without changing any paint
/// entry, so the renderer updates the paint's inputs without regenerating geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPaintInstance {
    /// The component lifetime painted fills name.
    pub target: CanvasTarget,
    /// Authored shader-definition source, for diagnostics.
    pub source: Arc<str>,
    /// Exact loaded shader definition; none while it is unset, pending or failed.
    pub shader: Option<AssetKey>,
    /// Numeric property values in name order; asset-valued properties are left out.
    pub properties: Arc<[(Arc<str>, crate::DynamicValue)]>,
}

/// Checkerboard beneath a box's fill, inside its border ring.
///
/// Square cells alternate between the two straight linear RGBA colours from the box's
/// own top-left corner, whose cell takes the first. A translucent fill composites
/// over it in linear light, its alpha being linear coverage: half-transparent white
/// over black shows sRGB 188, not the 128 of blending encoded values. Cells shrinking
/// toward a pixel, as on a minified or oblique Surface, fade to the colours' mean
/// instead of aliasing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasShapeChecker {
    /// Cell side in logical units; a checker needs a positive size.
    pub size: f32,
    /// Colours of the corner cell and of its neighbours.
    pub colors: [[f32; 4]; 2],
}

/// Localized paint-only edge glow; its radii never extend hit bounds.
///
/// Light falls off from the shape's outer contour: outward over `radius`, painting
/// only where the shape does not, and inward over `inner_radius`, over the fill and
/// beneath the border. Both sides share the colour, intensity and falloff exponent.
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct CanvasShapeGlow {
    pub color: [f32; 4],
    pub intensity: f32,
    pub radius: f32,
    pub inner_radius: f32,
    pub falloff: f32,
}

/// Contour of a [`CanvasPrimitive::Box`] within its rectangle.
///
/// Corner arrays run `[top-left, top-right, bottom-right, bottom-left]` in the box's
/// own orientation, so a mirrored box mirrors them. Lengths are final logical lengths
/// like the corner radius, and the renderer clamps each corner's length so the
/// lengths of two corners never overlap along the side they share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasBoxShape {
    /// The rectangle and its border ring.
    Rect {
        /// 45-degree cut length per corner; an uncut corner keeps the corner radius.
        corner_cut: [f32; 4],
        /// Accent span per corner along both edges leaving it, measured from the
        /// rectangle's corner; zero leaves that corner unaccented.
        corner_accent: [f32; 4],
        /// Border width inside accent spans, replacing the border width there.
        corner_accent_width: f32,
        /// Optional checkerboard beneath the fill. A box with a checker paints no
        /// corner accents.
        checker: Option<CanvasShapeChecker>,
    },
    /// One or two straight butt-capped segments painted by the fill, `border_width`
    /// thick, without a border ring. Each segment is `[x0, y0, x1, y1]` normalised to
    /// the box rectangle; a zero-length segment paints nothing. Segments sharing an
    /// end point leave a notch on the outer side of their joint unless one extends
    /// past it; overlapping segments paint their union once.
    Stroke {
        /// First and second segment.
        segments: [[f32; 4]; 2],
    },
    /// A ring arc painted by the fill, `border_width` thick, without a border ring,
    /// with butt ends along its radius. The ring's outer edge is the circle inscribed
    /// in the shorter side of the placed rectangle and centred in it, so a whole turn
    /// covers what a box with half-size radii and the same border width outlines; a
    /// width of the radius or more fills a pie.
    ///
    /// Angles are turns, clockwise from twelve o'clock in the box's own Y-down
    /// orientation, so a mirrored box mirrors its arc.
    Arc {
        /// First end; any finite value, taken modulo one turn.
        start: f32,
        /// Extent from `start`, clockwise when positive and counter-clockwise when
        /// negative. A whole turn or more paints the whole ring and zero paints
        /// nothing.
        sweep: f32,
        /// Dash cells per turn, laid from `start` along the sweep; zero paints a
        /// solid arc. A whole number divides a full ring evenly.
        dashes: f32,
        /// Fraction of each cell its dash covers, centred in the cell, so cells
        /// meet in the middle of the gaps. One paints a solid arc and zero nothing.
        dash_duty: f32,
    },
}

impl CanvasBoxShape {
    /// The plain rectangle: no cuts, no accents and no checker.
    pub const RECT: Self = Self::Rect {
        corner_cut: [0.0; 4],
        corner_accent: [0.0; 4],
        corner_accent_width: 0.0,
        checker: None,
    };
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
    /// Runtime-generated top-left/Y-down closed contours, retained with the publication.
    Path {
        style: CanvasPrimitiveStyle,
        path: Arc<crate::systems::plot::PlotPath>,
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
    /// Size precedes visual scale; corner radii, border width and the shape's corner
    /// lengths are final logical lengths.
    Box {
        style: CanvasPrimitiveStyle,
        size: [f32; 2],
        corner_radius: [f32; 2],
        border_width: f32,
        border_color: [f32; 4],
        fill: CanvasShapeFill,
        glow: Option<CanvasShapeGlow>,
        shape: CanvasBoxShape,
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
            | Self::Path {
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

    /// Update placement while preserving immutable local geometry storage.
    pub(crate) fn style_mut(&mut self) -> &mut CanvasPrimitiveStyle {
        match self {
            Self::Glyphs {
                style,
                ..
            }
            | Self::Drawing {
                style,
                ..
            }
            | Self::Path {
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
            }
            | Self::Path {
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
    /// Registered provider of the slot extent.
    pub surface_component: u16,
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
    /// Opaque plane ID of the anchor; the nested canvas presents on this
    /// layer's plane, and its own layers only order its content there.
    pub layer: u32,
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
///
/// Publications share every entry behind its own `Arc`, so the primitive variant's
/// larger size costs nothing per copy.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs, clippy::large_enum_variant)]
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

impl CanvasPaintEntry {
    /// Opaque current-publication plane ID of the producing entity.
    pub fn layer(&self) -> u32 {
        match self {
            Self::Primitive {
                primitive,
                ..
            } => primitive.style().layer,
            Self::Attachment(slot) => slot.layer,
        }
    }
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
    /// The box of an open light overlay, or the whole canvas under an open
    /// modal one, below the overlay's own content: it takes the pointer from
    /// lower layers and selects no control.
    Overlay,
}

/// One physical plane in a completed Canvas publication. IDs are local to that
/// publication; only `offset` determines Surface separation along its normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasLayerPlane {
    /// Opaque plane identity, shared only by exact coincidence within one scope.
    pub id: u32,
    /// Current resolved normal coordinate, multiplied by Surface layer spacing.
    pub offset: f64,
}

/// Immutable routing geometry supplied after local evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasHit {
    /// Exact ordinary entity/component incarnation.
    pub target: CanvasTarget,
    /// Local action or nested output classification.
    pub kind: CanvasHitKind,
    /// Ordinal in the canvas's tree-order paint walk, which keyboard traversal
    /// follows. Publication hits are listed by destination priority, then this ordinal; pointer
    /// targeting walks that list in reverse. Without layers it is the painter
    /// ordinal of the hit's paint.
    pub paint_order: u32,
    /// Destination logical priority, independent of current physical placement.
    pub priority: u32,
    /// Opaque current-publication plane ID, as on its paint.
    pub layer: u32,
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
    /// Immutable entries in destination logical priority then tree order,
    /// sharing unchanged primitive storage.
    pub entries: Arc<[Arc<CanvasPaintEntry>]>,
    /// Evaluated hit records by destination priority, then tree order; raw content creates no
    /// GUI control behavior.
    pub hits: Arc<[CanvasHit]>,
    /// Chart-local analytic marks placed through the same Canvas walk and clips.
    pub plot_hits: Arc<[crate::systems::plot::PlotCanvasHit]>,
    /// Current planes, ordered by discrete scope then physical offset; never
    /// empty. Entries and hits retain independent logical painter order.
    /// IDs and offsets are recomputed from the current authored endpoints.
    pub layers: Arc<[CanvasLayerPlane]>,
    /// Current local interaction priority, filtered by the composed router before use.
    pub interaction: CanvasInteractionPriority,
    /// The custom paints the entries' paint fills name, by target.
    pub paints: Arc<[CanvasPaintInstance]>,
    /// Custom paint revision within this output incarnation: it changes with
    /// `paints`, including property values, which leave `paint_revision` alone.
    pub paints_revision: u64,
    /// The entries this paint revision replaced in place, when every other
    /// entry is the one the previous paint revision published at its index,
    /// so a consumer that presented that revision need only look at these.
    pub paint_changes: Option<CanvasPaintChanges>,
    pub(crate) resources: Arc<[AssetKey]>,
}

/// Entries a paint revision replaced in place.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPaintChanges {
    /// Paint revision of the publication whose entries these replace; every
    /// other entry is that publication's shared entry at the same index.
    pub base: u64,
    /// Indices of the replaced entries, ascending.
    pub entries: Arc<[u32]>,
}

impl CanvasPaintChanges {
    /// Whether the entry at `index` was replaced.
    pub fn replaced(&self, index: usize) -> bool {
        u32::try_from(index).map_or(true, |index| self.entries.binary_search(&index).is_ok())
    }
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
            && self.paints_revision == other.paints_revision
            && self.interaction == other.interaction
    }
}

impl CanvasPublication {
    /// Exact resources passed to the generic publication lease builder.
    pub fn resources(&self) -> impl Iterator<Item = AssetKey> + '_ {
        self.resources.iter().copied()
    }

    /// The custom paint a [`CanvasShapeFill::Paint`] names.
    pub fn paint(&self, target: CanvasTarget) -> Option<&CanvasPaintInstance> {
        self.paints
            .binary_search_by_key(&target, |paint| paint.target)
            .ok()
            .map(|index| &self.paints[index])
    }
}

impl CanvasPublication {
    /// Physical normal coordinate of one current-publication opaque plane ID.
    pub fn layer_offset(&self, id: u32) -> Option<f64> {
        self.layers
            .get(id as usize)
            .filter(|plane| plane.id == id)
            .map(|plane| plane.offset)
    }
}
