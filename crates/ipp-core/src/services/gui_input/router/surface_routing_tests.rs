//! Shared Surface consumers exercised against real headless Host publications.

use crate::services::gui_input::GuiInputError;
use crate::services::gui_input::query::{
    GuiQueryOptions, GuiQueryOutcome, project_composed_point, query_composed_input,
};
use crate::services::gui_input::routing_test_support::*;
use crate::services::gui_input::test_support::*;
use crate::{
    Command, ComponentValue, CylinderSurface, EntityRef, FieldValue, FieldWrite, SphereSurface,
    Surface, SurfaceDomain, SurfaceGeometry, SurfaceIntersection, SurfaceSample,
    components::{Camera, FlatSurface, GuiSlider, Transform},
    systems::geometry::{GeometryRay, GeometryShape},
};
use std::sync::Arc;

fn scene(curved: ComponentValue) -> (Rig, Panel) {
    let (mut host, _) = host();
    let parent = scene_world(&mut host);
    let root = camera_root(&mut host, parent, Camera::default());
    let panel = panel(
        &mut host,
        parent,
        at(0.0, 0.0, 5.0),
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
            ..Default::default()
        }),
    );
    apply(
        &mut host,
        parent,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(panel.anchor),
                component: ComponentValue::FLAT_SURFACE,
            },
            Command::insert_value(EntityRef::Handle(panel.anchor), curved),
        ],
    );
    (Rig::new(host, root, viewport(800, 800)), panel)
}

fn components(k: f32, spacing: f32) -> [ComponentValue; 2] {
    [
        ComponentValue::CylinderSurface(CylinderSurface {
            width: 4.0,
            height: 3.0,
            curvature: k,
            layer_spacing: spacing,
        }),
        ComponentValue::SphereSurface(SphereSurface {
            width: 4.0,
            height: 3.0,
            curvature: k,
            layer_spacing: spacing,
        }),
    ]
}

fn independent_pixel(sphere: bool, k: f64, content: [f64; 2], offset: f64) -> [f32; 2] {
    let x = content[0] - 2.0;
    let y = 1.5 - content[1];
    let r = if sphere {
        x.hypot(y)
    } else {
        x
    };
    let angle = k * r;
    let sinc = if angle == 0.0 {
        1.0
    } else {
        angle.sin() / angle
    };
    let position = [
        x * sinc + offset * k * x * sinc,
        if sphere {
            y * sinc + offset * k * y * sinc
        } else {
            y
        },
        if k == 0.0 {
            offset
        } else {
            (angle.cos() - 1.0) / k + offset * angle.cos()
        },
    ];
    let perspective = (5.0 - position[2]) * (std::f64::consts::PI / 8.0).tan();
    [
        (0.5 + 0.5 * position[0] / perspective) as f32,
        (0.5 - 0.5 * position[1] / perspective) as f32,
    ]
}

#[test]
fn curved_composed_hits_and_projection_invert_independent_camera_rays() {
    for k in [-0.5, 0.5] {
        for (index, component) in components(k, 0.0).into_iter().enumerate() {
            let (rig, panel) = scene(component);
            let content: [f64; 2] = [3.0, 1.0];
            let pixel = independent_pixel(index == 1, f64::from(k), content, 0.0);
            let result =
                query_composed_input(&rig.host, rig.query(), pixel, GuiQueryOptions::default())
                    .unwrap();
            let GuiQueryOutcome::Hit(hit) = result.outcome else {
                panic!("missing curved hit");
            };
            assert_eq!(hit.output, panel.output);
            for (axis, expected) in content.into_iter().enumerate() {
                assert!((f64::from(hit.point[axis]) - expected).abs() < 1e-5);
            }
            let path: Vec<_> = hit.path.iter().map(|step| step.token.clone()).collect();
            let projected =
                project_composed_point(&rig.host, rig.query(), &path, pixel, false, hit.hit.layer)
                    .unwrap()
                    .unwrap();
            for (axis, expected) in content.into_iter().enumerate() {
                assert!((f64::from(projected.point[axis]) - expected).abs() < 1e-5);
            }
            rig.finish();
        }
    }
}

