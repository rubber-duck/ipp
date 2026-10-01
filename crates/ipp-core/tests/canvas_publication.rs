//! Real headless Canvas production through Host composition; no transport or GPU claim.

mod support;

use ipp_core::components::{Surface, Transform};
use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
use ipp_core::systems::canvas::{
    CanvasAttachmentSlot, CanvasBox, CanvasGlyph, CanvasGlyphRow, CanvasGlyphRun, CanvasHitKind,
    CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasStyle, CanvasSystem, CanvasText,
};
use ipp_core::{
    Batch, BatchOutcome, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef,
    ErrorReason, FieldValue, FieldWrite, HostRuntime, OperationEffect, OutputKind, OutputRef,
    PublishedWorldAttachment, WorldAttachment, WorldAttachmentEffect, WorldAttachmentRetirement,
    WorldFrameContext, WorldId, WorldViewport,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use support::CanvasTestHost;
use support::canvas_font_bytes as font_bytes;
use support::selection::{ATTACHMENTS, CAMERA, CANVAS, CANVAS_CONTENT, SURFACE, select};
use support::world_failures::select_with_failures;

type FrameObservations = Arc<Mutex<BTreeMap<WorldId, WorldFrameContext>>>;

struct FrameProbe(FrameObservations);

struct FrameProbeFactory(FrameObservations);

const FRAME_PROBE: ipp_core::systems::SystemId =
    ipp_core::systems::SystemId("fixture.canvas-frame-observer");

/// The named parts plus the frame probe, registered after every compiled System.
fn probed(parts: &[&[ipp_core::systems::SystemId]]) -> Vec<ipp_core::systems::SystemId> {
    let mut selected = select(parts);
    selected.push(FRAME_PROBE);
    selected
}

impl ipp_core::systems::SystemFactory for FrameProbeFactory {
    fn id(&self) -> ipp_core::systems::SystemId {
        FRAME_PROBE
    }

    fn create(
        &self,
        _: &mut ipp_core::systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn ipp_core::systems::System>, ipp_core::systems::SystemInitError> {
        Ok(Box::new(FrameProbe(self.0.clone())))
    }
}

impl ipp_core::systems::System for FrameProbe {
    fn update(&mut self, context: &mut ipp_core::systems::SystemUpdateContext<'_, '_>) {
        if let Some(frame) = context.world.frame_context() {
            self.0
                .lock()
                .unwrap()
                .insert(context.world.id(), frame.clone());
        }
    }
}

fn observing_host() -> (HostRuntime, FrameObservations) {
    let observed = Arc::new(Mutex::new(BTreeMap::new()));
    let mut factories = ipp_core::systems::compiled_system_factories();
    factories.push(Arc::new(FrameProbeFactory(observed.clone())));
    (
        HostRuntime::with_system_factories(factories).unwrap(),
        observed,
    )
}

fn edge(host: &HostRuntime, world: WorldId, anchor: EntityId) -> PublishedWorldAttachment {
    host.publication(host.latest_publication(world).unwrap())
        .unwrap()
        .attachments
        .iter()
        .find(|edge| edge.anchor == anchor)
        .unwrap()
        .clone()
}

fn surface_anchor(
    host: &mut HostRuntime,
    parent: OutputRef,
    parent_root: EntityId,
    child: OutputRef,
) -> EntityId {
    let world = parent.world().id();
    let anchor = create(
        host,
        world,
        vec![
            ComponentValue::CanvasStyle(CanvasStyle::default()),
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child)),
        ],
    );
    place(host, world, anchor, parent_root, None);
    anchor
}

fn raw_world(host: &mut HostRuntime) -> WorldId {
    host.create_world(Default::default(), &[CanvasSystem::ID])
        .unwrap()
}

fn resource_world(host: &mut HostRuntime) -> WorldId {
    host.create_world(
        Default::default(),
        &[
            ipp_core::systems::animation::AnimationSystem::ID,
            ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
            CanvasSystem::ID,
        ],
    )
    .unwrap()
}

fn submit(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
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
    submit(host, world, operations).result.unwrap()
}

fn create(host: &mut HostRuntime, world: WorldId, values: Vec<ComponentValue>) -> EntityId {
    let mut commands = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    commands.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    apply(host, world, commands)[0].1
}

fn place(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    parent: EntityId,
    before: Option<EntityId>,
) {
    apply(
        host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: before.map(EntityRef::Handle),
            },
        }],
    );
}

fn scalar(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    component: u16,
    offset: usize,
    value: f32,
) {
    apply(
        host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value: FieldValue::F32(value),
            },
        }],
    );
}

/// The canvas of `world` at 300 x 200 logical units and 100 units per metre,
/// with an empty top-level root entity.
fn canvas(host: &mut HostRuntime, world: WorldId) -> (OutputRef, EntityId) {
    let entity = create(host, world, vec![]);
    let world = host.world_ref(world).unwrap();
    (host.canvas_output(world, [300.0, 200.0], 100.0), entity)
}

/// Queue a canvas density change through the Canvas System command.
fn set_density(host: &mut HostRuntime, world: WorldId, units_per_metre: f32) {
    host.world_mut(world)
        .unwrap()
        .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(units_per_metre),
        })
        .unwrap();
}

fn shape(host: &mut HostRuntime, world: WorldId, parent: EntityId, style: CanvasStyle) -> EntityId {
    let entity = create(
        host,
        world,
        vec![
            ComponentValue::CanvasStyle(style),
            ComponentValue::CanvasBox(CanvasBox {
                width: 20.0,
                height: 10.0,
                ..Default::default()
            }),
        ],
    );
    place(host, world, entity, parent, None);
    entity
}

fn frame(host: &mut HostRuntime) {
    let report = host.frame(0.125).unwrap();
    assert!(
        report.worlds.values().all(Result::is_ok),
        "{:?}",
        report.worlds
    );
    assert!(
        report.publication_errors.is_empty(),
        "{:?}",
        report.publication_errors
    );
}

fn publication(host: &HostRuntime, output: OutputRef) -> (u64, CanvasPublication) {
    let publication = host.latest_publication(output.world().id()).unwrap();
    let chunk = host.output(publication, output).unwrap();
    (
        chunk.version(),
        chunk.data::<CanvasPublication>().unwrap().clone(),
    )
}

fn primitive(entry: &CanvasPaintEntry) -> &CanvasPrimitive {
    match entry {
        CanvasPaintEntry::Primitive {
            primitive,
            ..
        } => primitive,
        _ => panic!("expected primitive"),
    }
}

fn slot(output: &CanvasPublication) -> &CanvasAttachmentSlot {
    output
        .entries
        .iter()
        .find_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Attachment(slot) => Some(slot),
            _ => None,
        })
        .expect("expected attachment slot")
}

