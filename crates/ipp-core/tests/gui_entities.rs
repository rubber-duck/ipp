//! Ordinary entity layout and Canvas production through real headless Host frames.
#![cfg(feature = "gui")]

mod support;

use ipp_core::components::{GuiLayout, Surface};
use ipp_core::systems::canvas::{
    CanvasBox, CanvasGlyph, CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasStyle,
    CanvasSystem, CanvasText,
};
use ipp_core::systems::gui::{GuiLayoutSystem, GuiSystem};
use ipp_core::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use support::CanvasTestHost;
use support::selection::{ATTACHMENTS, CANVAS, CANVAS_CONTENT, GUI_LAYOUT, SURFACE, select};

type Observations = Arc<Mutex<BTreeMap<WorldId, WorldFrameContext>>>;
struct Probe(Observations);
struct ProbeFactory(Observations);

const PROBE: systems::SystemId = systems::SystemId("fixture.gui-entity-frame");

/// The named parts plus the frame probe, registered after every compiled System.
fn probed(parts: &[&[systems::SystemId]]) -> Vec<systems::SystemId> {
    let mut selected = select(parts);
    selected.push(PROBE);
    selected
}

impl systems::SystemFactory for ProbeFactory {
    fn id(&self) -> systems::SystemId {
        PROBE
    }

    fn create(
        &self,
        _: &mut systems::SystemInitContext<'_>,
    ) -> Result<Box<dyn systems::System>, systems::SystemInitError> {
        Ok(Box::new(Probe(self.0.clone())))
    }
}

impl systems::System for Probe {
    fn update(&mut self, context: &mut systems::SystemUpdateContext<'_, '_>) {
        if let Some(frame) = context.world.frame_context() {
            self.0
                .lock()
                .unwrap()
                .insert(context.world.id(), frame.clone());
        }
    }
}

fn host() -> (HostRuntime, Observations) {
    let observed = Arc::new(Mutex::new(BTreeMap::new()));
    let mut factories = systems::compiled_system_factories();
    factories.push(Arc::new(ProbeFactory(observed.clone())));
    (
        HostRuntime::with_system_factories(factories).unwrap(),
        observed,
    )
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
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

fn create(host: &mut HostRuntime, world: WorldId, components: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        components
            .into_iter()
            .map(|component| Command::insert_value(EntityRef::Alias(1), component)),
    );
    apply(host, world, operations).result.unwrap()[0].1
}

fn place(host: &mut HostRuntime, world: WorldId, entity: EntityId, parent: EntityId) {
    apply(
        host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    )
    .result
    .unwrap();
}

fn scalar(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    component: u16,
    offset: usize,
    value: f32,
) -> BatchOutcome {
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
    )
}

/// The canvas of `world` at 300 x 200 logical units and 100 units per metre,
/// with a top-level root optionally carrying `layout`.
fn canvas(
    host: &mut HostRuntime,
    world: WorldId,
    layout: Option<GuiLayout>,
) -> (OutputRef, EntityId) {
    let root = create(
        host,
        world,
        layout.map(ComponentValue::GuiLayout).into_iter().collect(),
    );
    let world = host.world_ref(world).unwrap();
    (host.canvas_output(world, [300.0, 200.0], 100.0), root)
}

fn shape(
    host: &mut HostRuntime,
    world: WorldId,
    parent: EntityId,
    layout: Option<GuiLayout>,
) -> EntityId {
    let mut components = vec![ComponentValue::CanvasBox(CanvasBox {
        width: 20.0,
        height: 10.0,
        ..Default::default()
    })];
    components.extend(layout.map(ComponentValue::GuiLayout));
    let entity = create(host, world, components);
    place(host, world, entity, parent);
    entity
}

fn frame(host: &mut HostRuntime) {
    let result = host.frame(0.125).unwrap();
    assert!(
        result.worlds.values().all(Result::is_ok),
        "{:?}",
        result.worlds
    );
    assert!(
        result.publication_errors.is_empty(),
        "{:?}",
        result.publication_errors
    );
}

