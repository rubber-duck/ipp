use super::{
    PositionedGlyph, SurfaceItemContent, SurfaceItemId, TextFont, TextLayout, TextLinePolicy,
    TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use crate::services::asset_management::{AssetKey, AssetManagementService, AssetSource};
use crate::world::WorldSimulationState;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(in crate::world) struct SurfaceLayoutCache {
    labels: BTreeMap<(crate::EntityId, SurfaceItemId), CachedLabel>,
    #[cfg(test)]
    pub(super) rebuilds: u64,
    #[cfg(test)]
    pub(in crate::world) model_preparations: u64,
    #[cfg(test)]
    pub(in crate::world) primitive_preparations: u64,
}

struct CachedLabel {
    initialized: bool,
    text: String,
    font: AssetKey,
    font_size_bits: u32,
    layout: TextLayout,
}

impl SurfaceLayoutCache {
    /// Shared headless measurement for one label. The retained key covers
    /// text, font incarnation, line policy, width and font size; camera and
    /// paint-only edits reuse the layout. Labels measure multiline and
    /// unbounded, matching long-standing label behaviour.
    fn label_prepared<'a>(
        &'a mut self,
        key: (crate::EntityId, SurfaceItemId),
        text: &str,
        font_key: AssetKey,
        font_size: f32,
        font: &crate::services::asset_management::font::FontAsset,
    ) -> Option<(&'a TextLayout, bool)> {
        let request = TextMeasureRequest::new(
            text,
            TextFont::Ready {
                key: font_key,
                font,
            },
            font_size,
            TextLinePolicy::Multiline,
            TextMaxWidth::Unbounded,
        )
        .ok()?;
        let cache_key = request.cache_key();
        let cached = self.labels.entry(key).or_insert_with(|| CachedLabel {
            text: String::new(),
            initialized: false,
            font: font_key,
            font_size_bits: 0,
            layout: TextLayout {
                font_size,
                glyphs: Vec::new(),
                lines: Vec::new(),
                grapheme_boundaries: Vec::new(),
                size: [0.0, 0.0],
            },
        });
        let rebuilt = !cached.initialized
            || cached.text != cache_key.text
            || cached.font != cache_key.font
            || cached.font_size_bits != cache_key.font_size_bits;
        if rebuilt {
            let TextOutcome::Measured(layout) = measure_text(&request) else {
                return None;
            };
            cached.initialized = true;
            cached.text.clear();
            cached.text.push_str(text);
            cached.font = font_key;
            cached.font_size_bits = cache_key.font_size_bits;
            cached.layout = layout;
            #[cfg(test)]
            {
                self.rebuilds += 1;
            }
        }
        Some((&cached.layout, rebuilt))
    }
}

/// Resolved immutable resource identity retained by a completed render submission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRenderResource {
    /// Runtime key whose generation is checked by the renderer's provider.
    pub key: AssetKey,
    /// Authored source retained for diagnostics and context-recovery lookup.
    pub source: AssetSource,
}

/// Identity domain of one prepared Surface primitive.
///
/// Raw authored items and GUI-generated output share the preparation boundary
/// but never share identities: authored items use the `Authored` variant with
/// their component-local [`SurfaceItemId`], while GUI layout output uses the
/// `Gui` variant with its root-local node identity. GUI parts never fabricate
/// authored item identities, and retained caches key raw labels by
/// `(entity, SurfaceItemId)` in a domain disjoint from GUI paint keyed by
/// root-local node identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfacePrimitiveIdentity {
    /// Raw authored Surface item, stable within its component collection.
    Authored(SurfaceItemId),
    /// GUI-generated output for one live node. The node lifetime fences reuse:
    /// removing and recreating a node never retargets an older primitive.
    #[cfg(feature = "gui")]
    Gui(GuiPrimitiveId),
}

/// Stable named part of one GUI-generated Surface primitive.
///
/// These names are the authored skin identities. They remain independent of
/// painter order and generated primitive indices.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiPrimitivePart {
    /// Resizable node background.
    Background,
    /// Text or control label.
    Label,
    /// Drawing or bitmap icon/content.
    Icon,
    /// Focus indicator painted independently of the background.
    FocusRing,
}