#[test]
fn raw_canvas_needs_neither_gui_nor_spatial_systems_and_retains_unchanged_chunks() {
    let mut host = HostRuntime::new();
    let world = raw_world(&mut host);
    let (selected, selected_entity) = canvas(&mut host, world);
    let first = shape(
        &mut host,
        world,
        selected_entity,
        CanvasStyle {
            x: 10.0,
            ..Default::default()
        },
    );
    let second = shape(
        &mut host,
        world,
        selected_entity,
        CanvasStyle {
            x: 40.0,
            ..Default::default()
        },
    );
    frame(&mut host);
    let (version, original) = publication(&host, selected);
    assert_eq!(original.logical_extent, [300.0, 200.0]);
    assert_eq!(original.entries.len(), 2);
    assert_eq!(
        primitive(&original.entries[0])
            .style()
            .identity
            .target
            .entity,
        first
    );
    assert_eq!(
        primitive(&original.entries[1])
            .style()
            .identity
            .target
            .entity,
        second
    );
    assert!(original.hits.is_empty());
    frame(&mut host);
    let (unchanged_version, unchanged) = publication(&host, selected);
    assert_eq!(version, unchanged_version);
    assert!(Arc::ptr_eq(&original.entries, &unchanged.entries));
    assert!(Arc::ptr_eq(&original.hits, &unchanged.hits));

    scalar(
        &mut host,
        world,
        first,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, red),
        0.25,
    );
    frame(&mut host);
    let (_, tinted) = publication(&host, selected);
    assert_ne!(original.paint_revision, tinted.paint_revision);
    assert_eq!(original.layout_revision, tinted.layout_revision);
    assert!(!Arc::ptr_eq(&original.entries[0], &tinted.entries[0]));
    assert!(Arc::ptr_eq(&original.entries[1], &tinted.entries[1]));

    place(&mut host, world, second, selected_entity, Some(first));
    frame(&mut host);
    let (_, reordered) = publication(&host, selected);
    assert!(Arc::ptr_eq(&tinted.entries[0], &reordered.entries[1]));
    assert!(Arc::ptr_eq(&tinted.entries[1], &reordered.entries[0]));
    assert_eq!(primitive(&original.entries[0]).style().color, [1.0; 4]);
}

/// A root viewport supplies CSS pixels and ignores density, only for the
/// presented World's canvas; the stored extent applies otherwise.
#[test]
fn viewport_constraints_apply_only_to_the_exact_selected_canvas() {
    let mut host = HostRuntime::new();
    let world = raw_world(&mut host);
    let other_world = raw_world(&mut host);
    let (selected, _) = canvas(&mut host, world);
    let (other, _) = canvas(&mut host, other_world);
    host.set_root_output(
        selected,
        WorldViewport {
            width: 1200,
            height: 800,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    frame(&mut host);
    assert_eq!(
        publication(&host, selected).1.logical_extent,
        [600.0, 400.0]
    );
    let (other_version, _) = publication(&host, other);
    assert_eq!(publication(&host, other).1.logical_extent, [300.0, 200.0]);
    host.world_mut(world)
        .unwrap()
        .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(250.0),
        })
        .unwrap();
    frame(&mut host);
    assert_eq!(
        publication(&host, selected).1.logical_extent,
        [600.0, 400.0]
    );
    let evaluated = host.world_mut(world).unwrap().canvas_state().unwrap();
    assert_eq!(evaluated.state.units_per_metre, 250.0);
    assert_eq!(evaluated.evaluated.unwrap().extent, [600.0, 400.0]);
    assert_eq!(publication(&host, other).0, other_version);
    host.clear_root_output(world);
    frame(&mut host);
    assert_eq!(
        publication(&host, selected).1.logical_extent,
        [300.0, 200.0]
    );
}

#[test]
fn nested_surface_slot_uses_one_invertible_mapping_without_rewriting_physical_extent() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let (parent_output, parent_output_entity) = canvas(&mut host, parent);
    let (child_output, _) = canvas(&mut host, child);
    let physical_surface = Surface {
        width: 2.0,
        height: 0.5,
    };
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform {
                x: 900.0,
                y: 700.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 20.0,
                y: 30.0,
                scale_x: 0.5,
                scale_y: 2.0,
                ..Default::default()
            }),
            ComponentValue::Surface(physical_surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
        ],
    );
    place(&mut host, parent, anchor, parent_output_entity, None);
    frame(&mut host);
    let (_, original) = publication(&host, parent_output);
    let original_slot = slot(&original);
    assert_eq!(original_slot.anchor, anchor);
    assert_eq!(original_slot.physical_extent, [2.0, 0.5]);
    assert_eq!(original_slot.to_canvas([-1.0, 0.25]), [20.0, 30.0]);
    assert_eq!(original_slot.to_canvas([1.0, -0.25]), [120.0, 130.0]);
    assert_eq!(original_slot.from_canvas([45.0, 55.0]), Some([-0.5, 0.125]));
    assert!(original_slot.from_canvas([f64::NAN, 0.0]).is_none());
    assert_eq!(
        original_slot.parent_affine(original.logical_extent, original.units_per_metre)[12..14],
        [-0.8, 0.2]
    );
    let original_edge = edge(&host, parent, anchor);
    assert_eq!(original_edge.token, original_slot.token);
    assert_eq!(original_edge.placement_output, Some(parent_output));
    assert_eq!(original_edge.output, Some(child_output));
    assert_eq!(
        original_edge.placement,
        original_slot.parent_affine(original.logical_extent, original.units_per_metre)
    );
    let child_frame = observed.lock().unwrap()[&child].clone();
    assert_eq!(child_frame.placement, original_edge.placement);
    assert_eq!(child_frame.surface_extent, Some([2.0, 0.5]));
    assert_eq!(child_frame.selected_output, Some(child_output));
    assert!(host.attached_publication(&original_edge).is_some());
    assert_eq!(
        original.hits[0].ancestry.as_ref(),
        &[parent_output_entity, anchor]
    );
    assert!(original.hits[0].contains([20.0, 30.0]));
    assert!(!original.hits[0].contains([120.0, 130.0]));
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [200.0, 50.0]
    );

    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, x),
        50.0,
    );
    frame(&mut host);
    let (_, moved) = publication(&host, parent_output);
    assert_eq!(slot(&moved).to_canvas([-1.0, 0.25]), [50.0, 30.0]);
    assert_eq!(slot(&moved).physical_extent, [2.0, 0.5]);
    let moved_edge = edge(&host, parent, anchor);
    assert_eq!(moved_edge.placement[12..14], [-0.5, 0.2]);
    assert_eq!(moved_edge.token, original_edge.token);
    assert_eq!(
        observed.lock().unwrap()[&child].placement,
        moved_edge.placement
    );
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [200.0, 50.0]
    );
    assert_eq!(slot(&original).to_canvas([-1.0, 0.25]), [20.0, 30.0]);

    host.set_root_output(
        parent_output,
        WorldViewport {
            width: 1200,
            height: 800,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    frame(&mut host);
    let (_, resized) = publication(&host, parent_output);
    assert_eq!(resized.logical_extent, [600.0, 400.0]);
    assert_eq!(slot(&resized).to_canvas([-1.0, 0.25]), [50.0, 30.0]);
    let resized_edge = edge(&host, parent, anchor);
    assert_eq!(resized_edge.placement[12..14], [-2.0, 1.2]);
    assert_eq!(
        resized_edge.placement,
        slot(&resized).parent_affine(resized.logical_extent, resized.units_per_metre)
    );
    assert_eq!(
        observed.lock().unwrap()[&child].placement,
        resized_edge.placement
    );
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [200.0, 50.0]
    );
}

