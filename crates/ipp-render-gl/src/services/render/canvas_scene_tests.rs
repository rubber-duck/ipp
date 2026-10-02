use super::*;
use ipp_core::components::{Camera, CanvasBox, CanvasStyle, Surface};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, WorldAttachment,
    WorldId, WorldViewport,
};
use std::sync::Arc;

/// Camera Worlds over rendered content, presenting child Worlds on spatial
/// Surfaces; they do not select the Canvas System, so their Surfaces stay spatial.
const CAMERA_SYSTEMS: &[ipp_core::systems::SystemId] = &[
    ipp_core::systems::world_attachment::WorldAttachmentSystem::ID,
    ipp_core::systems::animation::AnimationSystem::ID,
    ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
    ipp_core::systems::hierarchy::HierarchySystem::ID,
    ipp_core::systems::look_at::LookAtSystem::ID,
    ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
    ipp_core::systems::geometry::GeometrySystem::ID,
    ipp_core::systems::camera::CameraSystem::ID,
    ipp_core::systems::surface::SurfaceSystem::ID,
    ipp_core::systems::render::RenderSystem::ID,
];

/// Canvas Worlds with GUI controls and layout that present child Worlds on
/// Surface slots.
const CANVAS_SYSTEMS: &[ipp_core::systems::SystemId] = &[
    ipp_core::systems::world_attachment::WorldAttachmentSystem::ID,
    ipp_core::systems::animation::AnimationSystem::ID,
    ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
    ipp_core::systems::hierarchy::HierarchySystem::ID,
    ipp_core::systems::look_at::LookAtSystem::ID,
    ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
    ipp_core::systems::geometry::GeometrySystem::ID,
    ipp_core::systems::surface::SurfaceSystem::ID,
    ipp_core::systems::canvas::CanvasSystem::ID,
    ipp_core::systems::gui::GuiSystem::ID,
    ipp_core::systems::gui::GuiLayoutSystem::ID,
];

fn apply_outcome(host: &mut HostRuntime, world: WorldId, batch: Batch) -> ipp_core::BatchOutcome {
    host.world_mut(world).unwrap().enqueue(batch).unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<(u32, EntityId)> {
    apply_outcome(
        host,
        world,
        Batch {
            id: 1,
            operations,
        },
    )
    .result
    .unwrap()
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    parent: Option<EntityId>,
    values: Vec<ComponentValue>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(0),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations)[0].1
}

/// A new canvas World with a 300 x 200 extent at 100 units per metre.
fn canvas(host: &mut HostRuntime) -> OutputRef {
    let mut options = ipp_core::WorldCreateOptions::new(CANVAS_SYSTEMS.iter().copied());
    options.canvas = Some(ipp_core::CanvasState {
        extent: [300.0, 200.0],
        units_per_metre: 100.0,
    });
    let world = host
        .create_world_with_options(Default::default(), options)
        .unwrap();
    OutputRef::canvas(host.world_ref(world).unwrap())
}

/// A new camera World and its bound Camera output.
fn camera_world(host: &mut HostRuntime) -> OutputRef {
    let world = host
        .create_world(Default::default(), CAMERA_SYSTEMS)
        .unwrap();
    camera(host, world)
}

fn camera(host: &mut HostRuntime, world: WorldId) -> OutputRef {
    let entity = create(
        host,
        world,
        None,
        vec![ComponentValue::Camera(Camera::default())],
    );
    host.bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap()
}

fn attach(
    host: &mut HostRuntime,
    parent: OutputRef,
    child: OutputRef,
    style: CanvasStyle,
) -> EntityId {
    let surface = Surface {
        width: 2.0,
        height: 1.0,
        ..Default::default()
    };

    create(
        host,
        parent.world().id(),
        None,
        vec![
            ComponentValue::Surface(surface),
            ComponentValue::CanvasStyle(style),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child)),
        ],
    )
}

fn frame(host: &mut HostRuntime) {
    let frame = host.frame(0.0).unwrap();
    assert!(
        frame.publication_errors.is_empty(),
        "{:?}",
        frame.publication_errors
    );
    assert!(
        frame.worlds.values().all(Result::is_ok),
        "{:?}",
        frame.worlds
    );
}