#[cfg(feature = "gui")]
impl GuiPrimitivePart {
    /// Stable authored skin-part name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Label => "label",
            Self::Icon => "icon",
            Self::FocusRing => "focusRing",
        }
    }
}

/// Root-local identity of one GUI-generated Surface primitive.
///
/// This identifies paint, not authoring: it never appears in client edits to
/// authored Surface items and never collides with [`SurfaceItemId`].
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuiPrimitiveId {
    /// Root component incarnation fencing whole-root replacement.
    pub root_incarnation: u64,
    /// Stable root-local node identity, never reused within its root incarnation.
    pub node: crate::systems::gui::GuiNodeId,
    /// Node lifetime fencing primitive reuse after removal and recreation.
    pub lifetime: u32,
    /// Stable named paint part, independent of generated primitive order.
    pub part: GuiPrimitivePart,
}

/// Effective local-Surface clip rectangle `[min_x, min_y, max_x, max_y]` in
/// top-left, +Y-down content metres. `None` selects the whole root content
/// rectangle. The renderer intersects this rectangle with the root clip on
/// every glyph, drawing, bitmap and box path; an empty intersection suppresses
/// the primitive without disturbing painter order or scene depth behavior.
pub type SurfaceClipRect = [f32; 4];

/// Evaluated style shared by every primitive kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePrimitiveStyle {
    /// Stable primitive identity, independent of painter order.
    pub identity: SurfacePrimitiveIdentity,
    /// Local-metre translation from the Surface origin.
    pub position: [f32; 2],
    /// Local scale applied after the item-specific metric conversion.
    pub scale: [f32; 2],
    /// Straight linear RGBA tint. Alpha is multiplied by `opacity` exactly once.
    pub color: [f32; 4],
    /// Independent visibility multiplier.
    pub opacity: f32,
    /// Optional effective local-Surface clip rectangle; see [`SurfaceClipRect`].
    pub clip: Option<SurfaceClipRect>,
}

/// One resolved glyph in local Surface coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceGlyph {
    /// Original immutable font glyph identity.
    pub glyph_id: u32,
    /// Glyph origin relative to the item's position, in metres.
    pub position: [f32; 2],
    /// Optional straight linear RGBA override.
    pub color: Option<[f32; 4]>,
}

/// One painter-ordered Surface primitive with ready immutable resource identity.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum SurfaceRenderPrimitive {
    /// Basic runtime layout or externally positioned glyph run.
    Glyphs {
        style: SurfacePrimitiveStyle,
        font: SurfaceRenderResource,
        /// Metres per em used to scale the font-unit contours.
        font_size: f32,
        glyphs: Vec<SurfaceGlyph>,
    },
    /// Quadratic drawing; fill rules and painted path order belong to the asset.
    Drawing {
        style: SurfacePrimitiveStyle,
        drawing: SurfaceRenderResource,
    },
    /// Full-range `[0,0]` through `[1,1]` RGBA bitmap.
    Bitmap {
        style: SurfacePrimitiveStyle,
        bitmap: SurfaceRenderResource,
        /// Display size before the common item scale, in metres.
        size: [f32; 2],
    },
    /// Parameterized filled or bordered rectangle or rounded box.
    ///
    /// GUI-only: supplied by retained layout output, never by authored Surface
    /// items. Corner radii and border width are explicit local Surface-metre
    /// dimensions applied after the common item scale, so resizing `size`
    /// keeps corners and borders meaningful instead of stretching pre-baked
    /// curved corners. Fill color and opacity come from `style`; the border
    /// color carries its own straight linear RGBA with alpha multiplied by
    /// `style.opacity` exactly once.
    #[cfg(feature = "gui")]
    Box {
        style: SurfacePrimitiveStyle,
        /// Display size before the common item scale, in metres.
        size: [f32; 2],
        /// Explicit local corner radii `[rx, ry]` in final Surface metres.
        corner_radius: [f32; 2],
        /// Explicit local border width in final Surface metres; zero fills only.
        border_width: f32,
        /// Straight linear RGBA border color.
        border_color: [f32; 4],
    },
}