#[test]
fn core_ancestry_visual_transforms_and_empty_clips_are_shared_by_paint_and_hits() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let (parent_output, parent_output_entity) = canvas(&mut host, parent);
    let (child_output, _) = canvas(&mut host, child);
    let group = create(
        &mut host,
        parent,
        vec![ComponentValue::CanvasStyle(CanvasStyle {
            x: 100.0,
            y: 20.0,
            scale_x: -1.0,
            clipped: true,
            clip_min_x: 0.0,
            clip_min_y: 0.0,
            clip_max_x: 50.0,
            clip_max_y: 50.0,
            ..Default::default()
        })],
    );
    place(&mut host, parent, group, parent_output_entity, None);
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
        ],
    );
    place(&mut host, parent, anchor, group, None);
    frame(&mut host);
    let (_, clipped) = publication(&host, parent_output);
    assert_eq!(slot(&clipped).clip, [50.0, 20.0, 100.0, 70.0]);
    assert_eq!(clipped.hits[0].clip, slot(&clipped).clip);
    assert!(clipped.hits[0].contains([60.0, 30.0]));
    assert!(!clipped.hits[0].contains([40.0, 30.0]));
    scalar(
        &mut host,
        parent,
        group,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, clip_max_x),
        -1.0,
    );
    frame(&mut host);
    let (_, empty) = publication(&host, parent_output);
    assert!(!empty.hits[0].contains([60.0, 30.0]));
    assert!(slot(&empty).clip[2] <= slot(&empty).clip[0]);
    assert_eq!(edge(&host, parent, anchor).surface_extent, Some([1.0, 1.0]));
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [100.0, 100.0]
    );
}

#[test]
fn parent_tint_and_opacity_multiply_into_descendant_paint_without_their_own_style() {
    let mut host = HostRuntime::new();
    let world = raw_world(&mut host);
    let (output, root) = canvas(&mut host, world);
    let group = create(
        &mut host,
        world,
        vec![ComponentValue::CanvasStyle(CanvasStyle {
            red: 0.5,
            green: 0.8,
            opacity: 0.5,
            ..Default::default()
        })],
    );
    place(&mut host, world, group, root, None);
    // A styled child multiplies its own tint into the inherited one.
    shape(
        &mut host,
        world,
        group,
        CanvasStyle {
            green: 0.5,
            blue: 0.25,
            alpha: 0.5,
            ..Default::default()
        },
    );
    // A child without a CanvasStyle paints with the inherited style unchanged.
    let plain = create(
        &mut host,
        world,
        vec![ComponentValue::CanvasBox(CanvasBox::default())],
    );
    place(&mut host, world, plain, group, None);
    frame(&mut host);

    let (_, painted) = publication(&host, output);
    let styles: Vec<_> = painted
        .entries
        .iter()
        .map(|entry| *primitive(entry).style())
        .map(|style| (style.color, style.opacity))
        .collect();
    assert_eq!(
        styles,
        [([0.5, 0.4, 0.25, 0.5], 0.5), ([0.5, 0.8, 1.0, 1.0], 0.5)]
    );

    // A positioned glyph's own colour composes with the inherited tint too.
    let style = *primitive(&painted.entries[1]).style();
    let glyph = |color| CanvasGlyph {
        glyph_id: 0,
        position: [0.0; 2],
        color,
    };
    assert_eq!(style.glyph_tint(&glyph(None)), [0.5, 0.8, 1.0, 1.0]);
    assert_eq!(
        style.glyph_tint(&glyph(Some([1.0, 0.5, 0.0, 0.5]))),
        [0.5, 0.4, 0.0, 0.5]
    );
}

#[test]
fn camera_and_parent_surface_motion_retain_child_canvas_payloads() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let camera = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
            ComponentValue::Camera(Default::default()),
        ],
    );
    let camera_output = host
        .bind_output(host.world_ref(parent).unwrap(), camera, OutputKind::Camera)
        .unwrap();
    host.set_root_output(
        camera_output,
        WorldViewport {
            width: 800,
            height: 600,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    let (child_output, child_output_entity) = canvas(&mut host, child);
    shape(
        &mut host,
        child,
        child_output_entity,
        CanvasStyle::default(),
    );
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
        ],
    );
    frame(&mut host);
    let (version, original) = publication(&host, child_output);
    scalar(
        &mut host,
        parent,
        camera,
        ComponentValue::TRANSFORM,
        std::mem::offset_of!(Transform, x),
        10.0,
    );
    frame(&mut host);
    let (camera_version, camera_only) = publication(&host, child_output);
    assert_eq!(version, camera_version);
    assert!(Arc::ptr_eq(&original.entries, &camera_only.entries));
    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::TRANSFORM,
        std::mem::offset_of!(Transform, x),
        42.0,
    );
    frame(&mut host);
    let (moved_version, moved) = publication(&host, child_output);
    assert_eq!(version, moved_version);
    assert!(Arc::ptr_eq(&original.entries, &moved.entries));
    assert_eq!(observed.lock().unwrap()[&child].placement[12], 42.0);
    assert_eq!(edge(&host, parent, anchor).placement_output, None);
}

