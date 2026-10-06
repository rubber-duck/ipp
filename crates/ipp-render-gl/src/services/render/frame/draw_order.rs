//! Frame-local material ordering. Fingerprints affect ordering only, never draw
//! merging or upload elision: a hash collision cannot change submitted state.

use super::super::outputs::scene::{
    RenderEntity as EntityId, SceneDebug as DebugRenderItem, SceneItem as RenderItem,
};
use super::super::{
    lighting::selection::PreparedLighting,
    materials::{custom_material::PreparedCustomMaterial, shader::RenderShaderConfig},
};
use super::scratch::{RenderDraw, RenderDrawIndex};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::services::render) struct RenderMaterialKey {
    shader: u64,
    variant: u32,
    recipe: u32,
    values: u64,
}

pub(in crate::services::render) fn builtin_config(
    item: &ipp_core::RenderItem,
    shadow: bool,
) -> RenderShaderConfig {
    let config = RenderShaderConfig::new(item.texture.is_some(), item.texture_weights)
        .with_solid_fallback(item.solid_fallback);
    let config = if item.pbr.is_some() {
        config.with_lighting(shadow, item.normals)
    } else {
        config
    };
    config
        .with_skinning(item.skinned)
        .with_mesh_pose(item.pose.is_some())
        .with_particles(
            item.particle.is_some(),
            item.particle.is_some_and(|p| p.sprite),
        )
}

fn material_key(
    item: &ipp_core::RenderItem,
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

fn depth(item: &RenderItem<'_>, view_projection: &[f32; 16]) -> f64 {
    // Clip Z is affine and monotonic in view depth for both supported projections.
    f64::from(view_projection[2]) * f64::from(item.model[12])
        + f64::from(view_projection[6]) * f64::from(item.model[13])
        + f64::from(view_projection[10]) * f64::from(item.model[14])
        + f64::from(view_projection[14])
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    draws: &mut Vec<RenderDraw>,
    items: &[RenderItem<'_>],
    debug: &[DebugRenderItem],
    surfaces: &[super::super::outputs::scene::SceneOutputSurface],
    plot_planes: &[super::super::outputs::scene::ScenePlotPlane<'_>],
    customs: &BTreeMap<EntityId, PreparedCustomMaterial>,
    lighting: &PreparedLighting,
    view_projection: &[f32; 16],
) {
    draws.clear();
    let mut index = 0;
    while index < items.len() {
        let item = &items[index];
        let custom = customs.get(&item.entity);
        let sprite = item.particle.filter(|p| p.sprite);
        let transparent = sprite.is_some()
            || custom.is_some_and(|c| c.material.alpha_mode == 2)
            || item
                .derived
                .is_some_and(|(_, mesh)| mesh.geometry.meshes[mesh.mesh_index].color[3] < 1.0);
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
        let mut z = depth(item, view_projection);
        if !transparent && item.particle.is_some() {
            while end < items.len()
                && items[end].entity == item.entity
                && items[end].particle.is_some()
            {
                z = z.min(depth(&items[end], view_projection));
                end += 1;
            }
        }
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

    draws.extend(
        surfaces
            .iter()
            .enumerate()
            .filter(|(_, item)| item.geometry.exact_affine(0.0).is_some())
            .map(|(index, item)| RenderDraw {
                index: RenderDrawIndex::Surface(index),
                key: (item.entity, 2),
                material: RenderMaterialKey::default(),
                phase: 2,
                depth: f64::from(view_projection[2]) * f64::from(item.model[12])
                    + f64::from(view_projection[6]) * f64::from(item.model[13])
                    + f64::from(view_projection[10]) * f64::from(item.model[14])
                    + f64::from(view_projection[14]),
            }),
    );
    draws.extend(
        plot_planes
            .iter()
            .enumerate()
            .map(|(index, item)| RenderDraw {
                index: RenderDrawIndex::PlotPlane(index),
                key: (item.entity, 3),
                material: RenderMaterialKey::default(),
                phase: 2,
                depth: f64::from(view_projection[2]) * f64::from(item.model[12])
                    + f64::from(view_projection[6]) * f64::from(item.model[13])
                    + f64::from(view_projection[10]) * f64::from(item.model[14])
                    + f64::from(view_projection[14]),
            }),
    );
    draws.sort_unstable_by(RenderDraw::compare);
}

#[cfg(test)]
#[path = "draw_order_tests.rs"]
mod tests;