/// One evaluated Surface submission, independent of mesh submissions.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceRenderItem {
    /// Live entity identity, also providing deterministic inter-Surface ties.
    pub entity: crate::EntityId,
    /// Complete column-major local-to-World transform.
    pub model: [f32; 16],
    /// World-space anchor for transparency sorting (entity center).
    pub anchor: [f32; 3],
    /// Centred local clipping rectangle width and height, in metres.
    pub clip_size: [f32; 2],
    /// Explicit painter order; batching may combine only compatible contiguous entries.
    pub primitives: Vec<SurfaceRenderPrimitive>,
}

/// Root content rectangle `[0, 0, width, height]` for a Surface of the given size.
///
/// Returns `None` when the size is non-finite or non-positive; such Surfaces
/// fail component validation before preparation.
pub fn surface_content_clip(width: f32, height: f32) -> Option<SurfaceClipRect> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    Some([0.0, 0.0, width, height])
}

/// Whether a clip rectangle covers no area: inverted or zero-area bounds.
///
/// Non-finite bounds are treated as empty; callers suppress the primitive
/// rather than drawing under an undefined clip.
pub fn surface_clip_is_empty(clip: SurfaceClipRect) -> bool {
    if !clip.iter().all(|value| value.is_finite()) {
        return true;
    }

    clip[0] >= clip[2] || clip[1] >= clip[3]
}

/// Intersect two clip rectangles in shared Surface content coordinates.
///
/// Returns `None` when either rectangle is empty or the overlap covers no
/// area. Nested GUI clips compose by intersecting each ancestor clip once, in
/// painter-tree order, before preparing the primitive.
pub fn intersect_surface_clips(
    outer: SurfaceClipRect,
    inner: SurfaceClipRect,
) -> Option<SurfaceClipRect> {
    if surface_clip_is_empty(outer) || surface_clip_is_empty(inner) {
        return None;
    }

    let intersected = [
        outer[0].max(inner[0]),
        outer[1].max(inner[1]),
        outer[2].min(inner[2]),
        outer[3].min(inner[3]),
    ];
    if surface_clip_is_empty(intersected) {
        return None;
    }

    Some(intersected)
}

/// Resolve the drawable clip for one primitive on a Surface of the given size.
///
/// `None` style clips select the whole root content rectangle. Returns `None`
/// when the intersection is empty, in which case the caller suppresses the
/// primitive: preparation drops it from retained output and the renderer
/// issues no draw for it.
pub fn primitive_effective_clip(
    style: &SurfacePrimitiveStyle,
    surface_size: [f32; 2],
) -> Option<SurfaceClipRect> {
    let root = surface_content_clip(surface_size[0], surface_size[1])?;
    let Some(clip) = style.clip else {
        return Some(root);
    };

    intersect_surface_clips(root, clip)
}

/// Whether one prepared primitive survives clipping on a Surface of the given size.
///
/// This is the shared suppression predicate used by both preparation, which
/// drops empty primitives from retained painter-ordered output, and the
/// renderer, which skips draws whose effective clip is empty. Box parameters
/// are additionally validated: non-finite or non-positive size, negative
/// corner radii or border width, and out-of-range border colors suppress the
/// box, matching how missing resources suppress only their affected item.
pub fn surface_primitive_visible(
    primitive: &SurfaceRenderPrimitive,
    surface_size: [f32; 2],
) -> bool {
    let style = match primitive {
        SurfaceRenderPrimitive::Glyphs {
            style,
            ..
        }
        | SurfaceRenderPrimitive::Drawing {
            style,
            ..
        }
        | SurfaceRenderPrimitive::Bitmap {
            style,
            ..
        } => style,
        #[cfg(feature = "gui")]
        SurfaceRenderPrimitive::Box {
            style,
            size,
            corner_radius,
            border_width,
            border_color,
        } => {
            if !size.iter().all(|value| value.is_finite() && *value > 0.0)
                || !corner_radius
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0)
                || !border_width.is_finite()
                || *border_width < 0.0
                || !border_color
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            {
                return false;
            }

            style
        }
    };

    primitive_effective_clip(style, surface_size).is_some()
}