fn output(host: &HostRuntime, selection: OutputRef) -> CanvasPublication {
    host.output(
        host.latest_publication(selection.world().id()).unwrap(),
        selection,
    )
    .unwrap()
    .data::<CanvasPublication>()
    .unwrap()
    .clone()
}

fn box_geometry(entry: &CanvasPaintEntry) -> (EntityId, [f32; 2], [f32; 2]) {
    let CanvasPaintEntry::Primitive {
        primitive:
            CanvasPrimitive::Box {
                style,
                size,
                ..
            },
        ..
    } = entry
    else {
        panic!("expected box")
    };
    (style.identity.target.entity, style.position, *size)
}

#[test]
fn ordinary_row_measures_fixed_before_flex_but_paints_in_core_order_without_transform() {
    let mut host = HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[GuiSystem::ID, GuiLayoutSystem::ID, CanvasSystem::ID],
        )
        .unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        world,
        Some(GuiLayout {
            kind: 1,
            width: 100.0,
            height: 40.0,
            ..Default::default()
        }),
    );
    let flex = shape(
        &mut host,
        world,
        root_entity,
        Some(GuiLayout {
            flex: 1.0,
            ..Default::default()
        }),
    );
    let fixed = shape(&mut host, world, root_entity, None);
    frame(&mut host);
    let first = output(&host, root);
    assert_eq!(
        box_geometry(&first.entries[0]),
        (flex, [0.0, 0.0], [80.0, 10.0])
    );
    assert_eq!(
        box_geometry(&first.entries[1]),
        (fixed, [80.0, 0.0], [20.0, 10.0])
    );
    assert!(host.root_output(world).is_none());
    assert!(
        !host
            .world_manifest(world)
            .unwrap()
            .supports_component(ComponentValue::TRANSFORM)
    );
    frame(&mut host);
    assert!(Arc::ptr_eq(&first.entries, &output(&host, root).entries));
    scalar(
        &mut host,
        world,
        fixed,
        ComponentValue::CANVAS_BOX,
        std::mem::offset_of!(CanvasBox, width),
        40.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let changed = output(&host, root);
    assert_eq!(
        box_geometry(&changed.entries[0]),
        (flex, [0.0, 0.0], [60.0, 10.0])
    );
    assert_eq!(
        box_geometry(&changed.entries[1]),
        (fixed, [60.0, 0.0], [40.0, 10.0])
    );
}

#[test]
fn ordinary_container_alignment_and_margin_rules_use_parent_local_placements() {
    for (kind, expected) in [
        (2, [0.0, 0.0]),
        (3, [80.0, 30.0]),
        (4, [0.0, 0.0]),
        (5, [40.0, 15.0]),
        (6, [0.0, 0.0]),
    ] {
        let (mut host, _) = host();
        let world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
        let (root, root_entity) = canvas(
            &mut host,
            world,
            Some(GuiLayout {
                kind,
                width: 100.0,
                height: 40.0,
                ..Default::default()
            }),
        );
        let entity = shape(
            &mut host,
            world,
            root_entity,
            (kind == 3).then_some(GuiLayout {
                align_x: 1.0,
                align_y: 1.0,
                ..Default::default()
            }),
        );
        frame(&mut host);
        assert_eq!(
            box_geometry(&output(&host, root).entries[0]),
            (entity, expected, [20.0, 10.0])
        );
    }
}

