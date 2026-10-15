use super::*;
use crate::systems::canvas::CanvasTarget;
use crate::systems::render::{PublishedRenderItem, RenderItem, UnlitMaterial};

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn chart() -> PlotPublishedChart {
    let entity = crate::EntityId::from_bits((1 << 32) | 1);
    let geometry = Arc::new(PlotPreparedGeometry {
        meshes: vec![PlotMesh {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            ..Default::default()
        }],
        ..Default::default()
    });
    PlotPublishedChart {
        target: CanvasTarget {
            entity,
            component: crate::ComponentValue::PLOT_POINTS3D,
            incarnation: 1,
        },
        geometry: geometry.clone(),
        model: IDENTITY,
        meshes: vec![PlotPublishedMesh {
            geometry,
            mesh_index: 0,
            variant: PlotMeshVariant::Default,
            render: PublishedRenderItem {
                item: RenderItem {
                    particle: None,
                    solid_fallback: false,
                    custom_material: false,
                    normals: false,
                    texture_weights: false,
                    skinned: false,
                    entity,
                    transform: Default::default(),
                    model: IDENTITY,
                    normal: Ok(IDENTITY),
                    material: UnlitMaterial {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                    },
                    pbr: None,
                    mesh: crate::MeshKey {
                        asset: 0,
                        variant: 0,
                    },
                    pose: None,
                    texture: None,
                },
                incarnation: 1,
                custom: None,
                palette: None,
            },
        }],
        planes: Arc::from([]),
        callout_paint: None,
    }
}

#[test]
fn publication_equality_accepts_retained_and_independently_prepared_equal_geometry() {
    let original = chart();
    let retained = original.clone();
    let rebuilt = chart();
    assert!(Arc::ptr_eq(&original.geometry, &retained.geometry));
    assert!(!Arc::ptr_eq(&original.geometry, &rebuilt.geometry));
    assert_eq!(original, retained);
    assert_eq!(original, rebuilt);
}

#[test]
fn publication_equality_detects_placement_lifetime_draw_and_geometry_changes() {
    let original = chart();
    let mut changed = original.clone();
    changed.model[12] = 2.0;
    assert_ne!(original, changed);

    let mut changed = original.clone();
    changed.target.incarnation += 1;
    assert_ne!(original, changed);

    let mut changed = original.clone();
    changed.meshes[0].render.item.material.r = 0.5;
    assert_ne!(original, changed);

    let mut changed = original.clone();
    changed.meshes[0].mesh_index = 1;
    assert_ne!(original, changed);

    let mut changed = chart();
    Arc::make_mut(&mut changed.geometry).meshes[0].positions[0][0] = 0.25;
    assert_ne!(original, changed);

    // Mesh-local references need not equal the chart reference in a constructed
    // publication. The shortcut must not skip their independent value check.
    let mut changed = original.clone();
    Arc::make_mut(&mut changed.meshes[0].geometry).meshes[0]
        .indices
        .swap(0, 1);
    assert_ne!(original, changed);
}
