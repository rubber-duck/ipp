//! Prepared inputs for optional whole-Surface texture caching.
//!
//! RenderSystem publishes, beside each [`SurfaceRenderItem`], the authored
//! [`SurfaceCachePolicy`], a paint revision, a resource revision and (with
//! GUI) live interaction priority. The renderer compares these values to
//! reuse, refresh or bypass a cached image; it never inspects World state.
//! Everything here is transient and reconstructed from evaluated inputs:
//! nothing is persisted, and no component value is mirrored.
//!
//! # Revisions
//!
//! Both revisions come from one counter owned by this World's RenderSystem.
//! Every issued value is larger than all earlier ones, so a revision never
//! repeats within the World and a Surface that stops being tracked (policy
//! removed, Surface removed, entity deleted) takes fresh values when it is
//! tracked again. Surfaces without a valid policy publish zero for both.
//!
//! Revisions are derived after RenderSystem re-prepares Surface primitives,
//! which it already does only when some painted input changed. The tracker
//! hashes the prepared paint of each opted-in Surface on those frames and
//! issues a new paint revision when the hash differs from the previous one.
//! Frames without re-preparation (camera motion, unrelated edits) do no
//! hashing and keep every revision. Each painted input reaches the hash
//! through its existing change tracking:
//!
//! - Authored item content, style, order and numeric item animation: Surface
//!   component commits and numeric notifications.
//! - Surface width and height (the clip size): the same Surface changes.
//! - GUI layout, node paint, theme values and font readiness: GUI layout
//!   paint revisions.
//! - Scroll offsets: the GUI input scroll revision.
//! - Skin output for hover, press and focus: GUI skin cursors; animated or
//!   edited skin appearance: skin presentation updates.
//! - Caret, selection and provisional composition: the GUI caret revision.
//! - Resource readiness, replacement and release: asset lifecycle hooks,
//!   which re-prepare every Surface.
//!
//! Placement is excluded: entity transforms, hierarchy and the camera only
//! change `model` and `anchor`, which the hash never reads. Primitive
//! identities are also excluded because they do not change pixels.
//!
//! The resource revision changes when the set of resource identities an
//! item references changes. Resource payloads are immutable per generational
//! key and a primitive is prepared only while its resource is ready, so this
//! covers a resource becoming ready, replacement by another identity and
//! release. The renderer applies it without waiting for the refresh cadence.
//! A resource change also changes the paint hash, so both revisions advance
//! together.
//!
//! # Interaction priority
//!
//! With GUI, `interaction` mirrors [`GuiInputSystem::interaction_roots`]
//! every frame: keyboard focus, hover, press or capture on the Surface's
//! GuiRoot. The input system cancels those cursors when a node or root is
//! removed or disabled, so the flag clears in the same frame.
//!
//! [`GuiInputSystem::interaction_roots`]: crate::systems::gui::GuiInputSystem

use crate::EntityId;
use crate::SurfaceRenderItem;
use crate::services::asset_management::AssetKey;
use crate::systems::surface::{
    SurfaceCachePolicy, SurfaceGlyph, SurfacePrimitiveStyle, SurfaceRenderPrimitive,
    SurfaceRenderResource,
};
use crate::world::WorldSimulationState;
use std::collections::BTreeMap;
#[cfg(feature = "gui")]
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hasher};

/// Per-World bookkeeping for the prepared cache inputs.
#[derive(Default)]
pub(in crate::world) struct SurfaceCacheInputs {
    /// Hashes last published for each opted-in Surface.
    tracked: BTreeMap<EntityId, SurfaceCacheInputHashes>,
    /// Last issued revision; zero means none was issued.
    last_revision: u64,
    /// Reused buffer for one item's distinct resource identities.
    resource_keys: Vec<AssetKey>,
    /// GuiRoot entities with interaction priority in the last published frame.
    #[cfg(feature = "gui")]
    interaction_roots: BTreeSet<EntityId>,
}

#[derive(Clone, Copy)]
struct SurfaceCacheInputHashes {
    paint: u64,
    resources: u64,
}