#[test]
fn curved_capture_continues_outside_content_and_keeps_value_on_shell_miss() {
    for k in [-0.5, 0.5] {
        for (index, component) in components(k, 0.2).into_iter().enumerate() {
            let (mut rig, panel) = scene(component);
            // A structural ordinary root occupies rank0; raised control occupies rank1.
            let parent = panel.world();
            create(&mut rig.host, parent, Vec::new(), None);
            apply(
                &mut rig.host,
                parent,
                vec![Command::insert_value(
                    EntityRef::Handle(panel.controls[0]),
                    ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                        layer: 100,
                        ..Default::default()
                    }),
                )],
            );
            rig.send(press(71, [0.5, 0.5]));
            let before = rig.value(parent, panel.controls[0]);
            rig.send(movement(71, [4.0, 0.5]));
            assert!(rig.snapshot(parent, panel.controls[0]).interaction.captured);
            assert_eq!(
                rig.value(parent, panel.controls[0]),
                before,
                "shell miss must not synthesize movement"
            );
            let outside = independent_pixel(index == 1, f64::from(k), [4.5, 1.5], 0.2_f32.into());
            rig.send(movement(71, outside));
            assert_eq!(
                rig.value(parent, panel.controls[0]),
                GuiTestValue::Scalar(1.0)
            );
            assert!(rig.snapshot(parent, panel.controls[0]).interaction.captured);
            rig.finish();
        }
    }
}

#[test]
fn provider_conflicts_and_kind_replacement_preserve_ordered_mutation_and_fences() {
    let (mut rig, panel) = scene(components(0.5, 0.0)[0].clone());
    let parent = rig.root.world();
    let conflict = batch(
        &mut rig.host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(panel.anchor),
            components(0.5, 0.0)[1].clone(),
        )],
    );
    assert_eq!(
        conflict.result.unwrap_err().reason,
        crate::ErrorReason::InvalidValue
    );
    assert!(
        rig.host
            .world_mut(parent.id())
            .unwrap()
            .surface(panel.anchor)
            .is_some()
    );
    rig.send(press(81, [0.5, 0.5]));
    apply(
        &mut rig.host,
        parent,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(panel.anchor),
                component: ComponentValue::CYLINDER_SURFACE,
            },
            Command::insert_value(
                EntityRef::Handle(panel.anchor),
                components(0.5, 0.0)[1].clone(),
            ),
        ],
    );
    assert_eq!(
        rig.route(movement(81, [0.7, 0.5])),
        Err(GuiInputError::StalePath)
    );
    rig.finish();
}

#[derive(Clone, Debug, PartialEq)]
struct ShiftedSurface(FlatSurface, bool, bool);

impl Surface for ShiftedSurface {
    fn physical_extent(&self) -> [f64; 2] {
        self.0.physical_extent()
    }

    fn layer_spacing(&self) -> f32 {
        self.0.layer_spacing
    }

    fn sample(&self, content: [f64; 2], offset: f64) -> Result<SurfaceSample, crate::ErrorReason> {
        let mut sample = self.0.sample(content, offset)?;
        sample.position[0] += 1.25;
        sample.position[2] += 0.7;
        Ok(sample)
    }

    fn ray_intersections(
        &self,
        ray: &GeometryRay,
        offset: f64,
        domain: SurfaceDomain,
    ) -> Result<Vec<SurfaceIntersection>, crate::ErrorReason> {
        let mut ray = *ray;
        ray.origin[0] -= 1.25;
        ray.origin[2] -= 0.7;
        let mut hits = self.0.ray_intersections(&ray, offset, domain)?;
        if self.1 && !hits.is_empty() {
            // Synthetic two-root chart fixture: the nearer root enters empty
            // canvas content; the farther root enters its checkbox row.
            let mut farther = hits[0];
            farther.distance += 0.25;
            farther.content = [2.0, 0.5];
            hits[0].content = [2.0, 2.5];
            hits.push(farther);
        }
        Ok(hits)
    }

    fn validate_offsets(&self, offsets: [f64; 2]) -> Result<(), crate::ErrorReason> {
        self.0.validate_offsets(offsets)
    }

