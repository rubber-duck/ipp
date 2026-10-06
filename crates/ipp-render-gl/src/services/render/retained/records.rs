//! Instanced records of retained GUI storage: one record per drawn quad.
//!
//! Shapes and glyphs keep separate storage and programs. A shape record carries
//! the lanes of a box, stroke or arc over one rectangle; a glyph record carries a
//! glyph quad's rectangle, atlas rectangle, tint and clip. Both programs draw six
//! vertices per record from `gl_VertexID` with every attribute advancing per
//! instance, so a record is stored once instead of on six vertices, and painter
//! order within a draw is record order. The [GUI batches](super::shape_batches) own the
//! shape lanes and the [glyph atlas](super::glyph_atlas) the glyph records; the
//! [device layout tables](super::super::device) describe both to the GL devices.

/// Kind of records one retained GUI storage holds, selecting its layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuiRecordKind {
    /// [`GuiShapeRecord`]s, drawn by the canvas program.
    Shape,
    /// [`GuiGlyphRecord`]s, drawn by the glyph program sampling one atlas page.
    Glyph,
}

/// A record type of retained GUI storage.
///
/// Implemented only by [`GuiShapeRecord`] and [`GuiGlyphRecord`], whose
/// `#[repr(C)]` layouts the devices read as tightly packed `f32` lanes.
pub trait GuiRecord: Copy + std::fmt::Debug + PartialEq + private::Sealed + 'static {
    /// The layout this record type uses.
    const KIND: GuiRecordKind;

    /// All-zero record: an empty rectangle, which rasterizes nothing. Storage fills
    /// the room between retained slots with it.
    const EMPTY: Self;
}

mod private {
    pub trait Sealed {}

    impl Sealed for super::GuiShapeRecord {}

    impl Sealed for super::GuiGlyphRecord {}
}

/// One instanced quad of a box, stroke or arc (192 bytes).
///
/// The vertex shader places the quad's corners at `rect` and grows exterior corners
/// by the projected antialias footprint. Every other lane is the primitive's: an
/// outline-only box covers four to eight rectangles with the same lanes.
///
/// Positions, placements and lengths are in the Canvas logical units the Surface
/// transform maps. Strokes and arcs have no corners or border ring, so their geometry
/// occupies the corner radius, accent width and per-corner lanes, and their placement
/// is their own tight bounds rather than the part rectangle.
///
/// An arc's frame has `+y` through the middle of its sweep and `+x` along the sweep,
/// so it spans the angles `[-half_sweep, half_sweep]` from `+y` and starts at
/// `-half_sweep`; a whole ring has a half sweep of exactly π. Its lanes leave
/// `corner_accent[3]` unused.
///
/// A box's checker takes the accent lanes, so a box with a checker paints no accents.
/// Strokes and arcs use those lanes for their own geometry and paint no checker.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiShapeRecord {
    /// Covered rectangle `[x0, y0, x1, y1]`: its top-left and bottom-right corners.
    pub rect: [f32; 4],
    /// Placed origin and size `[pos_x, pos_y, size_x, size_y]`.
    pub placement: [f32; 4],
    /// Box: shape metrics `[corner_rx, corner_ry, border_width, accent_width]`; a
    /// stroke's thickness is its border width. Arc: `[mean_radius, half_sweep,
    /// thickness, dash_duty]`, the half sweep in radians.
    pub shape: [f32; 4],
    /// Box: clamped 45-degree cut per corner `[tl, tr, br, bl]` in the box's own
    /// orientation. Stroke: first segment `[center_x, center_y, half_x, half_y]`
    /// relative to the placement origin, all zero when it paints nothing. Arc:
    /// `[center_x, center_y, middle_x, middle_y]`, the ring centre from the placement
    /// origin and the unit direction through the middle of the sweep.
    pub corner_cut: [f32; 4],
    /// Box: clamped accent span per corner `[tl, tr, br, bl]`, or with a checker
    /// `[cell side, first colour, second colour, alphas]` as the batches pack them.
    /// Stroke: second segment. Arc: `[sin(half_sweep), cos(half_sweep), dashes,
    /// 0.0]`, the dash cells per turn negative when the sweep runs counter-clockwise on
    /// the Surface, and zero for a solid arc.
    pub corner_accent: [f32; 4],
    /// Fill start / solid straight linear RGBA. Colour fields: the tint and opacity
    /// multiplying their opaque colour. Custom paint: the colour the paint receives.
    pub color0: [f32; 4],
    /// Fill end straight linear RGBA (for gradients). Saturation-value field: its hue
    /// in turns from red, then zeros. Custom paint: the packed slot and parameter
    /// block, the opacity and the signed visual scale.
    pub color1: [f32; 4],
    /// Border straight linear RGBA.
    pub border_color: [f32; 4],
    /// Gradient coordinates from the placement origin: `[start_x, start_y, end_x,
    /// end_y]` of a linear gradient's or the hue's axis, `[center_x, center_y, radius,
    /// 0.0]`, or the part rectangle's corners `[x0, y0, x1, y1]` for a
    /// saturation-value field; a custom paint's part origin and own size.
    pub gradient_coords: [f32; 4],
    /// Material parameters: `[paint, glow_inner_radius, glow_radius, glow_falloff]`.
    /// Paint is the fill type plus the checker's and the shape's offsets; see
    /// [`GUI_PAINT_STROKE`](super::box_records::GUI_PAINT_STROKE).
    pub material_params: [f32; 4],
    /// Glow straight linear RGB; alpha is the glow's alpha times its intensity, the
    /// strength its falloff scales.
    pub glow_color: [f32; 4],
    /// Effective clip rectangle `[min_x, min_y, max_x, max_y]`.
    pub clip: [f32; 4],
}