#[test]
fn real_canvas_slots_prepare_nested_cameras_without_leaking_independent_canvases() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let parent = root.world().id();
    let nested = camera_world(&mut host);
    let hidden = canvas(&mut host);
    let hidden_camera = camera_world(&mut host);
    attach(&mut host, root, nested, CanvasStyle::default());
    attach(&mut host, hidden, hidden_camera, CanvasStyle::default());
    host.set_root_output(
        root,
        WorldViewport {
            width: 400,
            height: 200,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    frame(&mut host);

    let publication = host.root_output(parent).unwrap().2;
    let scene = CanvasScene::new(&host, root, publication).unwrap();
    assert_eq!(scene.canvas.logical_extent, [200.0, 100.0]);
    assert_eq!(scene.attachments().count(), 1);
    let outputs = output_order(&host, root, publication).unwrap();
    assert_eq!(
        outputs
            .iter()
            .map(|(selection, _)| *selection)
            .collect::<Vec<_>>(),
        [nested, root]
    );
    let hidden_publication = host.latest_publication(hidden.world().id()).unwrap();
    assert_eq!(
        output_order(&host, hidden, hidden_publication)
            .unwrap()
            .iter()
            .map(|(selection, _)| *selection)
            .collect::<Vec<_>>(),
        [hidden_camera, hidden]
    );
    assert_eq!(host.root_output(parent).unwrap().0, root);
    assert!(CanvasScene::new(&host, nested, publication).is_err());
}

#[test]
fn mapping_preserves_surface_extent_density_signed_scale_and_intersected_clip() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let parent = root.world().id();
    let nested = canvas(&mut host);
    attach(
        &mut host,
        root,
        nested,
        CanvasStyle {
            x: 250.0,
            y: 20.0,
            scale_x: -1.0,
            scale_y: 0.5,
            clipped: true,
            clip_min_x: 50.0,
            clip_max_x: 150.0,
            clip_min_y: 0.0,
            clip_max_y: 80.0,
            ..Default::default()
        },
    );
    frame(&mut host);
    let publication = host.latest_publication(parent).unwrap();
    let scene = CanvasScene::new(&host, root, publication).unwrap();
    let attachment = scene.attachments().next().unwrap();
    let child_canvas = CanvasScene::new(&host, nested, attachment.child.unwrap().id).unwrap();
    assert_eq!(attachment.slot.physical_extent, [2.0, 1.0]);
    assert_eq!(child_canvas.canvas.logical_extent, [200.0, 100.0]);
    let mapping = attachment
        .child_mapping(child_canvas.canvas.logical_extent, attachment.slot.clip)
        .unwrap();
    assert_eq!(mapping.origin, [250.0, 20.0]);
    assert_eq!(mapping.scale, [-1.0, 0.5]);
    assert_eq!(mapping.clip, [50.0, 0.0, 150.0, 80.0]);
    assert_eq!(
        attachment.edge.unwrap().placement,
        attachment
            .slot
            .parent_affine(scene.canvas.logical_extent, scene.canvas.units_per_metre)
    );
    assert!(
        attachment
            .child_mapping([0.0, 100.0], attachment.slot.clip)
            .is_none()
    );
}

#[test]
fn inverse_mapping_never_turns_an_empty_parent_clip_into_visible_content() {
    for scale in [[1.0, 1.0], [-1.0, -1.0]] {
        for clip in [[90.0, 0.0, 20.0, 10.0], [0.0, 10.0, 20.0, 10.0]] {
            let mapping = CanvasMapping::new([100.0, 100.0], scale, clip, [200.0; 2]).unwrap();
            assert_eq!(mapping.clip, [0.0; 4]);
        }
    }
}

#[test]
fn dependency_stamp_changes_for_child_content_with_unchanged_parent_paint() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let parent = root.world().id();
    let nested = canvas(&mut host);
    let child = nested.world().id();
    attach(&mut host, root, nested, CanvasStyle::default());
    let shape = create(
        &mut host,
        child,
        None,
        vec![ComponentValue::CanvasBox(CanvasBox::default())],
    );
    frame(&mut host);
    let initial = host.latest_publication(parent).unwrap();
    let entries = CanvasScene::new(&host, root, initial)
        .unwrap()
        .canvas
        .entries
        .clone();
    let stamp = OutputContentStamp::read(&host, root, initial).unwrap();
    frame(&mut host);
    let unchanged = host.latest_publication(parent).unwrap();
    assert_eq!(
        stamp,
        OutputContentStamp::read(&host, root, unchanged).unwrap()
    );

    apply(
        &mut host,
        child,
        vec![Command::insert_value(
            EntityRef::Handle(shape),
            ComponentValue::CanvasBox(CanvasBox {
                width: 50.0,
                ..Default::default()
            }),
        )],
    );
    frame(&mut host);
    let changed = host.latest_publication(parent).unwrap();
    assert!(Arc::ptr_eq(
        &entries,
        &CanvasScene::new(&host, root, changed)
            .unwrap()
            .canvas
            .entries
    ));
    assert_ne!(
        stamp,
        OutputContentStamp::read(&host, root, changed).unwrap()
    );
}