#[test]
fn nested_surface_frame_context_composes_each_mapping_exactly_once() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let middle = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let (parent_output, parent_output_entity) = canvas(&mut host, parent);
    let (middle_output, middle_output_entity) = canvas(&mut host, middle);
    let (child_output, _) = canvas(&mut host, child);
    let parent_anchor = surface_anchor(
        &mut host,
        parent_output,
        parent_output_entity,
        middle_output,
    );
    let middle_anchor =
        surface_anchor(&mut host, middle_output, middle_output_entity, child_output);
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(parent_anchor),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 20.0,
                y: 30.0,
                scale_x: 0.5,
                scale_y: 2.0,
                ..Default::default()
            }),
        )],
    );
    apply(
        &mut host,
        middle,
        vec![Command::insert_value(
            EntityRef::Handle(middle_anchor),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 10.0,
                y: 5.0,
                scale_x: 2.0,
                scale_y: 0.5,
                ..Default::default()
            }),
        )],
    );
    frame(&mut host);
    let parent_edge = edge(&host, parent, parent_anchor);
    let middle_edge = edge(&host, middle, middle_anchor);
    assert_eq!(parent_edge.placement[12..14], [-1.05, -0.3]);
    assert_eq!(middle_edge.placement[12..14], [0.6, 0.2]);
    let child_frame = observed.lock().unwrap()[&child].clone();
    assert!((child_frame.placement[12] + 0.75).abs() < 1e-12);
    assert!((child_frame.placement[13] - 0.1).abs() < 1e-12);
    assert_eq!(
        [child_frame.placement[0], child_frame.placement[5]],
        [1.0, 1.0]
    );
    assert_eq!(child_frame.surface_extent, Some([1.0, 1.0]));
    assert_eq!(child_frame.selected_output, Some(child_output));
    assert_eq!(
        publication(&host, middle_output).1.logical_extent,
        [100.0, 100.0]
    );
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [100.0, 100.0]
    );
}

#[test]
fn attachment_opacity_is_retained_in_paint_without_changing_mapping_or_child_state() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let (output, output_entity) = canvas(&mut host, parent);
    let (child_output, child_output_entity) = canvas(&mut host, child);
    let outside = shape(&mut host, parent, output_entity, CanvasStyle::default());
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(output_entity),
            ComponentValue::CanvasStyle(CanvasStyle {
                opacity: 0.5,
                ..Default::default()
            }),
        )],
    );
    let group = create(
        &mut host,
        parent,
        vec![ComponentValue::CanvasStyle(CanvasStyle {
            opacity: 0.5,
            ..Default::default()
        })],
    );
    place(&mut host, parent, group, output_entity, None);
    let anchor = surface_anchor(&mut host, output, output_entity, child_output);
    place(&mut host, parent, anchor, group, None);
    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, opacity),
        0.5,
    );
    shape(
        &mut host,
        child,
        child_output_entity,
        CanvasStyle {
            opacity: 0.5,
            ..Default::default()
        },
    );
    frame(&mut host);
    let original = publication(&host, output).1;
    let original_child = publication(&host, child_output).1;
    let original_edge = edge(&host, parent, anchor);
    assert_eq!(slot(&original).opacity, 0.125);
    assert_eq!(primitive(&original_child.entries[0]).style().opacity, 0.5);
    assert_eq!(
        primitive(&original.entries[0])
            .style()
            .identity
            .target
            .entity,
        outside
    );
    assert!(original.hits[0].eligible);

    scalar(
        &mut host,
        parent,
        group,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, opacity),
        0.25,
    );
    frame(&mut host);
    let fractional = publication(&host, output).1;
    assert_eq!(slot(&fractional).opacity, 0.0625);
    assert!(fractional.paint_revision > original.paint_revision);
    assert_eq!(fractional.layout_revision, original.layout_revision);
    assert_eq!(fractional.resource_revision, original.resource_revision);
    assert_eq!(fractional.input_revision, original.input_revision);
    assert!(Arc::ptr_eq(&fractional.hits, &original.hits));
    assert!(Arc::ptr_eq(&fractional.entries[0], &original.entries[0]));
    assert!(!Arc::ptr_eq(&fractional.entries[1], &original.entries[1]));
    assert!(Arc::ptr_eq(
        &publication(&host, child_output).1.entries,
        &original_child.entries
    ));
    let fractional_edge = edge(&host, parent, anchor);
    assert_eq!(fractional_edge.token, original_edge.token);
    assert_eq!(fractional_edge.placement, original_edge.placement);
    assert_eq!(fractional_edge.surface_extent, original_edge.surface_extent);
    assert_eq!(slot(&original).opacity, 0.125);

    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, opacity),
        0.0,
    );
    frame(&mut host);
    let invisible = publication(&host, output).1;
    assert_eq!(slot(&invisible).opacity, 0.0);
    assert!(!invisible.hits[0].eligible);
    assert!(invisible.paint_revision > fractional.paint_revision);
    assert!(invisible.input_revision > fractional.input_revision);
    assert_eq!(invisible.layout_revision, fractional.layout_revision);
    assert_eq!(invisible.resource_revision, fractional.resource_revision);
    let invisible_edge = edge(&host, parent, anchor);
    assert_eq!(invisible_edge.token, original_edge.token);
    assert_eq!(invisible_edge.placement_output, Some(output));
    assert_eq!(invisible_edge.placement, original_edge.placement);
    assert!(host.attached_publication(&invisible_edge).is_some());
    assert_eq!(
        observed.lock().unwrap()[&child].selected_output,
        Some(child_output)
    );
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([1.0, 1.0])
    );
    assert!(Arc::ptr_eq(
        &publication(&host, child_output).1.entries,
        &original_child.entries
    ));
    frame(&mut host);
    let unchanged = publication(&host, output).1;
    assert!(Arc::ptr_eq(&invisible.entries, &unchanged.entries));
    assert!(Arc::ptr_eq(&invisible.hits, &unchanged.hits));

    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, opacity),
        0.5,
    );
    frame(&mut host);
    let restored = publication(&host, output).1;
    assert_eq!(slot(&restored).opacity, 0.0625);
    assert!(restored.hits[0].eligible);
    assert_eq!(slot(&restored).token, original_edge.token);
}

#[test]
fn zero_ancestor_opacity_keeps_nested_camera_attachment_available_but_not_hittable() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CAMERA]))
        .unwrap();
    let (output, output_entity) = canvas(&mut host, parent);
    let camera = create(
        &mut host,
        child,
        vec![ComponentValue::Camera(
            ipp_core::components::Camera::default(),
        )],
    );
    let child_output = host
        .bind_output(host.world_ref(child).unwrap(), camera, OutputKind::Camera)
        .unwrap();
    let group = create(
        &mut host,
        parent,
        vec![ComponentValue::CanvasStyle(CanvasStyle::default())],
    );
    place(&mut host, parent, group, output_entity, None);
    let anchor = surface_anchor(&mut host, output, output_entity, child_output);
    place(&mut host, parent, anchor, group, None);
    frame(&mut host);
    let original = edge(&host, parent, anchor);
    scalar(
        &mut host,
        parent,
        group,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, opacity),
        0.0,
    );
    frame(&mut host);
    let invisible = publication(&host, output).1;
    assert_eq!(slot(&invisible).opacity, 0.0);
    assert!(!invisible.hits[0].contains([10.0, 10.0]));
    let attached = edge(&host, parent, anchor);
    assert_eq!(attached.token, original.token);
    assert_eq!(attached.placement, original.placement);
    assert_eq!(attached.output, Some(child_output));
    assert!(host.attached_publication(&attached).is_some());
    assert!(
        host.output(host.latest_publication(child).unwrap(), child_output)
            .is_some()
    );
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([1.0, 1.0])
    );
}

