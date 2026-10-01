//! Public root presentation authority and immediate nested-target retirement.

mod support;

use ipp_core::{
    ComponentValue, HostRuntime, OutputKind, OutputRef, WorldId, WorldViewport,
    components::{Camera, Transform},
};
use ipp_render_gl::{RenderError, RenderService};
use std::rc::Rc;
#[cfg(feature = "surfaces")]
use support::selection::{ATTACHMENTS, CANVAS, SURFACE};
use support::selection::{CAMERA, RENDER, select};
use support::{DeviceState, TestDevice, create};

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 100,
        height: 100,
        device_pixel_ratio: 1.0,
    }
}

fn camera(host: &mut HostRuntime, world: WorldId) -> OutputRef {
    let entity = create(
        &mut host.world_mut(world).unwrap(),
        vec![
            ComponentValue::Camera(Camera::default()),
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
        ],
    );
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn frame(host: &mut HostRuntime, renderer: &mut RenderService<TestDevice>, world: WorldId) {
    let report = host.frame(0.0).unwrap();
    assert!(report.worlds.values().all(Result::is_ok));
    assert!(report.publication_errors.is_empty());
    renderer
        .prepare(
            host,
            host.root_output(world)
                .map(|(output, _, publication)| (output, publication)),
        )
        .unwrap();
    host.progress_assets();
}

#[test]
fn root_draw_rejects_cleared_reselected_and_superseded_publications() {
    let mut host = HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let world = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let first = camera(&mut host, world);
    let second = camera(&mut host, world);
    host.set_root_output(first, viewport()).unwrap();
    frame(&mut host, &mut renderer, world);
    let publication = host.root_output(world).unwrap().2;
    renderer
        .draw(&host, first, publication, viewport(), 0.0)
        .unwrap();

    host.clear_root_output(world);
    assert!(host.output(publication, first).is_some());
    let ended = state.ended_frames.get();
    assert_eq!(
        renderer.draw(&host, first, publication, viewport(), 0.0),
        Err(RenderError::UnavailableOutput)
    );
    assert_eq!(state.ended_frames.get(), ended + 1);

    host.set_root_output(second, viewport()).unwrap();
    assert!(host.output(publication, first).is_some());
    assert_eq!(
        renderer.draw(&host, first, publication, viewport(), 0.0),
        Err(RenderError::UnavailableOutput)
    );
    renderer
        .draw(&host, second, publication, viewport(), 0.0)
        .unwrap();

    frame(&mut host, &mut renderer, world);
    let current = host.root_output(world).unwrap().2;
    assert_ne!(publication, current);
    assert_eq!(
        renderer.draw(&host, second, publication, viewport(), 0.0),
        Err(RenderError::UnavailableOutput)
    );
    renderer
        .draw(&host, second, current, viewport(), 0.0)
        .unwrap();
}

#[cfg(feature = "surfaces")]
fn attach(host: &mut HostRuntime, parent: WorldId, output: OutputRef) -> ipp_core::EntityId {
    create(
        &mut host.world_mut(parent).unwrap(),
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(Default::default()),
            ComponentValue::WorldAttachment(ipp_core::WorldAttachment::surface(output)),
        ],
    )
}

#[test]
fn root_draw_rejects_viewport_dimensions_and_pixel_ratio_without_reselecting() {
    let mut host = HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let world = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let selection = camera(&mut host, world);
    host.set_root_output(selection, viewport()).unwrap();
    frame(&mut host, &mut renderer, world);
    let root = host.root_output(world).unwrap();
    let publication = root.2;
    let ended = state.ended_frames.get();

    for mismatch in [
        WorldViewport {
            width: 101,
            ..viewport()
        },
        WorldViewport {
            height: 101,
            ..viewport()
        },
        WorldViewport {
            device_pixel_ratio: 2.0,
            ..viewport()
        },
    ] {
        assert_eq!(
            renderer.draw(&host, selection, publication, mismatch, 0.0),
            Err(RenderError::InvalidViewport)
        );
        assert_eq!(host.root_output(world), Some(root));
        assert_eq!(state.ended_frames.get(), ended);
        #[cfg(feature = "surfaces")]
        assert_eq!(state.cache_creates.get(), 0);
    }
    renderer
        .draw(&host, selection, publication, viewport(), 0.0)
        .unwrap();

    let resized = WorldViewport {
        width: 150,
        height: 120,
        device_pixel_ratio: 1.5,
    };
    host.set_root_output(selection, resized).unwrap();
    frame(&mut host, &mut renderer, world);
    let publication = host.root_output(world).unwrap().2;
    assert_eq!(
        renderer.draw(&host, selection, publication, viewport(), 0.0),
        Err(RenderError::InvalidViewport)
    );
    renderer
        .draw(&host, selection, publication, resized, 0.0)
        .unwrap();
}