#[test]
fn nested_surface_reflows_with_gui_layout_and_keeps_physical_child_constraints() {
    let (mut host, observed) = host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, GUI_LAYOUT, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[GUI_LAYOUT]))
        .unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        parent,
        Some(GuiLayout {
            kind: 1,
            width: 100.0,
            height: 40.0,
            clip: true,
            ..Default::default()
        }),
    );
    let (child_output, child_output_entity) = canvas(
        &mut host,
        child,
        Some(GuiLayout {
            kind: 1,
            ..Default::default()
        }),
    );
    shape(
        &mut host,
        child,
        child_output_entity,
        Some(GuiLayout {
            flex: 1.0,
            ..Default::default()
        }),
    );
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(root_entity),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 10.0,
                y: 5.0,
                scale_x: 2.0,
                scale_y: 0.5,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    let before = shape(&mut host, parent, root_entity, None);
    scalar(
        &mut host,
        parent,
        before,
        ComponentValue::CANVAS_BOX,
        std::mem::offset_of!(CanvasBox, width),
        40.0,
    )
    .result
    .unwrap();
    let surface = Surface {
        width: 2.0,
        ..Default::default()
    };
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
            ComponentValue::GuiLayout(GuiLayout {
                width: 30.0,
                height: 20.0,
                ..Default::default()
            }),
        ],
    );
    place(&mut host, parent, anchor, root_entity);
    frame(&mut host);
    let original = output(&host, root);
    let CanvasPaintEntry::Attachment(slot) = original.entries[1].as_ref() else {
        panic!("expected Surface slot")
    };
    assert_eq!(slot.to_canvas([-1.0, 0.5]), [90.0, 5.0]);
    assert_eq!(slot.to_canvas([1.0, -0.5]), [150.0, 15.0]);
    assert_eq!(slot.physical_extent, [2.0, 1.0]);
    assert_eq!(slot.clip, [10.0, 5.0, 210.0, 25.0]);
    assert_eq!(original.hits[0].bounds, [90.0, 5.0, 150.0, 15.0]);
    assert_eq!(original.hits[0].clip, slot.clip);
    assert_eq!(
        observed.lock().unwrap()[&child].placement,
        slot.parent_affine(original.logical_extent, original.units_per_metre)
    );
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([2.0, 1.0])
    );
    assert_eq!(output(&host, child_output).logical_extent, [200.0, 100.0]);
    let child_paint = output(&host, child_output);
    scalar(
        &mut host,
        parent,
        before,
        ComponentValue::CANVAS_BOX,
        std::mem::offset_of!(CanvasBox, width),
        60.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let moved = output(&host, root);
    let CanvasPaintEntry::Attachment(moved_slot) = moved.entries[1].as_ref() else {
        panic!("expected Surface slot")
    };
    assert_eq!(moved_slot.to_canvas([-1.0, 0.5]), [130.0, 5.0]);
    assert_eq!(moved_slot.token, slot.token);
    assert_eq!(moved_slot.physical_extent, slot.physical_extent);
    assert_eq!(output(&host, child_output).logical_extent, [200.0, 100.0]);
    assert!(Arc::ptr_eq(
        &child_paint.entries,
        &output(&host, child_output).entries
    ));
    assert_eq!(
        observed.lock().unwrap()[&child].placement,
        moved_slot.parent_affine(moved.logical_extent, moved.units_per_metre)
    );
}

#[test]
fn refused_layout_write_keeps_paint_and_attachments_unchanged() {
    let (mut host, _) = host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, GUI_LAYOUT, SURFACE]),
        )
        .unwrap();
    let child = host.create_world(Default::default(), CANVAS).unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        parent,
        Some(GuiLayout {
            kind: 1,
            width: 100.0,
            height: 40.0,
            ..Default::default()
        }),
    );
    let (child_output, _) = canvas(&mut host, child, None);
    shape(&mut host, parent, root_entity, None);
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
        ],
    );
    place(&mut host, parent, anchor, root_entity);
    frame(&mut host);
    let original = output(&host, root);
    let failed = scalar(
        &mut host,
        parent,
        root_entity,
        ComponentValue::GUI_LAYOUT,
        std::mem::offset_of!(GuiLayout, width),
        -2.0,
    );
    assert_eq!(failed.result.unwrap_err().reason, ErrorReason::InvalidValue);
    frame(&mut host);
    let unchanged = output(&host, root);
    assert!(Arc::ptr_eq(&unchanged.entries, &original.entries));
    assert_eq!(unchanged.hits, original.hits);
    assert_eq!(unchanged.paint_revision, original.paint_revision);
    assert!(
        !host
            .publication(host.latest_publication(parent).unwrap())
            .unwrap()
            .attachments
            .is_empty()
    );
    assert!(
        host.world_mut(parent)
            .unwrap()
            .gui_entity_layout(root_entity)
            .is_some_and(|layout| layout.available && layout.size == [100.0, 40.0])
    );
}