    fn bounds(&self, offsets: [f64; 2]) -> Result<GeometryShape, crate::ErrorReason> {
        let GeometryShape::Box {
            mut min,
            mut max,
        } = self.0.bounds(offsets)?
        else {
            unreachable!()
        };
        min[0] += 1.25;
        max[0] += 1.25;
        min[2] += 0.7;
        max[2] += 0.7;
        Ok(GeometryShape::Box {
            min,
            max,
        })
    }

    fn approximation_error(&self, patch: [f64; 4], offset: f64) -> Result<f64, crate::ErrorReason> {
        self.0.approximation_error(patch, offset)
    }

    fn exact_affine(&self, offset: f64) -> Option<[f64; 16]> {
        if self.1 {
            return None;
        }
        let mut affine = self.0.exact_affine(offset)?;
        affine[12] += 1.25;
        affine[14] += 0.7;
        if self.2 {
            affine[8..11].fill(0.0);
        }
        Some(affine)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn equivalent(&self, other: &dyn Surface) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
}

struct FixtureSurfaceFactory(bool);

struct FixtureSurfaceSystem(bool);

impl crate::systems::SystemFactory for FixtureSurfaceFactory {
    fn id(&self) -> crate::systems::SystemId {
        crate::systems::SystemId("fixture.surface-publication")
    }

    fn dependencies(&self) -> &[crate::systems::SystemDependency] {
        &[crate::systems::SystemDependency::After(
            crate::systems::world_attachment::WorldAttachmentSystem::ID,
        )]
    }

    fn create(
        &self,
        _: &mut crate::systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn crate::systems::System>, crate::systems::SystemInitError> {
        Ok(Box::new(FixtureSurfaceSystem(self.0)))
    }
}

impl crate::systems::System for FixtureSurfaceSystem {
    fn update(&mut self, _: &mut crate::systems::SystemUpdateContext<'_, '_>) {}

    fn completed_attachments(
        &self,
        _: &crate::WorldContext<'_>,
        attachments: &mut Vec<crate::PublishedWorldAttachment>,
    ) {
        for edge in attachments {
            if let Some(geometry) = &edge.surface_geometry {
                let extent = geometry.physical_extent();
                edge.surface_geometry = Some(SurfaceGeometry::new(
                    geometry.component(),
                    ShiftedSurface(
                        FlatSurface {
                            width: extent[0] as f32,
                            height: extent[1] as f32,
                            ..Default::default()
                        },
                        self.0,
                        false,
                    ),
                ));
            }
        }
    }
}

#[test]
fn custom_trait_geometry_drives_generic_composed_hit_and_projection() {
    let mut factories = crate::systems::compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(Default::default())));
    factories.push(Arc::new(FixtureSurfaceFactory(false)));
    let mut host = crate::test_task_scheduler::with_factories(factories).unwrap();
    let mut systems = SCENE_SYSTEMS.to_vec();
    systems.push(crate::systems::SystemId("fixture.surface-publication"));
    let parent_id = host.create_world(Default::default(), &systems).unwrap();
    let parent = host.world_ref(parent_id).unwrap();
    let root = camera_root(&mut host, parent, Camera::default());
    let panel = panel(
        &mut host,
        parent,
        at(0.0, 0.0, 5.0),
        ComponentValue::GuiSlider(GuiSlider::default()),
    );
    let rig = Rig::new(host, root, viewport(800, 800));
    let result = query_composed_input(
        &rig.host,
        rig.query(),
        [0.5, 0.5],
        GuiQueryOptions::default(),
    )
    .unwrap();
    let GuiQueryOutcome::Hit(hit) = result.outcome else {
        panic!("custom Surface hit unavailable");
    };
    assert!((hit.point[0] - 0.75).abs() < 1e-6);
    let path: Vec<_> = hit.path.iter().map(|step| step.token.clone()).collect();
    let projected = project_composed_point(&rig.host, rig.query(), &path, [0.5, 0.5], false, 0)
        .unwrap()
        .unwrap();
    assert!((projected.point[0] - 0.75).abs() < 1e-6);
    assert_eq!(hit.output, panel.output);
    rig.finish();
}

