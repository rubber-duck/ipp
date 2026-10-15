use super::*;
use ipp_core::{
    EntityId, MeshKey, RenderItem,
    components::{Transform, UnlitMaterial},
};

fn item(id: u64, z: f32) -> RenderItem {
    let mut model = [0.0; 16];
    for i in [0, 5, 10, 15] {
        model[i] = 1.0;
    }
    model[14] = z;
    RenderItem {
        particle: None,
        solid_fallback: false,
        custom_material: false,
        normals: false,
        texture_weights: false,
        skinned: false,
        entity: EntityId::from_bits((1 << 32) | id),
        transform: Transform::default(),
        model,
        normal: Ok(model),
        material: UnlitMaterial::default(),
        pbr: None,
        mesh: MeshKey {
            asset: 1,
            variant: 0,
        },
        pose: None,
        texture: None,
    }
}

fn ordered(items: &[RenderItem], customs: &BTreeMap<EntityId, PreparedCustomMaterial>) -> Vec<u64> {
    let published: Vec<_> = items
        .iter()
        .map(|item| ipp_core::systems::render::PublishedRenderItem {
            item: ipp_core::RenderItem {
                custom_material: customs.contains_key(&item.entity),
                ..*item
            },
            incarnation: 1,
            custom: None,
            palette: None,
        })
        .collect();
    let items: Vec<_> = published
        .iter()
        .map(
            |published| crate::services::render::outputs::scene::SceneItem {
                entity: crate::services::render::test_support::test_entity(
                    published.item.entity.to_bits(),
                ),
                value: published.item,
                published,
                derived: None,
            },
        )
        .collect();
    let customs: BTreeMap<_, _> = customs
        .iter()
        .map(|(entity, custom)| {
            (
                crate::services::render::materials::custom_material::CustomMaterialKey {
                    entity: crate::services::render::test_support::test_entity(entity.to_bits()),
                    mesh: ipp_core::MeshKey {
                        asset: 1,
                        variant: 0,
                    },
                    component: ipp_core::ComponentValue::MESH_INSTANCE,
                },
                custom.clone(),
            )
        })
        .collect();
    let mut draws = Vec::new();
    prepare(
        &mut draws,
        &items,
        &[],
        &[],
        &[],
        &customs,
        &PreparedLighting::default(),
        &[
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    draws
        .iter()
        .map(|d| u64::from(d.key.0.entity.index()))
        .collect()
}

fn custom(shader: u64, alpha_mode: u32) -> PreparedCustomMaterial {
    let mut custom = PreparedCustomMaterial::default();
    custom.key.asset = shader;
    custom.material.alpha_mode = alpha_mode;
    custom
}

#[test]
fn same_entity_material_candidates_keep_geometry_and_component_lifetimes_separate() {
    use crate::services::render::outputs::scene::SceneItem;
    use ipp_core::systems::plot::{PlotMeshVariant, PlotPreparedGeometry, PlotPublishedMesh};
    use ipp_core::systems::render::PublishedRenderItem;
    let ordinary = PublishedRenderItem {
        item: RenderItem {
            custom_material: true,
            ..item(12, 0.0)
        },
        incarnation: 1,
        custom: None,
        palette: None,
    };
    let plot = PlotPublishedMesh {
        render: PublishedRenderItem {
            item: RenderItem {
                mesh: MeshKey {
                    asset: 0,
                    variant: 0,
                },
                ..ordinary.item
            },
            ..ordinary.clone()
        },
        geometry: std::sync::Arc::new(PlotPreparedGeometry::default()),
        mesh_index: 0,
        variant: PlotMeshVariant::Data,
    };
    let entity = crate::services::render::test_support::test_entity(ordinary.item.entity.to_bits());
    let target = ipp_core::systems::canvas::CanvasTarget {
        entity: ordinary.item.entity,
        component: ipp_core::ComponentValue::PLOT_POINTS3D,
        incarnation: 1,
    };
    let mesh_item = SceneItem {
        entity,
        value: ordinary.item,
        published: &ordinary,
        derived: None,
    };
    let plot_item = SceneItem {
        entity,
        value: plot.render.item,
        published: &plot.render,
        derived: Some((target, &plot)),
    };
    let ordinary_key = mesh_item.custom_material_key();
    let plot_key = plot_item.custom_material_key();
    assert_ne!(ordinary_key, plot_key);
    let candidates = BTreeMap::from([(plot_key, custom(7, 0))]);
    assert!(!candidates.contains_key(&ordinary_key));
    assert_eq!(candidates.get(&plot_key).unwrap().key.asset, 7);
    let mut replacement = plot_item;
    replacement.entity.incarnation += 1;
    assert!(!candidates.contains_key(&replacement.custom_material_key()));
    let mut another_component = replacement;
    another_component.entity = entity;
    another_component.derived.as_mut().unwrap().0.component =
        ipp_core::ComponentValue::PLOT_GRID_BARS3D;
    assert!(!candidates.contains_key(&another_component.custom_material_key()));
}

#[test]
fn opaque_material_groups_precede_depth_with_deterministic_ties() {
    let items = [
        item(0, 1.0),
        item(1, 8.0),
        item(2, 4.0),
        item(3, 2.0),
        item(4, 2.0),
    ];
    let customs = BTreeMap::from([
        (items[0].entity, custom(2, 0)),
        (items[1].entity, custom(1, 0)),
        (items[2].entity, custom(2, 0)),
        (items[3].entity, custom(1, 0)),
        (items[4].entity, custom(1, 0)),
    ]);
    assert_eq!(ordered(&items, &customs), [3, 4, 1, 0, 2]);
}

#[test]
fn transparency_ignores_material_groups_and_keeps_back_to_front_order() {
    let items = [item(0, 1.0), item(1, 8.0), item(2, 4.0), item(3, 2.0)];
    let customs = BTreeMap::from([
        (items[0].entity, custom(1, 2)),
        (items[1].entity, custom(2, 2)),
        (items[2].entity, custom(1, 2)),
        (items[3].entity, custom(3, 0)),
    ]);
    assert_eq!(ordered(&items, &customs), [3, 1, 2, 0]);
}

#[test]
fn material_keys_observe_values_textures_and_shader_variants() {
    let mut a = item(0, 1.0);
    let original = material_key(&a, None, false);
    a.material.r = 0.0;
    assert!(original != material_key(&a, None, false));
    a.material.r = 1.0;
    a.texture = Some(ipp_core::TextureKey {
        asset: 3,
        variant: 0,
    });
    assert!(original != material_key(&a, None, false));
    let mut b = custom(1, 0);
    b.words = vec![1, 2];
    let original = material_key(&a, Some(&b), false);
    b.words[1] = 3;
    assert!(original != material_key(&a, Some(&b), false));
    b.words[1] = 2;
    b.key.variant = 2;
    assert!(original != material_key(&a, Some(&b), false));
}

#[test]
fn opaque_particle_group_depth_preserves_instanced_submission() {
    let mut items = [
        item(0, 10.0),
        item(0, 1.0),
        item(1, 5.0),
        item(1, 2.0),
        item(2, 3.0),
    ];
    for (index, item) in items[..4].iter_mut().enumerate() {
        item.particle = Some(ipp_core::systems::particles::ParticleRenderData {
            id: index as u64,
            sprite: false,
            additive: false,
            velocity_aligned: false,
            opacity: 1.0,
            velocity: [0.0; 3],
        });
    }
    let order = ordered(&items, &BTreeMap::new());
    let particles: Vec<_> = order.into_iter().filter(|id| *id != 2).collect();
    assert_eq!(particles, [0, 0, 1, 1]);
}