#[test]
fn unavailable_canvas_mapping_suppresses_hierarchy_fallback_and_can_recover() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let (parent_output, parent_output_entity) = canvas(&mut host, parent);
    let (child_output, _) = canvas(&mut host, child);
    let anchor = surface_anchor(&mut host, parent_output, parent_output_entity, child_output);
    frame(&mut host);
    let original = edge(&host, parent, anchor);
    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, scale_x),
        0.0,
    );
    frame(&mut host);
    let (_, collapsed) = publication(&host, parent_output);
    assert_eq!(slot(&collapsed).token, original.token);
    assert!(slot(&collapsed).from_canvas([0.0; 2]).is_none());
    assert!(!collapsed.hits[0].eligible);
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    assert_eq!(observed.lock().unwrap()[&child].selected_output, None);
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [300.0, 200.0]
    );

    scalar(
        &mut host,
        parent,
        anchor,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, scale_x),
        1.0,
    );
    frame(&mut host);
    assert_eq!(edge(&host, parent, anchor).token, original.token);
    assert_eq!(
        observed.lock().unwrap()[&child].selected_output,
        Some(child_output)
    );

    set_density(&mut host, parent, f32::MAX);
    let grandparent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, SURFACE]))
        .unwrap();
    let physical = Surface {
        width: 2.0,
        ..Default::default()
    };
    create(
        &mut host,
        grandparent,
        vec![
            ComponentValue::Surface(physical),
            ComponentValue::WorldAttachment(WorldAttachment::surface(parent_output)),
        ],
    );
    frame(&mut host);
    assert!(
        host.output(host.latest_publication(parent).unwrap(), parent_output)
            .is_none()
    );
    assert!(
        host.publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    assert_eq!(observed.lock().unwrap()[&child].surface_extent, None);
    set_density(&mut host, parent, 100.0);
    frame(&mut host);
    assert_eq!(edge(&host, parent, anchor).token, original.token);
    assert_eq!(
        publication(&host, parent_output).1.logical_extent,
        [200.0, 100.0]
    );
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([1.0, 1.0])
    );
}

#[test]
fn canvas_slots_and_hits_track_applied_write_tokens_not_just_component_incarnations() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
        .unwrap();
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let (parent_output, parent_output_entity) = canvas(&mut host, parent);
    let (child_output, _) = canvas(&mut host, child);
    let anchor = surface_anchor(&mut host, parent_output, parent_output_entity, child_output);
    frame(&mut host);
    let (_, original) = publication(&host, parent_output);
    let original_edge = edge(&host, parent, anchor);
    assert_eq!(slot(&original).token, original_edge.token);
    assert_eq!(
        original.hits[0].kind,
        CanvasHitKind::Attachment {
            anchor,
            token: original_edge.token.clone()
        }
    );

    let replacement = {
        let outcome = submit(
            &mut host,
            parent,
            vec![Command::SetField {
                entity: EntityRef::Handle(anchor),
                component: ComponentValue::WORLD_ATTACHMENT,
                field: FieldWrite {
                    offset: std::mem::offset_of!(WorldAttachment, output) as u32,
                    value: FieldValue::Output(None),
                },
            }],
        );
        outcome.result.unwrap();
        let OperationEffect::WorldAttachment(WorldAttachmentEffect::Written(token)) =
            &outcome.effects[0].effect
        else {
            panic!("expected attachment write receipt");
        };
        token.clone()
    };
    assert_ne!(replacement, original_edge.token);
    assert_eq!(replacement.incarnation(), original_edge.token.incarnation());
    frame(&mut host);
    let (version, rewritten) = publication(&host, parent_output);
    assert_eq!(slot(&rewritten).token, replacement);
    assert_eq!(edge(&host, parent, anchor).token, replacement);
    assert_eq!(
        rewritten.hits[0].kind,
        CanvasHitKind::Attachment {
            anchor,
            token: replacement.clone()
        }
    );
    assert_eq!(rewritten.layout_revision, original.layout_revision);
    assert!(rewritten.paint_revision > original.paint_revision);
    assert!(rewritten.input_revision > original.input_revision);
    assert_eq!(
        host.attachment_retirement(&original_edge.token),
        Ok(WorldAttachmentRetirement::Retired)
    );
    assert!(host.attached_publication(&original_edge).is_none());

    apply(
        &mut host,
        parent,
        vec![Command::DetachWorldAttachmentIf {
            expected: original_edge.token.clone(),
        }],
    );
    frame(&mut host);
    let (unchanged_version, unchanged) = publication(&host, parent_output);
    assert_eq!(version, unchanged_version);
    assert!(Arc::ptr_eq(&rewritten.entries, &unchanged.entries));
    let frozen = edge(&host, parent, anchor);
    assert_eq!(frozen.token, replacement);
}

/// Queue an invalid canvas extent: refused at the mutation boundary, with the
/// canvas state and its output unchanged.
fn refuse_canvas_extent(host: &mut HostRuntime, output: OutputRef) {
    let world = output.world().id();
    let before = host.world_mut(world).unwrap().canvas_state().unwrap().state;
    host.world_mut(world)
        .unwrap()
        .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
            extent: Some([0.0, before.extent[1]]),
            units_per_metre: None,
        })
        .unwrap();
    frame(host);
    assert_eq!(
        host.world_mut(world).unwrap().canvas_state().unwrap().state,
        before
    );
    assert_eq!(
        host.resolve_output_ref(output.world(), output.target()),
        Ok(output)
    );
}