impl SurfaceCacheInputs {
    /// Publish the policy and revisions of freshly prepared Surface items.
    ///
    /// `items` must be the complete prepared output in ascending entity
    /// order. Call this after every Surface preparation, once all authored
    /// and GUI primitives are in place.
    pub(in crate::world) fn publish(
        &mut self,
        world: &WorldSimulationState,
        items: &mut [SurfaceRenderItem],
    ) {
        self.publish_with(items, |entity| {
            // Mutation validates authored values; values that fail here (for
            // example after an unchecked numeric write) present directly.
            world
                .components
                .surface_cache(entity.index() as usize)
                .and_then(|component| SurfaceCachePolicy::new(component).ok())
        });
    }

    fn publish_with(
        &mut self,
        items: &mut [SurfaceRenderItem],
        policy: impl Fn(EntityId) -> Option<SurfaceCachePolicy>,
    ) {
        for item in items.iter_mut() {
            item.cache = policy(item.entity);
            if item.cache.is_none() {
                item.paint_revision = 0;
                item.resource_revision = 0;
                continue;
            }

            let hashes = SurfaceCacheInputHashes {
                paint: paint_hash(item),
                resources: resource_hash(item, &mut self.resource_keys),
            };
            let previous = self
                .tracked
                .insert(item.entity, hashes)
                .filter(|_| item.paint_revision != 0);
            let Some(previous) = previous else {
                item.paint_revision = next_revision(&mut self.last_revision);
                item.resource_revision = next_revision(&mut self.last_revision);
                continue;
            };

            if previous.paint != hashes.paint {
                item.paint_revision = next_revision(&mut self.last_revision);
            }

            if previous.resources != hashes.resources {
                item.resource_revision = next_revision(&mut self.last_revision);
            }
        }

        self.tracked.retain(|entity, _| {
            items
                .binary_search_by_key(entity, |item| item.entity)
                .is_ok_and(|index| items[index].cache.is_some())
        });
    }

    /// Publish live GUI interaction priority.
    ///
    /// Items are rewritten only when the interaction set changed or when
    /// `prepared` reports that Surface items were re-prepared this frame.
    #[cfg(feature = "gui")]
    pub(in crate::world) fn publish_interaction(
        &mut self,
        roots: BTreeSet<EntityId>,
        items: &mut [SurfaceRenderItem],
        prepared: bool,
    ) {
        if !prepared && roots == self.interaction_roots {
            return;
        }

        for item in items {
            item.interaction = roots.contains(&item.entity);
        }

        self.interaction_roots = roots;
    }
}

fn next_revision(last: &mut u64) -> u64 {
    *last += 1;
    *last
}

fn primitive_resource(primitive: &SurfaceRenderPrimitive) -> Option<&SurfaceRenderResource> {
    match primitive {
        SurfaceRenderPrimitive::Glyphs {
            font,
            ..
        } => Some(font),
        SurfaceRenderPrimitive::Drawing {
            drawing,
            ..
        } => Some(drawing),
        SurfaceRenderPrimitive::Bitmap {
            bitmap,
            ..
        } => Some(bitmap),
        #[cfg(feature = "gui")]
        SurfaceRenderPrimitive::Box {
            ..
        } => None,
    }
}

/// Distinct resource identities referenced by one prepared item, in key
/// order; how often or where an identity is used belongs to the paint hash.
fn resource_hash(item: &SurfaceRenderItem, keys: &mut Vec<AssetKey>) -> u64 {
    keys.clear();
    keys.extend(
        item.primitives
            .iter()
            .filter_map(primitive_resource)
            .map(|resource| resource.key),
    );
    keys.sort_unstable();
    keys.dedup();

    let mut hasher = DefaultHasher::new();
    for key in keys.iter() {
        write_key(&mut hasher, *key);
    }

    hasher.finish()
}