#[test]
fn canvas_visual_stamp_tracks_each_raster_input_independently() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let world = root.world().id();
    frame(&mut host);
    let publication = host.latest_publication(world).unwrap();
    let original = CanvasScene::new(&host, root, publication).unwrap().canvas;
    let stamp = OutputVisualStamp::from(original);
    for field in 0..5 {
        let mut changed = original.clone();
        match field {
            0 => changed.paint_revision += 1,
            1 => changed.resource_revision += 1,
            2 => changed.logical_extent[0] += 1.0,
            3 => changed.logical_extent[1] += 1.0,
            4 => changed.units_per_metre += 1.0,
            _ => unreachable!(),
        }
        assert_ne!(stamp, OutputVisualStamp::from(&changed), "field {field}");
    }
    let mut input_only = original.clone();
    input_only.input_revision += 1;
    input_only.layout_revision += 1;
    input_only.interaction.hovered = true;
    input_only.interaction.pressed = true;
    assert_eq!(stamp, OutputVisualStamp::from(&input_only));
}

#[test]
fn authored_canvas_mapping_and_camera_versions_invalidate_visual_stamps() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let world = root.world().id();
    frame(&mut host);
    let mut before =
        OutputContentStamp::read(&host, root, host.latest_publication(world).unwrap()).unwrap();
    for update in [
        ipp_core::CanvasStateUpdate {
            extent: Some([301.0, 200.0]),
            ..Default::default()
        },
        ipp_core::CanvasStateUpdate {
            extent: Some([301.0, 201.0]),
            ..Default::default()
        },
        ipp_core::CanvasStateUpdate {
            units_per_metre: Some(200.0),
            ..Default::default()
        },
    ] {
        host.world_mut(world)
            .unwrap()
            .enqueue_canvas_state_update(update)
            .unwrap();
        frame(&mut host);
        let after =
            OutputContentStamp::read(&host, root, host.latest_publication(world).unwrap()).unwrap();
        assert_ne!(before, after);
        before = after;
    }
    let nested = camera_world(&mut host);
    let child = nested.world().id();
    attach(&mut host, root, nested, CanvasStyle::default());
    frame(&mut host);
    before =
        OutputContentStamp::read(&host, root, host.latest_publication(world).unwrap()).unwrap();
    apply(
        &mut host,
        child,
        vec![Command::SetField {
            entity: EntityRef::Handle(nested.camera_entity().unwrap()),
            component: ComponentValue::CAMERA,
            field: ipp_core::FieldWrite {
                offset: std::mem::offset_of!(Camera, far) as u32,
                value: ipp_core::FieldValue::F32(123.0),
            },
        }],
    );
    frame(&mut host);
    let after =
        OutputContentStamp::read(&host, root, host.latest_publication(world).unwrap()).unwrap();
    assert_ne!(before, after);
    assert_eq!(before.outputs.last(), after.outputs.last());
    assert_ne!(
        before.outputs.first().unwrap().visual,
        after.outputs.first().unwrap().visual
    );
}