#[cfg(feature = "surfaces")]
#[test]
fn nested_output_is_not_an_independent_root_before_or_after_detach() {
    let mut host = HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    state.cache_limit.set(256);
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let root = camera(&mut host, parent);
    let nested = camera(&mut host, child);
    host.set_root_output(root, viewport()).unwrap();
    let anchor = attach(&mut host, parent, nested);
    frame(&mut host, &mut renderer, parent);
    let publication = host.latest_publication(child).unwrap();
    assert!(host.output(publication, nested).is_some());
    assert_eq!(
        renderer.draw(&host, nested, publication, viewport(), 0.0),
        Err(RenderError::UnavailableOutput)
    );
    assert_eq!(state.cache_creates.get(), 0);
    let root_publication = host.root_output(parent).unwrap().2;
    let summary = renderer
        .draw(&host, root, root_publication, viewport(), 0.0)
        .unwrap();
    assert_eq!((summary.draw_calls, summary.failed_draw_calls), (1, 0));
    assert_eq!(state.cache_targets_live.get(), 1);

    host.world_mut(parent)
        .unwrap()
        .enqueue(ipp_core::Batch {
            id: 99,
            operations: vec![ipp_core::Command::Delete {
                entity: ipp_core::EntityRef::Handle(anchor),
            }],
        })
        .unwrap();
    frame(&mut host, &mut renderer, parent);
    let detached = host.latest_publication(child).unwrap();
    assert!(host.output(detached, nested).is_some());
    assert!(host.root_output(child).is_none());
    assert_eq!(
        renderer.draw(&host, nested, detached, viewport(), 0.0),
        Err(RenderError::UnavailableOutput)
    );
    host.set_root_output(nested, viewport()).unwrap();
    renderer
        .draw(&host, nested, detached, viewport(), 0.0)
        .unwrap();
}

#[cfg(feature = "surfaces")]
#[test]
fn forgetting_world_releases_only_its_camera_target_without_prepare() {
    let mut host = HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    state.cache_limit.set(256);
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
        )
        .unwrap();
    let grandchild = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let root = camera(&mut host, parent);
    let middle = camera(&mut host, child);
    let inner = camera(&mut host, grandchild);
    host.set_root_output(root, viewport()).unwrap();
    attach(&mut host, parent, middle);
    attach(&mut host, child, inner);
    frame(&mut host, &mut renderer, parent);
    let publication = host.root_output(parent).unwrap().2;
    renderer
        .draw(&host, root, publication, viewport(), 0.0)
        .unwrap();
    assert_eq!(
        (state.cache_creates.get(), state.cache_targets_live.get()),
        (2, 2)
    );

    assert!(host.destroy_world(child));
    renderer.forget_world(child);
    assert_eq!(
        (state.cache_deletes.get(), state.cache_targets_live.get()),
        (1, 1)
    );
    renderer.forget_world(child);
    assert_eq!(
        (state.cache_deletes.get(), state.cache_targets_live.get()),
        (1, 1)
    );
    assert!(host.destroy_world(grandchild));
    renderer.forget_world(grandchild);
    assert_eq!(
        (state.cache_deletes.get(), state.cache_targets_live.get()),
        (2, 0)
    );
    drop(renderer);
    assert_eq!(state.cache_deletes.get(), 2);
}

#[cfg(feature = "surfaces")]
#[test]
fn scoped_surface_is_presented_only_inside_its_owning_output() {
    assert_scoped_camera_branch(false);
}

#[cfg(feature = "surfaces")]
#[test]
fn scoped_spatial_branch_keeps_nested_camera_only_inside_its_owning_output() {
    assert_scoped_camera_branch(true);
}