fn glyphs(entry: &CanvasPaintEntry) -> &Arc<[CanvasGlyph]> {
    let CanvasPaintEntry::Primitive {
        primitive: CanvasPrimitive::Glyphs {
            glyphs,
            ..
        },
        ..
    } = entry
    else {
        panic!("expected glyph run");
    };
    glyphs
}

#[test]
fn ordinary_text_demands_resources_and_reuses_measurement_until_constraints_change() {
    let mut host = HostRuntime::new();
    host.register_stream_resource_provider("gui-layout")
        .unwrap();
    let world = host
        .create_world(Default::default(), &select(&[CANVAS_CONTENT, GUI_LAYOUT]))
        .unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        world,
        Some(GuiLayout {
            kind: 1,
            width: 100.0,
            height: 40.0,
            ..Default::default()
        }),
    );
    let before = shape(&mut host, world, root_entity, None);
    let text = create(
        &mut host,
        world,
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "AA".into(),
                source: "gui-layout:///body.ippf".into(),
                variant: 0,
                font_size: 10.0,
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 12.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(Default::default()),
        ],
    );
    place(&mut host, world, text, root_entity);
    frame(&mut host);
    assert_eq!(output(&host, root).entries.len(), 1);
    let mut requests = Vec::new();
    for _ in 0..8 {
        requests.extend(host.take_resource_requests());
        if !requests.is_empty() {
            break;
        }
        frame(&mut host);
    }
    assert_eq!(requests.len(), 1);
    host.complete_resource(requests[0].id, Ok(support::canvas_font_bytes()))
        .unwrap();
    for _ in 0..8 {
        frame(&mut host);
        if output(&host, root).entries.len() == 2 {
            break;
        }
    }
    let ready = output(&host, root);
    assert_eq!(ready.entries.len(), 2);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .size,
        [12.0, 12.0]
    );
    assert_eq!(glyphs(&ready.entries[1]).len(), 2);
    frame(&mut host);
    assert!(Arc::ptr_eq(&ready.entries, &output(&host, root).entries));

    scalar(
        &mut host,
        world,
        before,
        ComponentValue::CANVAS_BOX,
        std::mem::offset_of!(CanvasBox, width),
        30.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let reflowed = output(&host, root);
    assert!(Arc::ptr_eq(
        glyphs(&ready.entries[1]),
        glyphs(&reflowed.entries[1])
    ));
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .origin,
        [30.0, 0.0]
    );
    scalar(
        &mut host,
        world,
        text,
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, x),
        9.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let translated = output(&host, root);
    assert!(Arc::ptr_eq(
        glyphs(&ready.entries[1]),
        glyphs(&translated.entries[1])
    ));
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .origin,
        [30.0, 0.0]
    );

    scalar(
        &mut host,
        world,
        text,
        ComponentValue::GUI_LAYOUT,
        std::mem::offset_of!(GuiLayout, width),
        6.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let wrapped = output(&host, root);
    assert!(!Arc::ptr_eq(
        glyphs(&ready.entries[1]),
        glyphs(&wrapped.entries[1])
    ));
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .size,
        [6.0, 24.0]
    );
    assert_eq!(
        glyphs(&wrapped.entries[1])[0].position[0],
        glyphs(&wrapped.entries[1])[1].position[0]
    );
    assert!(
        glyphs(&wrapped.entries[1])[1].position[1] > glyphs(&wrapped.entries[1])[0].position[1]
    );
    assert!(host.take_resource_requests().is_empty());
}

