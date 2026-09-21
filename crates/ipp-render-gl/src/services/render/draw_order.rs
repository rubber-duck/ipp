//! Frame-local material ordering. Fingerprints affect ordering only, never draw
//! merging or upload elision: a hash collision cannot change submitted state.

use super::{
    custom_material::PreparedCustomMaterial,
    frame_scratch::{RenderDraw, RenderDrawIndex},
    light_selection::PreparedLighting,
    shader::RenderShaderConfig,
};
use ipp_core::{DebugRenderItem, EntityId, RenderItem};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RenderMaterialKey {
    shader: u64,
    variant: u32,
    recipe: u32,
    values: u64,
}

pub(super) fn builtin_config(item: &RenderItem, shadow: bool) -> RenderShaderConfig {
    let config = RenderShaderConfig::new(item.texture.is_some(), item.texture_weights)
        .with_solid_fallback(item.solid_fallback);
    let config = if item.pbr.is_some() {
        config.with_lighting(shadow, item.normals)
    } else {
        config
    };
    #[cfg(feature = "skeletal-animation")]
    let config = config.with_skinning(item.skinned);
    #[cfg(feature = "mesh-poses")]
    let config = config.with_mesh_pose(item.pose.is_some());
    #[cfg(feature = "particles")]
    let config = config.with_particles(
        item.particle.is_some(),
        item.particle.is_some_and(|p| p.sprite),
    );
    config
}

fn material_key(
    item: &RenderItem,
    custom: Option<&PreparedCustomMaterial>,
    shadow: bool,
) -> RenderMaterialKey {
    let mut hash = DefaultHasher::new();
    if let Some(custom) = custom {
        custom.words.hash(&mut hash);
        for (_, texture) in &custom.textures {
            texture.to_u64().hash(&mut hash);
        }
        custom.material.alpha_mode.hash(&mut hash);
        custom.material.alpha_cutoff.to_bits().hash(&mut hash);
        custom.material.receives_shadows.hash(&mut hash);
        return RenderMaterialKey {
            shader: custom.key.asset,
            variant: custom.key.variant,
            recipe: custom.key.config.recipe_bits() | (u32::from(custom.key.lit) << 12),
            values: hash.finish(),
        };
    }
    for value in [item.material.r, item.material.g, item.material.b] {
        value.to_bits().hash(&mut hash);
    }
    if let Some(pbr) = item.pbr {
        pbr.metallic.to_bits().hash(&mut hash);
        pbr.roughness.to_bits().hash(&mut hash);
        pbr.receive_shadows.hash(&mut hash);
    }
    item.texture.map(|t| (t.asset, t.variant)).hash(&mut hash);
    RenderMaterialKey {
        recipe: builtin_config(item, shadow).recipe_bits(),
        values: hash.finish(),
        ..Default::default()
    }
}

fn depth(item: &RenderItem, view_projection: &[f32; 16]) -> f64 {
    // Clip Z is affine and monotonic in view depth for both supported projections.
    f64::from(view_projection[2]) * f64::from(item.model[12])
        + f64::from(view_projection[6]) * f64::from(item.model[13])
        + f64::from(view_projection[10]) * f64::from(item.model[14])
        + f64::from(view_projection[14])
}

#[cfg(feature = "surfaces")]
fn surface_depth(item: &ipp_core::SurfaceRenderItem, view_projection: &[f32; 16]) -> f64 {
    f64::from(view_projection[2]) * f64::from(item.anchor[0])
        + f64::from(view_projection[6]) * f64::from(item.anchor[1])
        + f64::from(view_projection[10]) * f64::from(item.anchor[2])
        + f64::from(view_projection[14])
}

pub(super) fn prepare(
    draws: &mut Vec<RenderDraw>,
    items: &[RenderItem],
    debug: &[DebugRenderItem],
    #[cfg(feature = "surfaces")] surfaces: &[ipp_core::SurfaceRenderItem],
    customs: &BTreeMap<EntityId, PreparedCustomMaterial>,
    lighting: &PreparedLighting,
    view_projection: &[f32; 16],
) {
    draws.clear();
    let mut index = 0;
    while index < items.len() {
        let item = &items[index];
        let custom = customs.get(&item.entity);
        #[cfg(feature = "particles")]
        let sprite = item.particle.filter(|p| p.sprite);
        #[cfg(feature = "particles")]
        let transparent = sprite.is_some();
        #[cfg(not(feature = "particles"))]
        let transparent = false;
        let transparent = transparent || custom.is_some_and(|c| c.material.alpha_mode == 2);
        let material = if transparent {
            RenderMaterialKey::default()
        } else {
            material_key(
                item,
                custom,
                lighting
                    .draws
                    .get(&item.entity)
                    .is_some_and(|d| d.frame.shadow_count > 0),
            )
        };
        let mut end = index + 1;
        #[allow(unused_mut)] // Particle groups share a depth; ordinary builds keep it immutable.
        let mut z = depth(item, view_projection);
        #[cfg(feature = "particles")]
        if !transparent && item.particle.is_some() {
            while end < items.len()
                && items[end].entity == item.entity
                && items[end].particle.is_some()
            {
                z = z.min(depth(&items[end], view_projection));
                end += 1;
            }
        }
        #[cfg(not(feature = "particles"))]
        let _ = &mut end;
        #[cfg(feature = "particles")]
        if sprite.is_some_and(|p| p.additive) {
            z = f64::INFINITY;
        }
        // Mesh particles use one group depth, retaining their single instanced draw.
        for i in index..end {
            draws.push(RenderDraw {
                index: RenderDrawIndex::Visual(i),
                key: (item.entity, 0),
                material,
                phase: if transparent {
                    2
                } else {
                    0
                },
                depth: z,
            });
        }
        index = end;
    }
    draws.extend(debug.iter().enumerate().map(|(index, item)| RenderDraw {
        index: RenderDrawIndex::Debug(index),
        key: (item.entity, 1),
        material: RenderMaterialKey::default(),
        phase: 1,
        depth: 0.0,
    }));
    #[cfg(feature = "surfaces")]
    draws.extend(surfaces.iter().enumerate().map(|(index, item)| RenderDraw {
        index: RenderDrawIndex::Surface(index),
        key: (item.entity, 2),
        material: RenderMaterialKey::default(),
        phase: 2,
        depth: surface_depth(item, view_projection),
    }));
    draws.sort_unstable_by(RenderDraw::compare);
}

#[cfg(test)]
#[path = "draw_order_tests.rs"]
mod tests;