/// Map a GUI logical point to Surface content coordinates.
///
/// For Surface width `W` and height `H` in metres and finite positive
/// `units_per_metre` `U`, GUI logical `(x, y)` maps to content
/// `(x / U, y / U)`; the logical root extent is `(W * U, H * U)`. Both spaces
/// share the top-left origin with +X right and +Y down, so this mapping is
/// unit scaling only: GUI never flips Y or translates the origin. Returns
/// `None` for non-finite points or a non-finite, non-positive scale.
#[cfg(feature = "gui")]
pub fn gui_logical_to_surface_content(logical: [f32; 2], units_per_metre: f32) -> Option<[f32; 2]> {
    if !logical.iter().all(|value| value.is_finite())
        || !units_per_metre.is_finite()
        || units_per_metre <= 0.0
    {
        return None;
    }

    Some([logical[0] / units_per_metre, logical[1] / units_per_metre])
}

/// Inverse of [`gui_logical_to_surface_content`]: map a Surface content point
/// back to GUI logical coordinates for hit testing. Returns `None` under the
/// same invalid inputs.
#[cfg(feature = "gui")]
pub fn surface_content_to_gui_logical(content: [f32; 2], units_per_metre: f32) -> Option<[f32; 2]> {
    if !content.iter().all(|value| value.is_finite())
        || !units_per_metre.is_finite()
        || units_per_metre <= 0.0
    {
        return None;
    }

    Some([content[0] * units_per_metre, content[1] * units_per_metre])
}