#[test]
fn multiple_front_roots_visit_the_same_canvas_until_a_control_is_hit() {
    let mut factories = crate::systems::compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(Default::default())));
    factories.push(Arc::new(FixtureSurfaceFactory(true)));
    let mut host = crate::test_task_scheduler::with_factories(factories).unwrap();
    let mut systems = SCENE_SYSTEMS.to_vec();
    systems.push(crate::systems::SystemId("fixture.surface-publication"));
    let parent_id = host.create_world(Default::default(), &systems).unwrap();
    let parent = host.world_ref(parent_id).unwrap();
    let root = camera_root(&mut host, parent, Camera::default());
    let panel = column_panel(&mut host, parent, at(0.0, 0.0, 5.0), 1);
    let rig = Rig::new(host, root, viewport(800, 800));
    let result = query_composed_input(
        &rig.host,
        rig.query(),
        [0.5, 0.5],
        GuiQueryOptions::default(),
    )
    .unwrap();
    let GuiQueryOutcome::Hit(hit) = result.outcome else {
        panic!("farther chart root was rejected");
    };
    assert_eq!(hit.hit.target.entity, panel.controls[0]);
    assert_eq!(hit.point, [2.0, 0.5]);
    rig.finish();
}

#[test]
fn keyboard_distance_uses_transformed_custom_affine_content_mapping() {
    use super::keyboard_panels::GuiKeyboardView;
    use crate::systems::geometry::GeometryShapeTransform;

    for zero_third_column in [false, true] {
        let geometry = ShiftedSurface(
            FlatSurface {
                width: 4.0,
                height: 3.0,
                ..Default::default()
            },
            false,
            zero_third_column,
        );
        let transform = GeometryShapeTransform::new([
            2.0, 0.0, 0.0, 0.0, 0.5, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, -2.5, 0.0, -2.8, 1.0,
        ])
        .unwrap();
        for orthographic in [false, true] {
            let view = GuiKeyboardView::from_pose([0.0, 0.0, 5.0], [0.0, 0.0, -1.0], orthographic)
                .unwrap();
            let (front, distance) = view.placement(&transform, &geometry).unwrap();
            assert!(front);
            assert!(
                (distance - 5.0).abs() < 1e-9,
                "custom affine distance: {distance}"
            );
        }
    }
}

#[test]
fn keyboard_orders_actual_curved_segments_when_conservative_bounds_overlap() {
    use super::keyboard_panels::GuiKeyboardView;
    use crate::systems::geometry::GeometryShapeTransform;

    // The eye lies inside the cylinder's loose segment enclosure, but remains
    // far from the segment itself. The small flat panel lies inside that same
    // enclosure and is actually closer, with both centre normals facing the eye.
    let curve = CylinderSurface {
        width: 5.0,
        height: 1.0,
        curvature: -1.0,
        ..Default::default()
    };
    let GeometryShape::Box {
        min,
        max,
    } = curve.bounds([0.0, 0.0]).unwrap()
    else {
        unreachable!()
    };
    let eye = [0.0, 0.0, 1.8];
    assert!((0..3).all(|axis| min[axis] <= eye[axis] && eye[axis] <= max[axis]));
    let flat = FlatSurface {
        width: 0.2,
        height: 0.2,
        ..Default::default()
    };
    let shifted = GeometryShapeTransform::new([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.7, 1.0,
    ])
    .unwrap();
    let view = GuiKeyboardView::from_pose(eye, [0.0, 0.0, -1.0], false).unwrap();
    let (curve_front, curve_distance) = view
        .placement(&GeometryShapeTransform::default(), &curve)
        .unwrap();
    let (flat_front, flat_distance) = view.placement(&shifted, &flat).unwrap();
    assert!(curve_front && flat_front);
    let endpoint_distance = 2.5_f64.sin().hypot(1.0 - 2.5_f64.cos() - eye[2]);
    assert!(
        (curve_distance - endpoint_distance).abs() < 0.01,
        "{curve_distance} versus {endpoint_distance}"
    );
    assert!((flat_distance - 0.1).abs() < 1e-9);
    assert!(flat_distance < curve_distance);
}