#[cfg(feature = "surfaces")]
fn assert_scoped_camera_branch(spatial: bool) {
    use ipp_core::systems::{
        System, SystemFactory, SystemId, SystemInitContext, SystemInitError,
        compiled_system_factories,
    };
    use std::sync::{Arc, Mutex};

    struct Placement(Arc<Mutex<Option<(ipp_core::EntityId, OutputRef)>>>);

    impl SystemFactory for Placement {
        fn id(&self) -> SystemId {
            SystemId("fixture.render-placement")
        }

        fn create(
            &self,
            _: &mut SystemInitContext<'_>,
        ) -> Result<Box<dyn System>, SystemInitError> {
            Ok(Box::new(Self(self.0.clone())))
        }
    }

    impl System for Placement {
        fn update(&mut self, _: &mut ipp_core::systems::SystemUpdateContext<'_, '_>) {}

        fn attachment_placement(
            &self,
            world: &ipp_core::WorldContext<'_>,
            anchor: ipp_core::EntityId,
        ) -> ipp_core::AttachmentPlacement {
            self.0
                .lock()
                .unwrap()
                .filter(|(entity, owner)| *entity == anchor && owner.world().id() == world.id())
                .map_or(ipp_core::AttachmentPlacement::Unmanaged, |(_, owner)| {
                    ipp_core::AttachmentPlacement::Ready {
                        owner,
                        affine: ipp_core::systems::geometry::GeometryShapeTransform::default()
                            .matrix(),
                    }
                })
        }
    }

    let placement = Arc::new(Mutex::new(None));
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(Placement(placement.clone())));
    let mut host = HostRuntime::with_system_factories(factories).unwrap();
    let state = Rc::new(DeviceState::default());
    state.cache_limit.set(256);
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &[
                select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
                vec![SystemId("fixture.render-placement")],
            ]
            .concat(),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let unrelated = camera(&mut host, parent);
    let owner = camera(&mut host, parent);
    let nested = camera(&mut host, child);
    let anchor = if spatial {
        let branch = host
            .create_world(
                Default::default(),
                &[
                    select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
                    vec![SystemId("fixture.render-placement")],
                ]
                .concat(),
            )
            .unwrap();
        attach(&mut host, branch, nested);
        let attachment = ipp_core::WorldAttachment::spatial(host.world_ref(branch).unwrap());
        create(
            &mut host.world_mut(parent).unwrap(),
            vec![
                ComponentValue::Transform(Transform::default()),
                ComponentValue::WorldAttachment(attachment),
            ],
        )
    } else {
        attach(&mut host, parent, nested)
    };
    *placement.lock().unwrap() = Some((anchor, owner));
    host.set_root_output(unrelated, viewport()).unwrap();
    frame(&mut host, &mut renderer, parent);
    let publication = host.root_output(parent).unwrap().2;
    let edge = &host.publication(publication).unwrap().attachments[0];
    assert_eq!(edge.placement_output, Some(owner));
    assert!(host.attached_publication(edge).is_some());
    let mut included = [ipp_core::OutputPublicationObservation {
        output: nested,
        publication: None,
    }];
    assert_eq!(
        renderer
            .draw_observed(
                &host,
                unrelated,
                publication,
                viewport(),
                0.0,
                &mut included
            )
            .unwrap()
            .draw_calls,
        0
    );
    assert!(included[0].publication.is_none());
    assert_eq!(state.cache_creates.get(), 0);

    host.set_root_output(owner, viewport()).unwrap();
    frame(&mut host, &mut renderer, parent);
    let publication = host.root_output(parent).unwrap().2;
    assert_eq!(
        renderer
            .draw_observed(&host, owner, publication, viewport(), 0.0, &mut included)
            .unwrap()
            .draw_calls,
        1
    );
    assert_eq!(included[0].publication, host.latest_publication(child));
    assert_eq!(state.cache_targets_live.get(), 1);

    frame(&mut host, &mut renderer, parent);
    assert_eq!(
        (state.cache_deletes.get(), state.cache_targets_live.get()),
        (0, 1)
    );
    let publication = host.root_output(parent).unwrap().2;
    assert_eq!(
        renderer
            .draw(&host, owner, publication, viewport(), 0.0)
            .unwrap()
            .draw_calls,
        1
    );
    assert_eq!(state.cache_creates.get(), 1);

    host.set_root_output(unrelated, viewport()).unwrap();
    frame(&mut host, &mut renderer, parent);
    assert_eq!(
        (state.cache_deletes.get(), state.cache_targets_live.get()),
        (1, 0)
    );
    let publication = host.root_output(parent).unwrap().2;
    assert_eq!(
        renderer
            .draw(&host, unrelated, publication, viewport(), 0.0)
            .unwrap()
            .draw_calls,
        0
    );
    assert_eq!(state.cache_creates.get(), 1);
}