pub(in crate::world) fn prepare_surface_render_items(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    cache: &mut SurfaceLayoutCache,
    output: &mut Vec<SurfaceRenderItem>,
    rebuild_primitives: bool,
) {
    #[cfg(test)]
    {
        cache.model_preparations += 1;
    }
    #[cfg(test)]
    let mut prepared_primitives = rebuild_primitives;
    let mut retained_labels = BTreeSet::new();
    let mut output_index = 0;
    for &entity in world.state.entities.keys() {
        let index = entity.index() as usize;
        let Some(surface) = world.components.surface(index) else {
            continue;
        };
        let Ok(model) = crate::systems::hierarchy::evaluated_affine(world, entity)
            .and_then(|affine| affine.render_matrix())
        else {
            continue;
        };
        while output_index < output.len() && output[output_index].entity < entity {
            output.remove(output_index);
        }
        let inserted = output
            .get(output_index)
            .is_none_or(|prepared| prepared.entity != entity);
        let surface_model = content_to_world_matrix(&model, surface.width, surface.height);
        let anchor = [model[12], model[13], model[14]];
        if inserted {
            #[cfg(test)]
            {
                prepared_primitives = true;
            }
            output.insert(
                output_index,
                SurfaceRenderItem {
                    entity,
                    model: surface_model,
                    anchor,
                    clip_size: [surface.width, surface.height],
                    primitives: Vec::with_capacity(surface.items().len()),
                },
            );
        }
        let prepared = &mut output[output_index];
        prepared.model = surface_model;
        prepared.anchor = anchor;
        prepared.clip_size = [surface.width, surface.height];
        output_index += 1;
        if !rebuild_primitives && !inserted {
            continue;
        }
        let mut written = 0;
        for item in surface.items() {
            let Some(style) = surface.style(item.id) else {
                continue;
            };
            let Some(source) = style.asset else {
                continue;
            };
            let Some(resource) = resolve_resource(world, assets, source) else {
                continue;
            };
            let common = SurfacePrimitiveStyle {
                identity: SurfacePrimitiveIdentity::Authored(item.id),
                position: style.position,
                scale: style.scale,
                color: style.color,
                opacity: style.opacity,
                // Authored items select the whole root content rectangle.
                // GUI layout output supplies its own intersected clip.
                clip: None,
            };
            let previous = prepared.primitives[written..]
                .iter()
                .position(|primitive| {
                    primitive.identity() == SurfacePrimitiveIdentity::Authored(item.id)
                })
                .map(|relative| written + relative);
            if let Some(previous) = previous {
                prepared.primitives.swap(written, previous);
            }
            let previous = previous.map(|_| written);
            let primitive = match &item.content {
                SurfaceItemContent::Label(text) => {
                    let Some(font) = assets
                        .get(resource.key)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| {
                            asset
                                .decoded()
                                .downcast_ref::<crate::services::asset_management::font::FontAsset>(
                                )
                        })
                    else {
                        continue;
                    };
                    let cache_key = (entity, item.id);
                    retained_labels.insert(cache_key);
                    let Some((layout, rebuilt)) =
                        cache.label_prepared(cache_key, text, resource.key, style.font_size, font)
                    else {
                        continue;
                    };
                    let reuse_layout = !rebuilt
                        && previous.is_some_and(|index| {
                            matches!(
                                &prepared.primitives[index],
                                SurfaceRenderPrimitive::Glyphs { font, font_size, .. }
                                    if font.key == resource.key && *font_size == style.font_size
                            )
                        });
                    let mut glyphs = take_glyphs(&mut prepared.primitives, previous);
                    if !reuse_layout {
                        glyphs.clear();
                        glyphs.extend(layout.glyphs.iter().map(|glyph| SurfaceGlyph {
                            glyph_id: glyph.glyph_id,
                            position: [
                                glyph.position[0] * style.font_size,
                                glyph.position[1] * style.font_size,
                            ],
                            color: None,
                        }));
                    }
                    SurfaceRenderPrimitive::Glyphs {
                        style: common,
                        font: resource,
                        font_size: style.font_size,
                        glyphs,
                    }
                }
                SurfaceItemContent::GlyphRun(glyphs) => {
                    let Some(font) = assets
                        .get(resource.key)
                        .and_then(|resource| resource.data())
                        .and_then(|asset| {
                            asset
                                .decoded()
                                .downcast_ref::<crate::services::asset_management::font::FontAsset>(
                                )
                        })
                    else {
                        continue;
                    };
                    if glyphs
                        .iter()
                        .any(|glyph| font.glyph(glyph.glyph_id).is_none())
                    {
                        continue;
                    }
                    let reuse_glyphs = previous.is_some_and(|index| {
                        let SurfaceRenderPrimitive::Glyphs {
                            font,
                            font_size,
                            glyphs: prepared,
                            ..
                        } = &prepared.primitives[index]
                        else {
                            return false;
                        };
                        font.key == resource.key
                            && *font_size == style.font_size
                            && prepared.len() == glyphs.len()
                            && prepared.iter().zip(glyphs).all(|(prepared, glyph)| {
                                prepared.glyph_id == glyph.glyph_id
                                    && prepared.position == glyph.position
                                    && prepared.color == glyph.color
                            })
                    });
                    let mut prepared_glyphs = take_glyphs(&mut prepared.primitives, previous);
                    if !reuse_glyphs {
                        prepared_glyphs.clear();
                        prepared_glyphs.extend(glyphs.iter().copied().map(SurfaceGlyph::from));
                    }
                    SurfaceRenderPrimitive::Glyphs {
                        style: common,
                        font: resource,
                        font_size: style.font_size,
                        glyphs: prepared_glyphs,
                    }
                }
                SurfaceItemContent::Drawing => SurfaceRenderPrimitive::Drawing {
                    style: common,
                    drawing: resource,
                },
                SurfaceItemContent::Bitmap {
                    size,
                } => SurfaceRenderPrimitive::Bitmap {
                    style: common,
                    bitmap: resource,
                    size: *size,
                },
            };
            // An empty clip intersection suppresses the primitive while
            // retaining painter order for the surviving entries. Authored
            // items always select the root rectangle and reach this check
            // intact; GUI output with an empty clip never enters retained
            // output or GPU draws.
            if !surface_primitive_visible(&primitive, [surface.width, surface.height]) {
                continue;
            }
            if written < prepared.primitives.len() {
                prepared.primitives[written] = primitive;
            } else {
                prepared.primitives.push(primitive);
            }
            written += 1;
        }
        prepared.primitives.truncate(written);
    }
    output.truncate(output_index);
    if rebuild_primitives {
        cache.labels.retain(|key, _| retained_labels.contains(key));
    }
    #[cfg(test)]
    if prepared_primitives {
        cache.primitive_preparations += 1;
    }
}

impl SurfaceRenderPrimitive {
    fn identity(&self) -> SurfacePrimitiveIdentity {
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
            } => style.identity,
            #[cfg(feature = "gui")]
            Self::Box {
                style,
                ..
            } => style.identity,
        }
    }

    /// Borrow the shared evaluated style of any primitive kind.
    pub fn style(&self) -> &SurfacePrimitiveStyle {
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
            } => style,
            #[cfg(feature = "gui")]
            Self::Box {
                style,
                ..
            } => style,
        }
    }
}