#[test]
fn core_reorder_and_layout_removal_reuse_leaf_identity_without_retaining_a_layout_boundary() {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        world,
        Some(GuiLayout {
            kind: 1,
            width: 100.0,
            height: 40.0,
            ..Default::default()
        }),
    );
    let first = shape(&mut host, world, root_entity, None);
    let second = shape(&mut host, world, root_entity, None);
    frame(&mut host);
    let original = output(&host, root);
    apply(
        &mut host,
        world,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(second),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root_entity)),
                before: Some(EntityRef::Handle(first)),
            },
        }],
    )
    .result
    .unwrap();
    frame(&mut host);
    let reordered = output(&host, root);
    assert_eq!(
        box_geometry(&reordered.entries[0]),
        (second, [0.0, 0.0], [20.0, 10.0])
    );
    assert_eq!(
        box_geometry(&reordered.entries[1]),
        (first, [20.0, 0.0], [20.0, 10.0])
    );
    assert_eq!(original.selection, reordered.selection);
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(root_entity),
            component: ComponentValue::GUI_LAYOUT,
        }],
    )
    .result
    .unwrap();
    frame(&mut host);
    let raw = output(&host, root);
    assert_eq!(
        box_geometry(&raw.entries[1]),
        (first, [0.0, 0.0], [20.0, 10.0])
    );
    assert!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(root_entity)
            .is_none()
    );
    assert!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(first)
            .is_none()
    );
}

#[test]
fn padding_offsets_own_leaf_content_but_preserves_outer_sizing_and_clip() {
    for explicit in [false, true] {
        let mut host = HostRuntime::new();
        let world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
        let (root, root_entity) = canvas(&mut host, world, None);
        let layout = GuiLayout {
            width: if explicit {
                30.0
            } else {
                -1.0
            },
            height: if explicit {
                20.0
            } else {
                -1.0
            },
            padding_left: 4.0,
            padding_top: 3.0,
            padding_right: 2.0,
            padding_bottom: 1.0,
            clip: true,
            ..Default::default()
        };
        let style = CanvasStyle {
            x: 10.0,
            y: 20.0,
            scale_x: 2.0,
            scale_y: 3.0,
            ..Default::default()
        };
        let leaf = shape(&mut host, world, root_entity, Some(layout));
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(leaf),
                ComponentValue::CanvasStyle(style),
            )],
        )
        .result
        .unwrap();
        frame(&mut host);
        let view = output(&host, root);
        let outer = if explicit {
            [30.0, 20.0]
        } else {
            [20.0, 10.0]
        };
        assert_eq!(box_geometry(&view.entries[0]), (leaf, [18.0, 29.0], outer));
        let CanvasPaintEntry::Primitive {
            primitive,
            ..
        } = view.entries[0].as_ref()
        else {
            unreachable!()
        };
        assert_eq!(
            primitive.style().clip,
            [10.0, 20.0, 10.0 + 2.0 * outer[0], 20.0 + 3.0 * outer[1]]
        );
        assert_eq!(
            host.world_mut(world)
                .unwrap()
                .gui_entity_layout(leaf)
                .unwrap()
                .size,
            outer
        );
    }
}