#[test]
fn hit_and_pointer_only_publications_preserve_visual_content_stamp() {
    use ipp_core::components::rows::Rows;
    use ipp_core::components::{GuiBehavior, GuiButton, GuiLayout};
    use ipp_core::services::gui_input::{
        GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputService,
    };
    use ipp_core::systems::gui::GuiSystem;
    use ipp_core::systems::gui::local::{
        GuiEntityTarget, GuiInteractionUpdate, GuiLocalCommand, GuiLocalEffect,
    };
    use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin};
    use ipp_core::systems::gui::{GuiPartId, GuiPrimitivePart};

    struct Permit;

    impl GuiDeliveryPermit for Permit {
        fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
            Ok(())
        }

        fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
            assert!(
                matches!(terminal, GuiDeliveryTerminal::Applied(_)),
                "{terminal:?}"
            );
        }
    }

    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let world = root.world().id();

    // Override rows win in every state: a plain box that pins what the default
    // look changes under hover, press and disable, so only hits change.
    let mut parts = Rows::new();
    parts
        .push(GuiPaintPart {
            color: Some([0.5, 0.5, 0.5, 1.0]),
            border_width: Some(0.0),
            border_color: Some([0.0; 4]),
            glow_intensity: Some(0.0),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap()
        })
        .unwrap();
    let control = create(
        &mut host,
        world,
        None,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 50.0,
                height: 50.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                parts,
                ..Default::default()
            }),
        ],
    );
    host.set_root_output(
        root,
        WorldViewport {
            width: 300,
            height: 200,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    frame(&mut host);
    let initial = host.latest_publication(world).unwrap();
    let stamp = OutputContentStamp::read(&host, root, initial).unwrap();
    let canvas = CanvasScene::new(&host, root, initial)
        .unwrap()
        .canvas
        .clone();
    let mut version = host.output(initial, root).unwrap().version();
    for enabled in [false, true] {
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(control),
                ComponentValue::GuiBehavior(GuiBehavior {
                    enabled,
                    ..Default::default()
                }),
            )],
        );
        frame(&mut host);
        let current = host.latest_publication(world).unwrap();
        let output = CanvasScene::new(&host, root, current).unwrap().canvas;
        assert_ne!(version, host.output(current, root).unwrap().version());
        version = host.output(current, root).unwrap().version();
        assert_eq!(output.hits[0].eligible, enabled);
        assert_ne!(output.input_revision, canvas.input_revision);
        assert_eq!(output.paint_revision, canvas.paint_revision);
        assert_eq!(
            stamp,
            OutputContentStamp::read(&host, root, current).unwrap()
        );
    }
    let service = GuiInputService::default();
    let session = service.open_session().unwrap();
    let context = service
        .bind_context(&host, &session, root.world())
        .unwrap()
        .context;
    // Hosts route input to the control identity their Canvas publication hit names.
    assert_eq!(canvas.hits[0].target.entity, control);
    let target = GuiEntityTarget::from_canvas(root.world(), canvas.hits[0].target);
    let mut lease = None;
    for (index, update) in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
        GuiInteractionUpdate::Release,
        GuiInteractionUpdate::Hover(false),
        GuiInteractionUpdate::Cancel,
    ]
    .into_iter()
    .enumerate()
    {
        let ticket = service
            .reserve_routed(
                &host,
                &context,
                target,
                index as u64 + 1,
                &[],
                Box::new(Permit),
            )
            .unwrap();
        let pointer = lease
            .get_or_insert_with(|| service.pointer_lease(&ticket, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::interaction(ticket, pointer, update).unwrap();
        host.world_mut(world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 1, command)
            .unwrap();
        frame(&mut host);
        let current = host.latest_publication(world).unwrap();
        if update != GuiInteractionUpdate::Cancel {
            assert_ne!(version, host.output(current, root).unwrap().version());
        }
        version = host.output(current, root).unwrap().version();
        let output = CanvasScene::new(&host, root, current).unwrap().canvas;
        assert_eq!(output.paint_revision, canvas.paint_revision);
        assert_eq!(
            stamp,
            OutputContentStamp::read(&host, root, current).unwrap()
        );
        assert_eq!(service.pending_count(), 0);
    }
    assert!(!lease.as_ref().unwrap().is_live());
    drop(lease);
    service.release_context(&context);
    service.close_session(&session);
}

#[test]
fn retained_publication_slot_cannot_resurrect_retired_write() {
    use ipp_core::components::{GuiButton, GuiLayout};
    use ipp_core::services::gui_input::{
        GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputService,
    };
    use ipp_core::systems::gui::local::{GuiEntityTarget, GuiLocalEffect};

    struct Permit;

    impl GuiDeliveryPermit for Permit {
        fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
            Ok(())
        }

        fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
            assert!(
                matches!(terminal, GuiDeliveryTerminal::Cancelled),
                "{terminal:?}"
            );
        }
    }

    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let parent = root.world().id();
    let nested = canvas(&mut host);
    let anchor = attach(&mut host, root, nested, CanvasStyle::default());
    let control = create(
        &mut host,
        parent,
        None,
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 50.0,
                height: 50.0,
                ..Default::default()
            }),
        ],
    );
    host.set_root_output(
        root,
        WorldViewport {
            width: 300,
            height: 200,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    frame(&mut host);
    let publication = host.latest_publication(parent).unwrap();
    let token = CanvasScene::new(&host, root, publication)
        .unwrap()
        .attachments()
        .find(|attachment| attachment.slot.anchor == anchor)
        .unwrap()
        .slot
        .token
        .clone();

    // A routed GUI reservation keeps its completed source publication readable
    // after the parent publishes again.
    let service = GuiInputService::default();
    let session = service.open_session().unwrap();
    let context = service
        .bind_context(&host, &session, root.world())
        .unwrap()
        .context;
    let hit = CanvasScene::new(&host, root, publication)
        .unwrap()
        .canvas
        .hits
        .iter()
        .find(|hit| hit.target.entity == control)
        .unwrap()
        .clone();
    let reservation = service
        .reserve_routed(
            &host,
            &context,
            GuiEntityTarget::from_canvas(root.world(), hit.target),
            1,
            &[],
            Box::new(Permit),
        )
        .unwrap();

    // Detaching while the anchor's Surface is removed leaves no current slot,
    // but the routed reservation still reads the completed publication whose
    // slot presents this write, and the World canvas stays valid, so the write
    // retires only when that read ends.
    let outcome = apply_outcome(
        &mut host,
        parent,
        Batch {
            id: 2,
            operations: vec![
                Command::DetachWorldAttachmentIf {
                    expected: token.clone(),
                },
                Command::RemoveComponent {
                    entity: EntityRef::Handle(anchor),
                    component: ComponentValue::SURFACE,
                },
            ],
        },
    );
    outcome.result.unwrap();
    assert_ne!(host.latest_publication(parent), Some(publication));
    assert!(host.publication(publication).is_some());
    assert_eq!(
        host.attachment_retirement(&token),
        Ok(ipp_core::WorldAttachmentRetirement::Pending)
    );
    drop(reservation);
    frame(&mut host);
    assert!(host.publication(publication).is_none());
    assert_eq!(
        host.attachment_retirement(&token),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );

    // A valid Surface again does not bring the retired write back.
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::Surface(Surface {
                width: 2.0,
                height: 1.0,
                ..Default::default()
            }),
        )],
    );
    assert_eq!(
        host.attachment_retirement(&token),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );

    service.release_context(&context);
    service.close_session(&session);
}

