use super::*;
use ipp_core::{
    MeshKey,
    components::{Transform, UnlitMaterial},
};

fn item(id: u64, z: f32) -> RenderItem {
    let mut model = [0.0; 16];
    for i in [0, 5, 10, 15] {
        model[i] = 1.0;
    }
    model[14] = z;
    RenderItem {
        #[cfg(feature = "particles")]
        particle: None,
        solid_fallback: false,
        custom_material: false,
        normals: false,
        texture_weights: false,
        #[cfg(feature = "skeletal-animation")]
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
        #[cfg(feature = "mesh-poses")]
        pose: None,
        texture: None,
    }
}

fn ordered(items: &[RenderItem], customs: &BTreeMap<EntityId, PreparedCustomMaterial>) -> Vec<u64> {
    let mut draws = Vec::new();
    prepare(
        &mut draws,
        items,
        &[],
        #[cfg(feature = "surfaces")]
        &[],
        customs,
        &PreparedLighting::default(),
        &[
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    draws.iter().map(|d| u64::from(d.key.0.index())).collect()
}

fn custom(shader: u64, alpha_mode: u32) -> PreparedCustomMaterial {
    let mut custom = PreparedCustomMaterial::default();
    custom.key.asset = shader;
    custom.material.alpha_mode = alpha_mode;
    custom
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

#[cfg(feature = "particles")]
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