impl GuiRecord for GuiShapeRecord {
    const KIND: GuiRecordKind = GuiRecordKind::Shape;

    const EMPTY: Self = Self {
        rect: [0.0; 4],
        placement: [0.0; 4],
        shape: [0.0; 4],
        corner_cut: [0.0; 4],
        corner_accent: [0.0; 4],
        color0: [0.0; 4],
        color1: [0.0; 4],
        border_color: [0.0; 4],
        gradient_coords: [0.0; 4],
        material_params: [0.0; 4],
        glow_color: [0.0; 4],
        clip: [0.0; 4],
    };
}

// GLES attribute strides and the WebGL bridge read exactly this many bytes per record.
const _: () = assert!(std::mem::size_of::<GuiShapeRecord>() == 192);

/// One instanced atlas glyph quad (64 bytes).
///
/// The glyph program places the corners at `rect` without antialias growth, since
/// atlas entries already hold a blank texel around their coverage, and samples the
/// atlas at the matching corners of `uv`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiGlyphRecord {
    /// Quad corners `[x0, y0, x1, y1]`; a mirrored run has `x0 > x1` or `y0 > y1`.
    pub rect: [f32; 4],
    /// Atlas coordinates `[u0, v0, u1, v1]` at the corners `[x0, y0]` and `[x1, y1]`.
    pub uv: [f32; 4],
    /// Straight linear RGBA tint, opacity included.
    pub color: [f32; 4],
    /// Effective clip rectangle `[min_x, min_y, max_x, max_y]`.
    pub clip: [f32; 4],
}

impl GuiRecord for GuiGlyphRecord {
    const KIND: GuiRecordKind = GuiRecordKind::Glyph;

    const EMPTY: Self = Self {
        rect: [0.0; 4],
        uv: [0.0; 4],
        color: [0.0; 4],
        clip: [0.0; 4],
    };
}

const _: () = assert!(std::mem::size_of::<GuiGlyphRecord>() == 64);

/// Vertices each record draws: two triangles, `[TL, BL, BR]` and `[TL, BR, TR]`.
pub(crate) const GUI_RECORD_VERTICES: usize = 6;