fn bounded_relationship_case(name: &str) -> bool {
    if std::env::var("IPP_CANVAS_RELATIONSHIP_CASE").as_deref() == Ok(name) {
        return true;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("IPP_CANVAS_RELATIONSHIP_CASE", name)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(2) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "relationship traversal exceeded two seconds: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

fn failed_cycle(host: &mut HostRuntime, world: WorldId, entity: EntityId, parent: EntityId) {
    let outcome = submit(
        host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::UnsupportedDependency
    );
    let context = host.world_mut(world).unwrap();
    assert_eq!(context.entity_link(entity).unwrap().parent, Some(parent));
    assert_eq!(context.fault(), None);
}

#[test]
fn failed_structural_edits_preserve_canvas_and_camera_output_lifetimes() {
    for kind in [OutputKind::Canvas, OutputKind::Camera] {
        for attached in [false, true] {
            let mut host = HostRuntime::new();
            let world = host
                .create_world(
                    Default::default(),
                    if kind == OutputKind::Canvas {
                        CANVAS
                    } else {
                        CAMERA
                    },
                )
                .unwrap();
            let (output, output_entity) = if kind == OutputKind::Canvas {
                let (output, output_entity) = canvas(&mut host, world);
                shape(&mut host, world, output_entity, CanvasStyle::default());
                (output, output_entity)
            } else {
                let entity = create(
                    &mut host,
                    world,
                    vec![
                        ComponentValue::Transform(Transform::default()),
                        ComponentValue::Camera(ipp_core::components::Camera::default()),
                    ],
                );
                let output = host
                    .bind_output(host.world_ref(world).unwrap(), entity, kind)
                    .unwrap();
                (output, entity)
            };
            let parent_edge = attached.then(|| {
                let parent = host
                    .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
                    .unwrap();
                let (parent_output, parent_output_entity) = canvas(&mut host, parent);
                let anchor = surface_anchor(&mut host, parent_output, parent_output_entity, output);
                (parent, anchor)
            });
            if !attached {
                host.set_root_output(
                    output,
                    WorldViewport {
                        width: 640,
                        height: 480,
                        device_pixel_ratio: 1.0,
                    },
                )
                .unwrap();
            }
            frame(&mut host);
            assert!(
                host.output(host.latest_publication(world).unwrap(), output)
                    .is_some()
            );
            failed_cycle(&mut host, world, output_entity, output_entity);
            assert_eq!(
                host.resolve_output_ref(output.world(), output.target()),
                Ok(output)
            );
            apply(
                &mut host,
                world,
                vec![Command::PlaceEntity {
                    entity: EntityRef::Handle(output_entity),
                    placement: EntityPlacementRef {
                        parent: None,
                        before: None,
                    },
                }],
            );
            frame(&mut host);
            assert_eq!(
                host.resolve_output_ref(output.world(), output.target()),
                Ok(output)
            );
            let current = host.latest_publication(world).unwrap();
            assert!(host.output(current, output).is_some());
            if let Some((parent, anchor)) = parent_edge {
                let edge = edge(&host, parent, anchor);
                assert_eq!(edge.publication, Some(current));
                assert_eq!(host.attached_publication(&edge).unwrap().id, current);
            } else {
                assert_eq!(host.root_output(world).unwrap().2, current);
            }
        }
    }
}

#[test]
fn cyclic_canvas_roots_and_ancestor_branches_are_unavailable_until_corrected() {
    if !bounded_relationship_case(
        "cyclic_canvas_roots_and_ancestor_branches_are_unavailable_until_corrected",
    ) {
        return;
    }
    for cycle_kind in 0..3 {
        let (mut host, observed) = observing_host();
        let parent = host
            .create_world(Default::default(), &select(&[ATTACHMENTS, CANVAS, SURFACE]))
            .unwrap();
        let child = host
            .create_world(Default::default(), &probed(&[CANVAS]))
            .unwrap();
        let (output, output_entity) = canvas(&mut host, parent);
        let (child_output, _) = canvas(&mut host, child);
        let anchor = surface_anchor(&mut host, output, output_entity, child_output);
        let painted = shape(&mut host, parent, output_entity, CanvasStyle::default());
        // An unrelated top-level branch of the same canvas.
        let unrelated_root = create(&mut host, parent, Vec::new());
        let unrelated = shape(&mut host, parent, unrelated_root, CanvasStyle::default());
        let first = create(&mut host, parent, Vec::new());
        let second = create(&mut host, parent, Vec::new());
        let (broken, target) = match cycle_kind {
            0 => (output_entity, output_entity),
            1 => {
                place(&mut host, parent, first, output_entity, None);
                (output_entity, first)
            }
            _ => {
                place(&mut host, parent, output_entity, first, None);
                place(&mut host, parent, first, second, None);
                (second, first)
            }
        };
        frame(&mut host);
        let original = publication(&host, output).1;
        let original_edge = edge(&host, parent, anchor);
        assert_eq!(original.entries.len(), 3);
        assert_eq!(original.hits.len(), 1);
        let unrelated_entry = original
            .entries
            .iter()
            .find(|entry| {
                matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. }
                    if primitive.style().identity.target.entity == unrelated)
            })
            .unwrap()
            .clone();
        failed_cycle(&mut host, parent, broken, target);
        frame(&mut host);
        assert_eq!(
            host.resolve_output_ref(output.world(), output.target()),
            Ok(output)
        );
        // Only the cyclic branch leaves the canvas; the unrelated branch keeps
        // its retained paint.
        let unavailable = host.latest_publication(parent).unwrap();
        let remaining = publication(&host, output).1;
        assert_eq!(remaining.entries.len(), 1);
        assert!(Arc::ptr_eq(&remaining.entries[0], &unrelated_entry));
        assert!(remaining.hits.is_empty());
        assert!(
            host.publication(unavailable)
                .unwrap()
                .attachments
                .is_empty()
        );
        assert_eq!(observed.lock().unwrap()[&child].selected_output, None);
        scalar(
            &mut host,
            parent,
            painted,
            ComponentValue::CANVAS_BOX,
            std::mem::offset_of!(CanvasBox, width),
            31.0,
        );
        apply(
            &mut host,
            parent,
            vec![Command::PlaceEntity {
                entity: EntityRef::Handle(broken),
                placement: EntityPlacementRef {
                    parent: None,
                    before: None,
                },
            }],
        );
        frame(&mut host);
        let recovered = publication(&host, output).1;
        assert_eq!(recovered.entries.len(), 3);
        assert_eq!(recovered.hits.len(), 1);
        assert_eq!(slot(&recovered).token, original_edge.token);
        assert!(recovered.entries.iter().any(|entry| matches!(
            entry.as_ref(),
            CanvasPaintEntry::Primitive {
                primitive: CanvasPrimitive::Box {
                    size: [31.0, 10.0],
                    ..
                },
                ..
            }
        )));
        assert_eq!(edge(&host, parent, anchor).placement_output, Some(output));
        assert_eq!(
            observed.lock().unwrap()[&child].surface_extent,
            Some([1.0, 1.0])
        );
    }
}