fn take_glyphs(
    primitives: &mut [SurfaceRenderPrimitive],
    previous: Option<usize>,
) -> Vec<SurfaceGlyph> {
    let Some(previous) = previous else {
        return Vec::new();
    };
    let SurfaceRenderPrimitive::Glyphs {
        glyphs,
        ..
    } = &mut primitives[previous]
    else {
        return Vec::new();
    };
    std::mem::take(glyphs)
}

fn resolve_resource(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    source: AssetSource,
) -> Option<SurfaceRenderResource> {
    let key = assets.find_source(world.id, source.kind, &source.uri, source.variant)?;
    assets.get(key)?.data()?;
    Some(SurfaceRenderResource {
        key,
        source,
    })
}

/// Append retained GUI paint primitives to one prepared Surface item.
///
/// Render preparation calls this for GUI-owned Surfaces after authored
/// items resolve, so generated output shares the common preparation
/// boundary (entity model, anchor, root clip) while keeping disjoint
/// primitive identities. Previously retained GUI primitives are replaced
/// wholesale: authored primitives are untouched, and the supplied paint
/// arrives in painter order. Primitives failing the shared visibility
/// predicate (empty clips, invalid box parameters) are dropped, matching
/// authored preparation.
#[cfg(feature = "gui")]
pub fn append_gui_surface_primitives(
    prepared: &mut SurfaceRenderItem,
    primitives: Vec<SurfaceRenderPrimitive>,
    surface_size: [f32; 2],
) {
    prepared
        .primitives
        .retain(|primitive| !matches!(primitive.identity(), SurfacePrimitiveIdentity::Gui(_)));
    prepared.primitives.extend(
        primitives
            .into_iter()
            .filter(|primitive| surface_primitive_visible(primitive, surface_size)),
    );
}

/// Translate retained GUI paint by scrolled-ancestor shifts, in Surface
/// content metres.
///
/// `shifts` maps `(node, lifetime)` to the node's ancestor scroll shift;
/// only primitives whose identity matches a key exactly move, fencing node
/// reuse after removal and recreation. Authored primitives and GUI
/// primitives without a shift pass through unchanged, so unscrolled
/// subtrees stay byte-identical. Every kind moves its `position` while
/// glyph payloads, box parameters and bitmap sizes stay put; per-primitive
/// clips stay fixed because retained clips compose ScrollView viewport
/// rects in final coordinates, matching scroll-aware hit testing.
/// Non-finite shifts never move paint.
#[cfg(feature = "gui")]
pub fn translate_gui_primitives_for_scroll(
    primitives: Vec<SurfaceRenderPrimitive>,
    shifts: &BTreeMap<(crate::systems::gui::GuiNodeId, u32), [f32; 2]>,
) -> Vec<SurfaceRenderPrimitive> {
    primitives
        .into_iter()
        .map(|primitive| {
            let SurfacePrimitiveIdentity::Gui(id) = primitive.style().identity else {
                return primitive;
            };
            let shift = shifts
                .get(&(id.node, id.lifetime))
                .copied()
                .unwrap_or([0.0, 0.0]);
            if shift == [0.0, 0.0] || !shift.iter().all(|lane| lane.is_finite()) {
                return primitive;
            }
            let mut moved = primitive.clone();
            let style = match &mut moved {
                SurfaceRenderPrimitive::Glyphs {
                    style,
                    ..
                }
                | SurfaceRenderPrimitive::Drawing {
                    style,
                    ..
                }
                | SurfaceRenderPrimitive::Bitmap {
                    style,
                    ..
                } => style,
                SurfaceRenderPrimitive::Box {
                    style,
                    ..
                } => style,
            };
            style.position = [style.position[0] + shift[0], style.position[1] + shift[1]];
            moved
        })
        .collect()
}