#[test]
fn one_collapsed_occupied_shell_makes_the_whole_query_unavailable() {
    for (k, spacing) in [(0.5, -2.0), (-0.5, 2.0)] {
        for component in components(k, spacing) {
            let (mut rig, panel) = scene(component);
            create(&mut rig.host, panel.world(), Vec::new(), None);
            apply(
                &mut rig.host,
                panel.world(),
                vec![Command::insert_value(
                    EntityRef::Handle(panel.controls[0]),
                    ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                        layer: 100,
                        ..Default::default()
                    }),
                )],
            );
            rig.frame();
            let result = query_composed_input(
                &rig.host,
                rig.query(),
                [0.5, 0.5],
                GuiQueryOptions::default(),
            )
            .unwrap();
            assert!(matches!(
                result.outcome,
                GuiQueryOutcome::Unavailable(
                    crate::services::gui_input::query::GuiQueryUnavailable::Data(
                        crate::ErrorReason::InvalidGeometry
                    )
                )
            ));
            rig.finish();
        }
    }
}

#[test]
fn curved_queries_follow_rotated_nonuniform_parent_placements() {
    for k in [-0.5, 0.5] {
        for (index, component) in components(k, 0.0).into_iter().enumerate() {
            let (mut rig, panel) = scene(component);
            let angle = 0.4_f32;
            apply(
                &mut rig.host,
                rig.root.world(),
                vec![Command::insert_value(
                    EntityRef::Handle(panel.anchor),
                    ComponentValue::Transform(Transform {
                        x: 0.2,
                        y: -0.1,
                        z: 5.0,
                        qy: (angle * 0.5).sin(),
                        qw: (angle * 0.5).cos(),
                        sx: 1.4,
                        sy: 0.8,
                        sz: 1.2,
                        ..Default::default()
                    }),
                )],
            );
            rig.frame();
            let content: [f64; 2] = [3.0, 1.0];
            let x = content[0] - 2.0;
            let y = 1.5 - content[1];
            let arc = f64::from(k)
                * if index == 1 {
                    x.hypot(y)
                } else {
                    x
                };
            let sinc = arc.sin() / arc;
            let p = [
                x * sinc * 1.4_f32 as f64,
                if index == 1 {
                    y * sinc * 0.8_f32 as f64
                } else {
                    y * 0.8_f32 as f64
                },
                (arc.cos() - 1.0) / f64::from(k) * 1.2_f32 as f64,
            ];
            let theta = f64::from(angle);
            let world = [
                0.2_f32 as f64 + theta.cos() * p[0] + theta.sin() * p[2],
                -0.1_f32 as f64 + p[1],
                5.0 - theta.sin() * p[0] + theta.cos() * p[2],
            ];
            let denominator = (10.0 - world[2]) * (std::f64::consts::PI / 8.0).tan();
            let pixel = [
                (0.5 + 0.5 * world[0] / denominator) as f32,
                (0.5 - 0.5 * world[1] / denominator) as f32,
            ];
            let result =
                query_composed_input(&rig.host, rig.query(), pixel, GuiQueryOptions::default())
                    .unwrap();
            let GuiQueryOutcome::Hit(hit) = result.outcome else {
                panic!("transformed curve missing");
            };
            for (actual, expected) in hit.point.into_iter().zip(content) {
                assert!((f64::from(actual) - expected).abs() < 1e-5);
            }
            rig.finish();
        }
    }
}