#[test]
fn cyclic_descendants_leave_the_canvas_and_their_spatial_attachments_until_corrected() {
    if !bounded_relationship_case(
        "cyclic_descendants_leave_the_canvas_and_their_spatial_attachments_until_corrected",
    ) {
        return;
    }
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(
            Default::default(),
            &[
                CanvasSystem::ID,
                ipp_core::systems::world_attachment::WorldAttachmentSystem::ID,
            ],
        )
        .unwrap();
    let child = raw_world(&mut host);
    let (output, output_entity) = canvas(&mut host, parent);
    let group = create(&mut host, parent, Vec::new());
    place(&mut host, parent, group, output_entity, None);
    shape(&mut host, parent, group, CanvasStyle::default());
    let child_world = host.world_ref(child).unwrap();
    let anchor = create(
        &mut host,
        parent,
        vec![ComponentValue::WorldAttachment(WorldAttachment::spatial(
            child_world,
        ))],
    );
    place(&mut host, parent, anchor, group, None);
    frame(&mut host);
    assert_eq!(publication(&host, output).1.entries.len(), 1);
    // A spatial attachment in a canvas World keeps its spatial placement.
    assert_eq!(edge(&host, parent, anchor).placement_output, None);
    failed_cycle(&mut host, parent, group, group);
    frame(&mut host);
    let invalid = host.latest_publication(parent).unwrap();
    assert!(publication(&host, output).1.entries.is_empty());
    assert!(publication(&host, output).1.hits.is_empty());
    assert!(host.publication(invalid).unwrap().attachments.is_empty());
    assert_eq!(host.spatial_contributions(invalid).len(), 1);
    place(&mut host, parent, group, output_entity, None);
    frame(&mut host);
    assert_eq!(publication(&host, output).1.entries.len(), 1);
    assert_eq!(edge(&host, parent, anchor).placement_output, None);
}

#[test]
fn refused_canvas_update_keeps_its_attachments_scoped_beside_camera_contributions() {
    let (mut host, observed) = observing_host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CAMERA, CANVAS, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[CANVAS]))
        .unwrap();
    let spatial_child = host.create_world(Default::default(), &[]).unwrap();
    let camera = create(
        &mut host,
        parent,
        vec![ComponentValue::Camera(Default::default())],
    );
    let camera_output = host
        .bind_output(host.world_ref(parent).unwrap(), camera, OutputKind::Camera)
        .unwrap();
    host.set_root_output(
        camera_output,
        WorldViewport {
            width: 800,
            height: 600,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    let (selected, selected_entity) = canvas(&mut host, parent);
    shape(&mut host, parent, selected_entity, CanvasStyle::default());
    let (child_output, _) = canvas(&mut host, child);
    let anchor = surface_anchor(&mut host, selected, selected_entity, child_output);
    let spatial_world = host.world_ref(spatial_child).unwrap();
    let spatial_anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(spatial_world)),
        ],
    );
    place(&mut host, parent, spatial_anchor, selected_entity, None);
    frame(&mut host);
    // The Surface anchor is a canvas slot; the spatial attachment keeps its
    // spatial placement and contributes to the camera beside the World's own
    // content.
    assert_eq!(edge(&host, parent, anchor).placement_output, Some(selected));
    assert_eq!(edge(&host, parent, spatial_anchor).placement_output, None);
    assert_eq!(
        host.spatial_contributions(host.latest_publication(parent).unwrap())
            .len(),
        2
    );

    refuse_canvas_extent(&mut host, selected);
    frame(&mut host);
    let unchanged = host.latest_publication(parent).unwrap();
    assert_eq!(publication(&host, selected).1.entries.len(), 2);
    assert!(!host.publication(unchanged).unwrap().attachments.is_empty());
    assert_eq!(host.spatial_contributions(unchanged).len(), 2);
    assert_eq!(edge(&host, parent, anchor).placement_output, Some(selected));
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([1.0, 1.0])
    );
    assert!(host.root_output(parent).is_some());
}

#[test]
fn spatial_child_does_not_inherit_parent_camera_or_viewport_selection() {
    let mut host = HostRuntime::new();
    let parent = host
        .create_world(Default::default(), &select(&[ATTACHMENTS, CAMERA]))
        .unwrap();
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let camera = create(
        &mut host,
        parent,
        vec![ComponentValue::Camera(Default::default())],
    );
    let selection = host
        .bind_output(host.world_ref(parent).unwrap(), camera, OutputKind::Camera)
        .unwrap();
    host.set_root_output(
        selection,
        WorldViewport {
            width: 1200,
            height: 800,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    let (child_output, _) = canvas(&mut host, child);
    let child_world = host.world_ref(child).unwrap();
    create(
        &mut host,
        parent,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(child_world)),
        ],
    );
    frame(&mut host);
    assert_eq!(
        publication(&host, child_output).1.logical_extent,
        [300.0, 200.0]
    );
}

#[test]
fn replacing_leaf_components_changes_incarnation_not_reordered_identity() {
    let mut host = HostRuntime::new();
    let world = raw_world(&mut host);
    let (selected, selected_entity) = canvas(&mut host, world);
    let entity = shape(&mut host, world, selected_entity, Default::default());
    frame(&mut host);
    let (_, original) = publication(&host, selected);
    let previous_id = primitive(&original.entries[0]).style().identity;
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::CANVAS_BOX,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::CanvasBox(CanvasBox::default()),
            ),
        ],
    );
    frame(&mut host);
    let (_, replaced) = publication(&host, selected);
    assert_ne!(
        previous_id,
        primitive(&replaced.entries[0]).style().identity
    );
}

#[test]
fn selected_manifest_requires_asset_dependencies_only_for_resource_components() {
    let mut host = HostRuntime::new();
    let raw = raw_world(&mut host);
    let ready = resource_world(&mut host);
    let entity = create(&mut host, raw, Vec::new());

    for component in [
        ComponentValue::CANVAS_TEXT,
        ComponentValue::CANVAS_GLYPH_RUN,
        ComponentValue::CANVAS_DRAWING,
        ComponentValue::CANVAS_BITMAP,
    ] {
        assert!(
            host.world_mut(ready)
                .unwrap()
                .manifest()
                .supports_component(component)
        );
        assert!(
            !host
                .world_mut(raw)
                .unwrap()
                .manifest()
                .supports_component(component)
        );
        let outcome = submit(
            &mut host,
            raw,
            vec![Command::InsertComponent {
                entity: EntityRef::Handle(entity),
                component,
                fields: Vec::new(),
                adopt: false,
            }],
        );
        assert_eq!(
            outcome.result.unwrap_err().reason,
            ErrorReason::UnsupportedDependency
        );
    }

    for component in [ComponentValue::CANVAS_STYLE, ComponentValue::CANVAS_BOX] {
        assert!(
            host.world_mut(raw)
                .unwrap()
                .manifest()
                .supports_component(component)
        );
    }

    let (selected, selected_entity) = canvas(&mut host, raw);
    shape(&mut host, raw, selected_entity, CanvasStyle::default());
    frame(&mut host);
    assert_eq!(publication(&host, selected).1.entries.len(), 1);
    assert!(host.take_resource_requests().is_empty());
}