/// Appearance-only style override for one primitive, preserving identity.
///
/// Copies the primitive with the supplied colour, opacity, and scale applied
/// to its shared style. Position, clip, identity, and kind-specific payloads
/// (glyphs, resources, box geometry) are unchanged, so behaviour, layout, and
/// painter order never move. Invalid lanes (non-finite values, colours or
/// opacity outside `0..=1`) keep the base field instead of flowing into paint.
pub fn surface_primitive_with_skin_style(
    primitive: &SurfaceRenderPrimitive,
    color: Option<[f32; 4]>,
    opacity: Option<f32>,
    scale: Option<[f32; 2]>,
) -> SurfaceRenderPrimitive {
    let valid_color = color.filter(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    let valid_opacity = opacity.filter(|o| o.is_finite() && (0.0..=1.0).contains(o));
    let valid_scale = scale.filter(|s| s.iter().all(|v| v.is_finite()));
    if valid_color.is_none() && valid_opacity.is_none() && valid_scale.is_none() {
        return primitive.clone();
    }
    let mut next = primitive.clone();
    let style = match &mut next {
        SurfaceRenderPrimitive::Glyphs {
            style,
            ..
        }
        | SurfaceRenderPrimitive::Drawing {
            style,
            ..
        }
        | SurfaceRenderPrimitive::Bitmap {
            style,
            ..
        } => style,
        #[cfg(feature = "gui")]
        SurfaceRenderPrimitive::Box {
            style,
            ..
        } => style,
    };
    if let Some(color) = valid_color {
        style.color = color;
    }
    if let Some(opacity) = valid_opacity {
        style.opacity = opacity;
    }
    if let Some(scale) = valid_scale {
        style.scale = scale;
    }
    next
}

fn content_to_world_matrix(entity_model: &[f32; 16], width: f32, height: f32) -> [f32; 16] {
    let half_w = width * 0.5;
    let half_h = height * 0.5;
    [
        entity_model[0],
        entity_model[1],
        entity_model[2],
        entity_model[3],
        -entity_model[4],
        -entity_model[5],
        -entity_model[6],
        -entity_model[7],
        entity_model[8],
        entity_model[9],
        entity_model[10],
        entity_model[11],
        entity_model[12] - half_w * entity_model[0] + half_h * entity_model[4],
        entity_model[13] - half_w * entity_model[1] + half_h * entity_model[5],
        entity_model[14] - half_w * entity_model[2] + half_h * entity_model[6],
        entity_model[15] - half_w * entity_model[3] + half_h * entity_model[7],
    ]
}

impl From<PositionedGlyph> for SurfaceGlyph {
    fn from(value: PositionedGlyph) -> Self {
        Self {
            glyph_id: value.glyph_id,
            position: value.position,
            color: value.color,
        }
    }
}

#[cfg(test)]
#[path = "gui_output_tests.rs"]
mod gui_output_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> crate::services::asset_management::font::FontAsset {
        let mut bytes = b"IPPF".to_vec();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&1000_u32.to_le_bytes());
        for value in [800.0_f32, -200.0, 100.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1_u32, 1, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [500.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&65_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        crate::services::asset_management::font::FontAsset::decode(&bytes).unwrap()
    }

    #[test]
    fn label_layout_cache_survives_numeric_and_camera_only_preparation() {
        let mut cache = SurfaceLayoutCache::default();
        let entity = crate::EntityId::from_bits(1);
        let item = SurfaceItemId(1);
        let asset = AssetKey {
            slot: 1,
            generation: 1,
        };
        let font = font();
        let first = cache
            .label_prepared((entity, item), "A\nA", asset, 0.1, &font)
            .unwrap()
            .0
            .glyphs
            .to_vec();
        let second = cache
            .label_prepared((entity, item), "A\nA", asset, 0.1, &font)
            .unwrap()
            .0
            .glyphs
            .to_vec();
        assert_eq!(first, second);
        assert_eq!(cache.rebuilds, 1);
        // Shared headless metrics keep original IDs and Y-down growth.
        assert_eq!(first[0].glyph_id, 0);
        assert!(first[1].position[1] > first[0].position[1]);

        let empty_item = SurfaceItemId(2);
        let (empty, rebuilt) = cache
            .label_prepared((entity, empty_item), "", asset, 0.1, &font)
            .unwrap();
        assert!(rebuilt);
        assert!(empty.glyphs.is_empty());
        assert_eq!(empty.grapheme_boundaries, vec![0]);
        assert_eq!(cache.rebuilds, 2);
    }
}