#[test]
fn camera_child_render_changes_invalidate_stamp_without_changing_canvas_or_camera() {
    let mut host = HostRuntime::new();
    let root = canvas(&mut host);
    let parent = root.world().id();
    let nested = camera_world(&mut host);
    let child = nested.world().id();
    attach(&mut host, root, nested, CanvasStyle::default());
    frame(&mut host);
    let publication = host.latest_publication(parent).unwrap();
    let scene = CanvasScene::new(&host, root, publication).unwrap();
    let entries = scene.canvas.entries.clone();
    let stamp = OutputContentStamp::read(&host, root, publication).unwrap();
    let version = host
        .output(host.latest_publication(child).unwrap(), nested)
        .unwrap()
        .version();

    host.world_mut(child)
        .unwrap()
        .enqueue_render_state_update(ipp_core::RenderStatePatch {
            show_all_debug_geometries: Some(true),
            ..Default::default()
        })
        .unwrap();
    frame(&mut host);
    let publication = host.latest_publication(parent).unwrap();
    let scene = CanvasScene::new(&host, root, publication).unwrap();
    assert!(Arc::ptr_eq(&entries, &scene.canvas.entries));
    assert_eq!(
        host.output(host.latest_publication(child).unwrap(), nested)
            .unwrap()
            .version(),
        version
    );
    assert_ne!(
        stamp,
        OutputContentStamp::read(&host, root, publication).unwrap()
    );
}

#[test]
fn camera_domain_discovers_canvas_slots_but_not_unattached_canvases() {
    let mut host = HostRuntime::new();
    let root = camera_world(&mut host);
    let parent = root.world().id();
    let panel = canvas(&mut host);
    let nested = camera_world(&mut host);
    let hidden = canvas(&mut host);
    let hidden_camera = camera_world(&mut host);
    create(
        &mut host,
        parent,
        None,
        vec![
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(panel)),
        ],
    );
    attach(&mut host, panel, nested, CanvasStyle::default());
    attach(&mut host, hidden, hidden_camera, CanvasStyle::default());
    frame(&mut host);
    let publication = host.latest_publication(parent).unwrap();
    assert_eq!(
        output_order(&host, root, publication)
            .unwrap()
            .iter()
            .map(|(output, _)| *output)
            .collect::<Vec<_>>(),
        [nested, panel, root]
    );
}