#[test]
fn ordinary_text_and_glyph_components_acquire_fonts_and_publish_after_readiness() {
    let mut host = HostRuntime::new();
    host.register_stream_resource_provider("canvas-demand")
        .unwrap();
    let world = resource_world(&mut host);
    let (selected, selected_entity) = canvas(&mut host, world);
    shape(&mut host, world, selected_entity, CanvasStyle::default());
    let source = AssetSource {
        kind: FONT_TYPE,
        uri: "canvas-demand:///body.ippf".into(),
        variant: 7,
    };
    let text = create(
        &mut host,
        world,
        vec![ComponentValue::CanvasText(CanvasText {
            text: "AA".into(),
            source: source.uri.clone(),
            variant: source.variant,
            font_size: 10.0,
        })],
    );
    place(&mut host, world, text, selected_entity, None);
    let mut glyphs = ipp_core::components::rows::Rows::new();
    glyphs
        .push(CanvasGlyphRow {
            glyph_id: 1,
            position: [20.0, 30.0],
            color: None,
        })
        .unwrap();
    let run = create(
        &mut host,
        world,
        vec![ComponentValue::CanvasGlyphRun(CanvasGlyphRun {
            source: source.uri.clone(),
            variant: source.variant,
            font_size: 12.0,
            glyphs,
        })],
    );
    place(&mut host, world, run, selected_entity, None);

    frame(&mut host);
    let (_, pending) = publication(&host, selected);
    assert_eq!(pending.entries.len(), 1);
    assert_eq!(pending.resources().count(), 0);
    let mut requests = Vec::new();
    for _ in 0..8 {
        frame(&mut host);
        requests.extend(host.take_resource_requests());
        if !requests.is_empty() {
            break;
        }
    }
    assert_eq!(
        requests.len(),
        1,
        "both authored consumers must share one source demand"
    );
    let request = &requests[0];
    assert_eq!(request.kind, FONT_TYPE);
    assert_eq!(request.source, source.uri);
    assert_eq!(request.variant, source.variant);
    host.complete_resource(request.id, Ok(font_bytes()))
        .unwrap();

    for _ in 0..8 {
        frame(&mut host);
        if publication(&host, selected).1.entries.len() == 3 {
            break;
        }
    }
    let (_, ready) = publication(&host, selected);
    assert_eq!(ready.entries.len(), 3);
    assert!(Arc::ptr_eq(&pending.entries[0], &ready.entries[0]));
    assert!(ready.paint_revision > pending.paint_revision);
    assert!(ready.resource_revision > pending.resource_revision);
    let key = host.asset_resources().find(&source).unwrap();
    assert_eq!(ready.resources().collect::<Vec<_>>(), [key]);
    let CanvasPrimitive::Glyphs {
        font,
        glyphs,
        ..
    } = primitive(&ready.entries[1])
    else {
        panic!("expected measured text");
    };
    assert_eq!(*font, key);
    assert_eq!(glyphs.len(), 2);
    assert_eq!(
        primitive(&ready.entries[1]).style().identity.target.entity,
        text
    );
    let CanvasPrimitive::Glyphs {
        font,
        glyphs,
        ..
    } = primitive(&ready.entries[2])
    else {
        panic!("expected authored glyph run");
    };
    assert_eq!(*font, key);
    assert_eq!(glyphs[0].position, [20.0, 30.0]);
    assert_eq!(
        primitive(&ready.entries[2]).style().identity.target.entity,
        run
    );
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), key)
            .is_some()
    );
    assert!(host.take_resource_requests().is_empty());
}

#[test]
fn measured_text_reuses_glyph_storage_and_host_retains_exact_source_lease() {
    let (mut host, failure) = support::world_failures::host_with_world_failures();
    let world = host
        .create_world(Default::default(), &select_with_failures(&[CANVAS_CONTENT]))
        .unwrap();
    let (selected, selected_entity) = canvas(&mut host, world);
    let source = AssetSource {
        kind: FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/17/1", world.0)),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), font_bytes())
        .unwrap();
    host.progress_assets();
    let key = host.asset_resources().find(&source).unwrap();
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::CanvasStyle(CanvasStyle::default()),
            ComponentValue::CanvasText(CanvasText {
                text: "AA".into(),
                source: source.uri.clone(),
                variant: 0,
                font_size: 10.0,
            }),
        ],
    );
    place(&mut host, world, entity, selected_entity, None);
    frame(&mut host);
    let (_, original) = publication(&host, selected);
    let CanvasPrimitive::Glyphs {
        glyphs,
        font,
        ..
    } = primitive(&original.entries[0])
    else {
        panic!("expected glyphs")
    };
    assert_eq!(*font, key);
    assert_eq!(glyphs.len(), 2);
    assert_eq!(glyphs[0].glyph_id, 1);
    assert!((glyphs[1].position[0] - glyphs[0].position[0] - 6.0).abs() < 0.0001);
    scalar(
        &mut host,
        world,
        entity,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, x),
        25.0,
    );
    frame(&mut host);
    let (_, moved) = publication(&host, selected);
    let CanvasPrimitive::Glyphs {
        glyphs: retained,
        ..
    } = primitive(&moved.entries[0])
    else {
        panic!("expected glyphs")
    };
    assert!(Arc::ptr_eq(glyphs, retained));
    let completed = host.latest_publication(world).unwrap();
    assert!(host.publication_resource(completed, key).is_some());

    // A failed publication retains the completed one after the World drops its text.
    failure.fail_publication(Some(world));
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::CANVAS_TEXT,
        }],
    );
    assert_eq!(host.latest_publication(world), Some(completed));
    host.asset_resources_mut()
        .release_client_source(world, &source);
    host.flush_resource_lifecycle();
    assert!(host.publication_resource(completed, key).is_some());
    host.asset_resources_mut().invalidate_graphics(key);
    host.flush_resource_lifecycle();
    assert!(host.publication_resource(completed, key).is_some());
    host.asset_resources_mut().revoke_resource(key);
    host.flush_resource_lifecycle();
    assert!(host.publication_resource(completed, key).is_none());
    failure.fail_publication(None);
    frame(&mut host);
    assert!(publication(&host, selected).1.entries.is_empty());
}