#[test]
fn captured_curved_input_refreshes_target_plane_after_transition_reorders_ids() {
    use crate::components::{CanvasLayerTransition, CanvasStyle};
    for (index, component) in components(0.5, 0.2).into_iter().enumerate() {
        let (mut rig, panel) = scene(component);
        let world = panel.world();
        create(&mut rig.host, world, Vec::new(), None);
        create(
            &mut rig.host,
            world,
            vec![ComponentValue::CanvasStyle(CanvasStyle {
                layer: 20,
                ..Default::default()
            })],
            None,
        );
        apply(
            &mut rig.host,
            world,
            vec![
                Command::insert_value(
                    EntityRef::Handle(panel.controls[0]),
                    ComponentValue::CanvasStyle(CanvasStyle {
                        layer: 30,
                        ..Default::default()
                    }),
                ),
                Command::insert_value(
                    EntityRef::Handle(panel.controls[0]),
                    ComponentValue::CanvasLayerTransition(CanvasLayerTransition {
                        previous_layer: 10,
                        progress: 0.0,
                    }),
                ),
            ],
        );
        rig.frame();
        let pixel = independent_pixel(index == 1, 0.5, [2.0, 1.5], f64::from(0.2_f32));
        let hit = query_composed_input(&rig.host, rig.query(), pixel, GuiQueryOptions::default())
            .unwrap();
        let GuiQueryOutcome::Hit(hit) = hit.outcome else {
            panic!("missing initial moving hit")
        };
        assert_eq!(hit.hit.layer, 1);
        let initial_target = hit.hit.target;
        rig.send(press(73, pixel));
        apply(
            &mut rig.host,
            world,
            vec![Command::SetField {
                entity: EntityRef::Handle(panel.controls[0]),
                component: ComponentValue::CANVAS_LAYER_TRANSITION,
                field: FieldWrite {
                    offset: std::mem::offset_of!(CanvasLayerTransition, progress) as u32,
                    value: FieldValue::F32(1.0),
                },
            }],
        );
        rig.frame();
        let publication = rig.host.latest_publication(world.id()).unwrap();
        let semantic = rig
            .host
            .publication(publication)
            .unwrap()
            .chunk(crate::systems::canvas::CanvasSystem::ID)
            .unwrap()
            .data::<crate::systems::gui::presentation::GuiCanvasPublication>()
            .unwrap();
        let slider = semantic.views[&panel.output]
            .controls
            .iter()
            .find(|control| control.record.target.entity == panel.controls[0])
            .unwrap()
            .slider
            .unwrap();
        let x = f64::from(
            slider.thumb_centers[0] + 0.75 * (slider.thumb_centers[1] - slider.thumb_centers[0]),
        );
        let pixel = independent_pixel(index == 1, 0.5, [x, 1.5], 3.0 * f64::from(0.2_f32));
        let hit = query_composed_input(&rig.host, rig.query(), pixel, GuiQueryOptions::default())
            .unwrap();
        let GuiQueryOutcome::Hit(hit) = hit.outcome else {
            panic!("missing completed moving hit")
        };
        assert_eq!(hit.hit.layer, 2);
        assert_eq!(hit.hit.target, initial_target);
        rig.send(movement(73, pixel));
        let GuiTestValue::Scalar(value) = rig.value(world, panel.controls[0]) else {
            panic!("missing slider value")
        };
        assert!((value - 0.75).abs() < 1e-5, "{value}");
        assert!(rig.snapshot(world, panel.controls[0]).interaction.captured);
        rig.finish();
    }
}