#[cfg(feature = "surfaces")]
#[test]
fn deep_canvas_chain_draws_on_a_bounded_native_stack() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            use ipp_core::components::CanvasBox;
            let mut host = HostRuntime::new();
            let state = Rc::new(DeviceState::default());
            let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
            renderer.install(&mut host).unwrap();
            let mut outputs = Vec::new();
            for _ in 0..1024 {
                let mut options =
                    ipp_core::WorldCreateOptions::new(select(&[ATTACHMENTS, CANVAS, SURFACE]));
                options.canvas = Some(ipp_core::CanvasState {
                    extent: [100.0, 100.0],
                    units_per_metre: 100.0,
                });
                let world = host
                    .create_world_with_options(Default::default(), options)
                    .unwrap();
                outputs.push(OutputRef::canvas(host.world_ref(world).unwrap()));
            }
            for pair in outputs.windows(2) {
                attach(&mut host, pair[0].world().id(), pair[1]);
            }
            let leaf = *outputs.last().unwrap();
            create(
                &mut host.world_mut(leaf.world().id()).unwrap(),
                vec![ComponentValue::CanvasBox(CanvasBox {
                    width: 100.0,
                    height: 100.0,
                    ..Default::default()
                })],
            );
            host.set_root_output(outputs[0], viewport()).unwrap();
            frame(&mut host, &mut renderer, outputs[0].world().id());
            let publication = host.root_output(outputs[0].world().id()).unwrap().2;
            let result = renderer
                .draw(&host, outputs[0], publication, viewport(), 0.0)
                .unwrap();
            assert_eq!(result.failed_draw_calls, 0);
            assert_eq!(state.gui_batch_draws.get(), 1);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(feature = "surfaces")]
#[test]
fn forgotten_descendant_releases_canvas_image_without_another_prepare() {
    use ipp_core::{Batch, Command, EntityRef, components::SurfaceCache};

    let mut host = HostRuntime::new();
    let state = Rc::new(DeviceState::default());
    state.cache_limit.set(256);
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, RENDER, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let grandchild = host
        .create_world(Default::default(), &select(&[CAMERA, RENDER]))
        .unwrap();
    let root = camera(&mut host, parent);
    let nested = OutputRef::canvas(host.world_ref(child).unwrap());
    let inner = camera(&mut host, grandchild);
    let anchor = attach(&mut host, parent, nested);
    attach(&mut host, child, inner);
    {
        let batch = Batch {
            id: 2,
            operations: vec![Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::SurfaceCache(SurfaceCache {
                    direct_distance: 0.0,
                    ..Default::default()
                }),
            )],
        };
        let outcome = support::apply_batch(&mut host.world_mut(parent).unwrap(), batch);
        assert!(outcome.result.is_ok());
    }
    host.set_root_output(root, viewport()).unwrap();
    frame(&mut host, &mut renderer, parent);
    let publication = host.root_output(parent).unwrap().2;
    let mut included = [root, nested, inner].map(|output| ipp_core::OutputPublicationObservation {
        output,
        publication: None,
    });
    renderer
        .draw_observed(&host, root, publication, viewport(), 0.0, &mut included)
        .unwrap();
    assert!(included.iter().all(|source| source.publication.is_some()));
    assert_eq!(state.cache_targets_live.get(), 2);
    assert!(host.destroy_world(grandchild));
    renderer.forget_world(grandchild);
    assert_eq!(state.cache_targets_live.get(), 0);
    assert_eq!(state.cache_deletes.get(), 2);
    renderer.forget_world(grandchild);
    assert_eq!(state.cache_deletes.get(), 2);
}