#[test]
fn padding_text_and_surface_hits_share_content_mapping_without_shifting_outer_clips() {
    let (mut host, observed) = host();
    let parent = host
        .create_world(
            Default::default(),
            &select(&[ATTACHMENTS, CANVAS_CONTENT, GUI_LAYOUT, SURFACE]),
        )
        .unwrap();
    let child = host
        .create_world(Default::default(), &probed(&[GUI_LAYOUT]))
        .unwrap();
    let (root, root_entity) = canvas(&mut host, parent, None);
    let (selected_child, _) = canvas(&mut host, child, None);
    let source = ipp_core::services::asset_management::AssetSource {
        kind: ipp_core::services::asset_management::font::FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/17/24", parent.0)),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(parent, source.clone(), support::canvas_font_bytes())
        .unwrap();
    let layout = GuiLayout {
        width: 30.0,
        height: 20.0,
        padding_left: 4.0,
        padding_top: 3.0,
        padding_right: 2.0,
        padding_bottom: 1.0,
        clip: true,
        ..Default::default()
    };
    let style = CanvasStyle {
        x: 10.0,
        y: 20.0,
        scale_x: 2.0,
        scale_y: 3.0,
        ..Default::default()
    };
    let text = create(
        &mut host,
        parent,
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "AA".into(),
                source: source.uri,
                variant: 0,
                font_size: 10.0,
            }),
            ComponentValue::GuiLayout(layout),
            ComponentValue::CanvasStyle(style),
        ],
    );
    place(&mut host, parent, text, root_entity);
    let anchor = create(
        &mut host,
        parent,
        vec![
            ComponentValue::Surface(Surface::default()),
            ComponentValue::WorldAttachment(WorldAttachment::surface(selected_child)),
            ComponentValue::GuiLayout(layout),
            ComponentValue::CanvasStyle(style),
        ],
    );
    place(&mut host, parent, anchor, root_entity);
    for _ in 0..8 {
        frame(&mut host);
        if output(&host, root).entries.len() == 2 {
            break;
        }
    }
    let view = output(&host, root);
    assert_eq!(view.entries.len(), 2);
    let CanvasPaintEntry::Primitive {
        primitive,
        ..
    } = view.entries[0].as_ref()
    else {
        panic!("expected text")
    };
    assert_eq!(primitive.style().position, [18.0, 29.0]);
    assert_eq!(primitive.style().clip, [10.0, 20.0, 70.0, 80.0]);
    assert_eq!(glyphs(&view.entries[0])[1].position[0], 6.0);
    assert_eq!(
        host.world_mut(parent)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .size,
        [30.0, 20.0]
    );
    let CanvasPaintEntry::Attachment(slot) = view.entries[1].as_ref() else {
        panic!("expected attachment")
    };
    assert_eq!(slot.to_canvas([-0.5, 0.5]), [18.0, 29.0]);
    assert_eq!(slot.to_canvas([0.5, -0.5]), [78.0, 89.0]);
    assert_eq!(slot.clip, primitive.style().clip);
    assert_eq!(view.hits.len(), 1);
    let hit = &view.hits[0];
    assert_eq!(hit.bounds, [18.0, 29.0, 78.0, 89.0]);
    assert_eq!(hit.position, [18.0, 29.0]);
    assert_eq!(hit.clip, slot.clip);
    assert!(hit.contains([18.0, 29.0]));
    assert!(!hit.contains([10.0, 20.0]));
    assert!(!hit.contains([75.0, 29.0]));
    assert_eq!(
        observed.lock().unwrap()[&child].placement,
        slot.parent_affine(view.logical_extent, view.units_per_metre)
    );
    assert_eq!(
        observed.lock().unwrap()[&child].surface_extent,
        Some([1.0, 1.0])
    );

    scalar(
        &mut host,
        parent,
        text,
        ComponentValue::GUI_LAYOUT,
        std::mem::offset_of!(GuiLayout, width),
        -1.0,
    )
    .result
    .unwrap();
    scalar(
        &mut host,
        parent,
        text,
        ComponentValue::GUI_LAYOUT,
        std::mem::offset_of!(GuiLayout, height),
        -1.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    assert_eq!(
        host.world_mut(parent)
            .unwrap()
            .gui_entity_layout(text)
            .unwrap()
            .size,
        [12.0, 12.0]
    );
}