/// Everything that determines the painted pixels of one prepared item.
///
/// The destructuring below lists every field without `..`, so adding a
/// painted field to a primitive fails to compile until it is hashed here or
/// explicitly ignored.
fn paint_hash(item: &SurfaceRenderItem) -> u64 {
    let mut hasher = DefaultHasher::new();
    write_f32s(&mut hasher, &item.clip_size);
    hasher.write_usize(item.primitives.len());
    for primitive in &item.primitives {
        match primitive {
            SurfaceRenderPrimitive::Glyphs {
                style,
                font,
                font_size,
                glyphs,
            } => {
                hasher.write_u8(0);
                write_style(&mut hasher, style);
                write_resource(&mut hasher, font);
                write_f32s(&mut hasher, &[*font_size]);
                hasher.write_usize(glyphs.len());
                for SurfaceGlyph {
                    glyph_id,
                    position,
                    color,
                } in glyphs
                {
                    hasher.write_u32(*glyph_id);
                    write_f32s(&mut hasher, position);
                    write_optional_f32s(&mut hasher, color.as_ref().map(|color| &color[..]));
                }
            }
            SurfaceRenderPrimitive::Drawing {
                style,
                drawing,
            } => {
                hasher.write_u8(1);
                write_style(&mut hasher, style);
                write_resource(&mut hasher, drawing);
            }
            SurfaceRenderPrimitive::Bitmap {
                style,
                bitmap,
                size,
            } => {
                hasher.write_u8(2);
                write_style(&mut hasher, style);
                write_resource(&mut hasher, bitmap);
                write_f32s(&mut hasher, size);
            }
            #[cfg(feature = "gui")]
            SurfaceRenderPrimitive::Box {
                style,
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
            } => {
                hasher.write_u8(3);
                write_style(&mut hasher, style);
                write_f32s(&mut hasher, size);
                write_f32s(&mut hasher, corner_radius);
                write_f32s(&mut hasher, &[*border_width]);
                write_f32s(&mut hasher, border_color);
                write_fill(&mut hasher, fill);
                write_glow(&mut hasher, glow.as_ref());
            }
        }
    }

    hasher.finish()
}

fn write_style(hasher: &mut DefaultHasher, style: &SurfacePrimitiveStyle) {
    let SurfacePrimitiveStyle {
        identity: _,
        position,
        scale,
        color,
        opacity,
        clip,
    } = style;
    write_f32s(hasher, position);
    write_f32s(hasher, scale);
    write_f32s(hasher, color);
    write_f32s(hasher, &[*opacity]);
    write_optional_f32s(hasher, clip.as_ref().map(|clip| &clip[..]));
}

fn write_resource(hasher: &mut DefaultHasher, resource: &SurfaceRenderResource) {
    let SurfaceRenderResource {
        key,
        source: _,
    } = resource;
    write_key(hasher, *key);
}

fn write_key(hasher: &mut DefaultHasher, key: AssetKey) {
    hasher.write_u32(key.slot);
    hasher.write_u32(key.generation);
}

#[cfg(feature = "gui")]
fn write_fill(hasher: &mut DefaultHasher, fill: &crate::systems::surface::GuiShapeFill) {
    use crate::systems::surface::GuiShapeFill;

    match fill {
        GuiShapeFill::Solid(color) => {
            hasher.write_u8(0);
            write_f32s(hasher, color);
        }
        GuiShapeFill::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => {
            hasher.write_u8(1);
            write_f32s(hasher, start);
            write_f32s(hasher, end);
            write_f32s(hasher, start_color);
            write_f32s(hasher, end_color);
        }
        GuiShapeFill::RadialGradient {
            center,
            radius,
            start_color,
            end_color,
        } => {
            hasher.write_u8(2);
            write_f32s(hasher, center);
            write_f32s(hasher, &[*radius]);
            write_f32s(hasher, start_color);
            write_f32s(hasher, end_color);
        }
    }
}

#[cfg(feature = "gui")]
fn write_glow(hasher: &mut DefaultHasher, glow: Option<&crate::systems::surface::GuiShapeGlow>) {
    let Some(crate::systems::surface::GuiShapeGlow {
        color,
        intensity,
        radius,
        falloff,
    }) = glow
    else {
        hasher.write_u8(0);
        return;
    };

    hasher.write_u8(1);
    write_f32s(hasher, color);
    write_f32s(hasher, &[*intensity, *radius, *falloff]);
}

fn write_optional_f32s(hasher: &mut DefaultHasher, values: Option<&[f32]>) {
    match values {
        Some(values) => {
            hasher.write_u8(1);
            write_f32s(hasher, values);
        }
        None => hasher.write_u8(0),
    }
}

fn write_f32s(hasher: &mut DefaultHasher, values: &[f32]) {
    for value in values {
        hasher.write_u32(value.to_bits());
    }
}

#[cfg(test)]
#[path = "surface_cache_inputs_tests.rs"]
mod tests;