#[test]
fn zero_spacing_surface_queries_preserve_destination_order_when_plane_ids_reverse_it() {
    use crate::components::{CanvasLayerTransition, CanvasStyle};
    let (mut rig, panel) = scene(components(0.5, 0.0)[0].clone());
    let world = panel.world();
    create(&mut rig.host, world, Vec::new(), None);
    apply(
        &mut rig.host,
        world,
        vec![
            Command::insert_value(
                EntityRef::Handle(panel.controls[0]),
                ComponentValue::CanvasStyle(CanvasStyle {
                    layer: 10,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(panel.controls[0]),
                ComponentValue::CanvasLayerTransition(CanvasLayerTransition {
                    previous_layer: 30,
                    progress: 0.0,
                }),
            ),
        ],
    );
    let other = create(
        &mut rig.host,
        world,
        vec![
            sized(2, 4.0, 3.0),
            ComponentValue::GuiSlider(GuiSlider::default()),
            ComponentValue::CanvasStyle(CanvasStyle {
                layer: 20,
                ..Default::default()
            }),
        ],
        None,
    );
    rig.frame();
    let canvas = rig
        .host
        .output(
            rig.host.latest_publication(world.id()).unwrap(),
            panel.output,
        )
        .unwrap()
        .data::<crate::systems::canvas::CanvasPublication>()
        .unwrap();
    let first = canvas
        .hits
        .iter()
        .find(|hit| hit.target.entity == panel.controls[0])
        .unwrap();
    let second = canvas
        .hits
        .iter()
        .find(|hit| hit.target.entity == other)
        .unwrap();
    assert!(first.layer > second.layer);
    assert!(first.priority < second.priority);
    let pixel = independent_pixel(false, 0.5, [2.0, 1.5], 0.0);
    let result =
        query_composed_input(&rig.host, rig.query(), pixel, GuiQueryOptions::default()).unwrap();
    let GuiQueryOutcome::Hit(hit) = result.outcome else {
        panic!("missing coincident hit")
    };
    assert_eq!(hit.hit.target.entity, other);
    rig.finish();
}

#[test]
fn captured_nested_input_refreshes_moving_slot_plane_from_current_publication() {
    use crate::components::{CanvasLayerTransition, CanvasStyle};
    let (mut rig, outer) = scene(components(0.5, 0.2)[0].clone());
    let inner = panel(
        &mut rig.host,
        outer.world(),
        at(0.0, 0.0, 0.0),
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.5,
            ..Default::default()
        }),
    );
    create(&mut rig.host, outer.world(), Vec::new(), None);
    create(
        &mut rig.host,
        outer.world(),
        vec![ComponentValue::CanvasStyle(CanvasStyle {
            layer: 20,
            ..Default::default()
        })],
        None,
    );
    apply(
        &mut rig.host,
        outer.world(),
        vec![
            Command::insert_value(
                EntityRef::Handle(inner.anchor),
                ComponentValue::CanvasStyle(CanvasStyle {
                    layer: 30,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(inner.anchor),
                ComponentValue::CanvasLayerTransition(CanvasLayerTransition {
                    previous_layer: 10,
                    progress: 0.0,
                }),
            ),
        ],
    );
    rig.frame();
    let initial = independent_pixel(false, 0.5, [2.0, 1.5], f64::from(0.2_f32));
    let result =
        query_composed_input(&rig.host, rig.query(), initial, GuiQueryOptions::default()).unwrap();
    let GuiQueryOutcome::Hit(hit) = result.outcome else {
        panic!("missing nested hit")
    };
    assert_eq!(hit.output, inner.output);
    assert_eq!(hit.path.len(), 2);
    let path: Vec<_> = hit.path.iter().map(|step| step.token.clone()).collect();
    let identity = hit.hit.target;
    rig.send(press(74, initial));
    apply(
        &mut rig.host,
        outer.world(),
        vec![Command::SetField {
            entity: EntityRef::Handle(inner.anchor),
            component: ComponentValue::CANVAS_LAYER_TRANSITION,
            field: FieldWrite {
                offset: std::mem::offset_of!(CanvasLayerTransition, progress) as u32,
                value: FieldValue::F32(1.0),
            },
        }],
    );
    rig.frame();
    let publication = rig.host.latest_publication(inner.world().id()).unwrap();
    let semantic = rig
        .host
        .publication(publication)
        .unwrap()
        .chunk(crate::systems::canvas::CanvasSystem::ID)
        .unwrap()
        .data::<crate::systems::gui::presentation::GuiCanvasPublication>()
        .unwrap();
    let slider = semantic.views[&inner.output]
        .controls
        .iter()
        .find(|control| control.record.target.entity == inner.controls[0])
        .unwrap()
        .slider
        .unwrap();
    let x = f64::from(
        slider.thumb_centers[0] + 0.75 * (slider.thumb_centers[1] - slider.thumb_centers[0]),
    );
    let moved = independent_pixel(false, 0.5, [x, 1.5], 3.0 * f64::from(0.2_f32));
    let projection = project_composed_point(&rig.host, rig.query(), &path, moved, true, 0)
        .unwrap()
        .unwrap();
    assert!((f64::from(projection.point[0]) - x).abs() < 1e-5);
    rig.send(movement(74, moved));
    let GuiTestValue::Scalar(value) = rig.value(inner.world(), inner.controls[0]) else {
        panic!("missing nested slider")
    };
    assert!((value - 0.75).abs() < 1e-5, "{value}");
    assert_eq!(
        rig.snapshot(inner.world(), inner.controls[0]).target.entity,
        identity.entity
    );
    assert!(
        rig.snapshot(inner.world(), inner.controls[0])
            .interaction
            .captured
    );
    rig.finish();
}