#[test]
fn excessive_managed_layout_depth_is_observable_and_recovers_after_core_reparenting() {
    use ipp_core::systems::gui::layout::{GuiEntityLayoutDiagnostic, MAX_LAYOUT_DEPTH};
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default(), GUI_LAYOUT).unwrap();
    let (root, root_entity) = canvas(&mut host, world, Some(GuiLayout::default()));
    let mut parent = root_entity;
    let mut first_excluded = parent;
    for depth in 1..=MAX_LAYOUT_DEPTH + 1 {
        let entity = create(&mut host, world, Vec::new());
        place(&mut host, world, entity, parent);
        parent = entity;
        if depth == MAX_LAYOUT_DEPTH + 1 {
            first_excluded = entity;
        }
    }
    let leaf = shape(&mut host, world, parent, None);
    frame(&mut host);
    assert!(output(&host, root).entries.is_empty());
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout_diagnostics()
            .unwrap(),
        &[GuiEntityLayoutDiagnostic::DepthLimit {
            entity: first_excluded,
            limit: MAX_LAYOUT_DEPTH
        }]
    );
    assert!(
        !host
            .world_mut(world)
            .unwrap()
            .gui_entity_layout(first_excluded)
            .unwrap()
            .available
    );
    place(&mut host, world, first_excluded, root_entity);
    frame(&mut host);
    assert_eq!(box_geometry(&output(&host, root).entries[0]).0, leaf);
    assert!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout_diagnostics()
            .unwrap()
            .is_empty()
    );
}

#[cfg(feature = "diagnostics")]
#[test]
fn counters_identify_full_dirty_reflow_and_retain_text_runs() {
    use ipp_core::systems::gui::layout::GuiEntityLayoutWork;
    let mut host = HostRuntime::new();
    let world = host
        .create_world(Default::default(), &select(&[CANVAS_CONTENT, GUI_LAYOUT]))
        .unwrap();
    let (root, root_entity) = canvas(
        &mut host,
        world,
        Some(GuiLayout {
            kind: 1,
            ..Default::default()
        }),
    );
    let source = ipp_core::services::asset_management::AssetSource {
        kind: ipp_core::services::asset_management::font::FONT_TYPE,
        uri: std::sync::Arc::<str>::from(format!("producer://{}/17/25", world.0)),
        variant: 0,
    };
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), support::canvas_font_bytes())
        .unwrap();
    let mut texts = Vec::new();
    for _ in 0..2 {
        let text = create(
            &mut host,
            world,
            vec![
                ComponentValue::CanvasText(CanvasText {
                    text: "AA".into(),
                    source: source.uri.clone(),
                    variant: 0,
                    font_size: 10.0,
                }),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 50.0,
                    ..Default::default()
                }),
                ComponentValue::CanvasStyle(Default::default()),
            ],
        );
        place(&mut host, world, text, root_entity);
        texts.push(text);
    }
    for _ in 0..8 {
        frame(&mut host);
    }
    let initial = host
        .world_mut(world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    assert_eq!(initial.latest, GuiEntityLayoutWork::default());
    let before = output(&host, root);
    scalar(
        &mut host,
        world,
        texts[0],
        ComponentValue::CANVAS_STYLE,
        std::mem::offset_of!(CanvasStyle, x),
        5.0,
    )
    .result
    .unwrap();
    frame(&mut host);
    let visual = host
        .world_mut(world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    assert_eq!(visual.latest, GuiEntityLayoutWork::default());
    assert_eq!(visual.total, initial.total);
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(texts[0]),
            component: ComponentValue::CANVAS_TEXT,
            field: FieldWrite {
                offset: std::mem::offset_of!(CanvasText, text) as u32,
                value: FieldValue::String("AAA".into()),
            },
        }],
    )
    .result
    .unwrap();

    // The applying frame already evaluated the content edit.
    let content = host
        .world_mut(world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap();
    assert_eq!(
        content.latest,
        GuiEntityLayoutWork {
            reflows: 1,
            visited_entities: 3,
            text_measurements: 1,
            reused_texts: 1
        }
    );
    assert!(Arc::ptr_eq(
        glyphs(&before.entries[1]),
        glyphs(&output(&host, root).entries[1])
    ));
    println!(
        "ordinary-layout-work visual={:?} content={:?}",
        visual.latest, content.latest
    );
}
