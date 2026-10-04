//! Real headless control paint and same-publication semantic/hit joins; no transport admission claim.

mod support;
use support::task_scheduler::HostTaskTestDriver;

#[path = "support/gui_interaction.rs"]
mod interaction;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiLayout, GuiSlider, GuiTextInput,
};
use ipp_core::systems::canvas::{
    CanvasHitKind, CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication,
    CanvasShapeFill, CanvasStyle, CanvasSystem,
};
use ipp_core::systems::gui::GuiPrimitivePart;
use ipp_core::systems::gui::local::{GuiControlKind, GuiLocalCommand};
use ipp_core::systems::gui::presentation::{
    GuiCanvasPublication, GuiFont, GuiPaintPart, GuiRoutingValue, GuiSkin, GuiTheme,
};
use ipp_core::systems::gui::{GuiPartId, GuiPartProperty, GuiPartVariant, GuiSkinState, GuiSystem};
use ipp_core::*;
use std::sync::Arc;
use support::CanvasTestHost;
use support::gui_panel::{ControlValue, read_control, replacement};
use support::selection::{ASSETS, GUI_LAYOUT, select};

use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputService, GuiInputSession,
};
use ipp_core::systems::gui::local::GuiLocalEffect;

struct ControlHost {
    next_request: u64,
    host: HostRuntime,
    input: GuiInputService,
    session: GuiInputSession,
}

impl Default for ControlHost {
    fn default() -> Self {
        let input = GuiInputService::default();
        let session = input.open_session().unwrap();
        Self {
            next_request: 0,
            host: crate::support::task_scheduler::host(),
            input,
            session,
        }
    }
}

impl std::ops::Deref for ControlHost {
    type Target = HostRuntime;

    fn deref(&self) -> &HostRuntime {
        &self.host
    }
}

impl std::ops::DerefMut for ControlHost {
    fn deref_mut(&mut self) -> &mut HostRuntime {
        &mut self.host
    }
}

struct AppliedPermit;

impl GuiDeliveryPermit for AppliedPermit {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        assert!(matches!(
            terminal,
            GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
        ));
    }
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame_for_test(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    values: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations).result.unwrap()[0].1
}

fn fixture() -> (ControlHost, WorldId, OutputRef) {
    let mut host = ControlHost::default();
    let world = host
        .create_world(Default::default(), &select(&[ASSETS, GUI_LAYOUT]))
        .unwrap();
    create(
        &mut host,
        world,
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 1,
            ..Default::default()
        })],
        None,
    );
    let world_ref = host.world_ref(world).unwrap();
    let selection = host.canvas_output(world_ref, [300.0, 100.0], 100.0);
    (host, world, selection)
}

fn frame(host: &mut HostRuntime) {
    let frame = host.frame_for_test(0.1).unwrap();
    assert!(
        frame.worlds.values().all(Result::is_ok),
        "{:?}",
        frame.worlds
    );
    assert!(
        frame.publication_errors.is_empty(),
        "{:?}",
        frame.publication_errors
    );
}

fn output(host: &HostRuntime, selection: OutputRef) -> (CanvasPublication, GuiCanvasPublication) {
    let publication = host
        .publication(host.latest_publication(selection.world().id()).unwrap())
        .unwrap();
    (
        publication
            .output(selection)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone(),
        publication
            .chunk(CanvasSystem::ID)
            .unwrap()
            .data::<GuiCanvasPublication>()
            .unwrap()
            .clone(),
    )
}

fn control(
    host: &mut HostRuntime,
    world: WorldId,
    root: OutputRef,
    value: ComponentValue,
) -> EntityId {
    let parent = support::top_level_root(host, root.world().id());
    create(
        host,
        world,
        vec![
            value,
            ComponentValue::GuiLayout(GuiLayout {
                width: 50.0,
                height: 20.0,
                ..Default::default()
            }),
        ],
        Some(parent),
    )
}

/// Replace a control's value field the way a client does: compare-and-set
/// against the value it last read.
fn replace(host: &mut ControlHost, world: WorldId, entity: EntityId, value: ControlValue) {
    let read = read_control(host, world, entity).unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 7,
            operations: vec![replacement(&read, value)],
        })
        .unwrap();
}

#[test]
fn headless_gui_manifest_supports_field_reads_gui_action_and_replacement_without_layout() {
    use ipp_core::systems::WorldOperation;
    use ipp_core::systems::gui::local::GuiLocalAction;

    let mut host = ControlHost::default();
    // Controls require CanvasBounds, so the smallest GUI World selects Canvas.
    let world = host
        .create_world(Default::default(), &[CanvasSystem::ID, GuiSystem::ID])
        .unwrap();
    assert!(
        host.world_manifest(world)
            .unwrap()
            .supports_operation(WorldOperation::Gui)
    );
    let entity = create(
        &mut host,
        world,
        vec![ComponentValue::GuiCheckbox(GuiCheckbox::default())],
        None,
    );
    let before = read_control(&mut host, world, entity).unwrap();
    assert!(before.available && before.enabled && before.visible);
    assert_eq!(before.value, ControlValue::Bool(false));
    assert!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout(entity)
            .is_none()
    );
    assert!(host.root_output(world).is_none());
    assert!(
        host.input
            .bind_context(&host.host, &host.session, before.target.world)
            .is_err()
    );
    let toggled = apply(
        &mut host,
        world,
        vec![Command::GuiAction {
            target: GuiActionTarget {
                entity: EntityRef::Handle(entity),
                component: before.target.component,
                incarnation: before.target.incarnation,
            },
            action: GuiLocalAction::Toggle,
        }],
    );
    assert!(toggled.result.is_ok(), "{toggled:?}");
    frame(&mut host);
    let after = read_control(&mut host, world, entity).unwrap();
    assert_eq!(after.value, ControlValue::Bool(true));
    assert_eq!(after.target, before.target);
    assert!(!after.focused);
    replace(&mut host, world, entity, ControlValue::Bool(false));
    frame(&mut host);
    let replaced = read_control(&mut host, world, entity).unwrap();
    assert_eq!(replaced.value, ControlValue::Bool(false));
    assert_eq!(replaced.target, before.target);
    assert!(!replaced.focused);

    // A replacement that read a value since superseded changes nothing.
    let outcome = apply(
        &mut host,
        world,
        vec![Command::set_field_if(
            EntityRef::Handle(entity),
            ComponentValue::GUI_CHECKBOX,
            FieldWrite {
                offset: std::mem::offset_of!(GuiCheckbox, checked) as u32,
                value: FieldValue::Bool(false),
            },
            FieldValue::Bool(true),
        )],
    );
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ErrorReason::ValueMismatch
    );
    assert_eq!(
        read_control(&mut host, world, entity).unwrap().value,
        ControlValue::Bool(false)
    );
    assert!(host.root_output(world).is_none());
}

fn primitive(entry: &CanvasPaintEntry) -> &CanvasPrimitive {
    let CanvasPaintEntry::Primitive {
        primitive,
        ..
    } = entry
    else {
        panic!("expected control primitive")
    };
    primitive
}

#[test]
fn all_control_roles_publish_exact_immutable_hits_and_semantics_without_a_presented_root() {
    let (mut host, world, root) = fixture();
    let controls = [
        ComponentValue::GuiButton(GuiButton::default()),
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        ComponentValue::GuiSlider(GuiSlider::default()),
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    ]
    .map(|value| control(&mut host, world, root, value));
    frame(&mut host);
    let (canvas, semantics) = output(&host, root);
    assert!(host.root_output(world).is_none());
    let view = &semantics.views[&root];
    assert_eq!(view.input_revision, canvas.input_revision);
    assert_eq!(canvas.hits.len(), 4);
    assert_eq!(view.controls.len(), 4);
    for (index, role) in [
        GuiControlKind::Button,
        GuiControlKind::Checkbox,
        GuiControlKind::Slider,
        GuiControlKind::TextInput,
    ]
    .into_iter()
    .enumerate()
    {
        let hit = &canvas.hits[index];
        assert_eq!(hit.kind, CanvasHitKind::Entity);
        let observed = view.control(hit.target).unwrap();
        assert_eq!(observed.hit, *hit);
        assert_eq!(observed.record.target.entity, controls[index]);
        assert_eq!(observed.record.target.world, root.world());
        assert_eq!(observed.record.kind, role);
        assert_eq!(
            observed.record.ancestry.as_ref(),
            &[support::top_level_root(&mut host, world), controls[index]]
        );
        let read = read_control(&mut host, world, controls[index]).unwrap();
        assert_eq!(read.target, observed.record.target);
        assert_eq!(read.ancestry, observed.record.ancestry);
        assert_eq!(
            hit.bounds,
            [index as f32 * 50.0, 0.0, (index + 1) as f32 * 50.0, 20.0]
        );
        assert!(hit.eligible && observed.available);
        assert_eq!(observed.supported_actions().len(), 3);
        assert!(
            observed
                .supported_actions()
                .contains(&ipp_core::systems::gui::presentation::GuiSemanticActionKind::Blur)
        );
    }
    frame(&mut host);
    let (unchanged, semantic_unchanged) = output(&host, root);
    assert!(Arc::ptr_eq(&canvas.entries, &unchanged.entries));
    assert!(Arc::ptr_eq(&semantics.views, &semantic_unchanged.views));
}

#[test]
fn value_replacement_updates_visual_parts_and_input_revision_not_layout_or_prior_publications() {
    let (mut host, world, root) = fixture();
    let checkbox = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let slider = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiSlider(GuiSlider::default()),
    );
    frame(&mut host);
    let (before, semantics) = output(&host, root);
    replace(&mut host, world, checkbox, ControlValue::Bool(true));
    replace(&mut host, world, slider, ControlValue::Scalar(0.75));
    frame(&mut host);
    let (after, new_semantics) = output(&host, root);
    assert_eq!(before.layout_revision, after.layout_revision);
    assert!(
        after.input_revision > before.input_revision
            && after.paint_revision > before.paint_revision
    );
    assert_eq!(
        read_control(&mut host, world, checkbox).unwrap().value,
        ControlValue::Bool(true)
    );
    assert_eq!(
        read_control(&mut host, world, slider).unwrap().value,
        ControlValue::Scalar(0.75)
    );
    // The earlier publication keeps the values it was evaluated with.
    assert_eq!(
        semantics.views[&root].controls[1].record.value,
        GuiRoutingValue::Scalar(0.0)
    );
    assert_eq!(
        new_semantics.views[&root].controls[1].record.value,
        GuiRoutingValue::Scalar(0.75)
    );
    let checked_icon = |publication: &CanvasPublication| {
        publication.entries.iter().any(|entry| {
            primitive(entry).style().identity.target.entity == checkbox
                && primitive(entry).style().identity.part == CanvasPart::Icon
        })
    };
    assert!(!checked_icon(&before));
    assert!(checked_icon(&after));
    assert!(
        after
            .entries
            .iter()
            .any(
                |entry| primitive(entry).style().identity.target.entity == checkbox
                    && primitive(entry).style().identity.part == CanvasPart::Icon
            )
    );
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .latest
            .reflows,
        0
    );
    // The checked box repaints its default look's checked fill; the slider's
    // rail, unchanged by its value, keeps its shared entry.
    let entry = |publication: &CanvasPublication, entity, part| {
        publication
            .entries
            .iter()
            .find(|entry| {
                primitive(entry).style().identity.target.entity == entity
                    && primitive(entry).style().identity.part == part
            })
            .unwrap()
            .clone()
    };
    assert!(!Arc::ptr_eq(
        &entry(&before, checkbox, CanvasPart::Background),
        &entry(&after, checkbox, CanvasPart::Background)
    ));
    assert!(Arc::ptr_eq(
        &entry(&before, slider, CanvasPart::Background),
        &entry(&after, slider, CanvasPart::Background)
    ));
}

fn part(identity: GuiPartId, color: [f32; 4]) -> GuiPaintPart {
    GuiPaintPart {
        color: Some(color),
        ..GuiPaintPart::keyed(identity).unwrap()
    }
}

#[test]
fn shared_theme_variants_and_sparse_overrides_preserve_control_and_part_identity() {
    let (mut host, world, root) = fixture();
    let mut parts = Rows::new();
    let base = parts
        .push(part(
            GuiPartId::base(GuiPrimitivePart::Background),
            [1.0, 0.0, 0.0, 1.0],
        ))
        .unwrap();
    parts
        .push(part(
            GuiPartId::variant(
                GuiPrimitivePart::Background,
                GuiSkinState::Idle,
                GuiPartVariant::Checked,
            ),
            [0.0, 1.0, 0.0, 1.0],
        ))
        .unwrap();
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })],
        None,
    );
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (original, semantics) = output(&host, root);
    let identity = primitive(&original.entries[0]).style().identity;
    let CanvasPrimitive::Box {
        fill,
        ..
    } = primitive(&original.entries[0])
    else {
        unreachable!()
    };
    assert_eq!(*fill, CanvasShapeFill::Solid([1.0, 0.0, 0.0, 1.0]));
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(theme),
            component: ComponentValue::GUI_THEME,
            field: FieldWrite {
                offset: Rows::<GuiPaintPart>::offset(0, base, GuiPartProperty::Color.index())
                    .unwrap(),
                value: FieldValue::Dynamic(DynamicValue::Vec4([0.0, 0.0, 1.0, 1.0])),
            },
        }],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (restyled, semantic_restyled) = output(&host, root);
    assert_eq!(primitive(&restyled.entries[0]).style().identity, identity);
    assert_eq!(restyled.layout_revision, original.layout_revision);
    assert_eq!(restyled.input_revision, original.input_revision);
    assert!(Arc::ptr_eq(
        &semantics.views[&root].controls[0],
        &semantic_restyled.views[&root].controls[0]
    ));
    replace(&mut host, world, entity, ControlValue::Bool(true));
    frame(&mut host);
    let (checked, checked_semantics) = output(&host, root);
    assert_eq!(
        read_control(&mut host, world, entity).unwrap().value,
        ControlValue::Bool(true)
    );
    let record = &checked_semantics.views[&root].controls[0].record;
    let CanvasPrimitive::Box {
        fill,
        ..
    } = primitive(&checked.entries[0])
    else {
        unreachable!()
    };
    assert_eq!(*fill, CanvasShapeFill::Solid([0.0, 1.0, 0.0, 1.0]));
    let mut overrides = Rows::new();
    overrides
        .push(part(
            GuiPartId::base(GuiPrimitivePart::Background),
            [0.5, 0.5, 0.5, 1.0],
        ))
        .unwrap();
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                parts: overrides,
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (overridden, overridden_semantics) = output(&host, root);
    assert_eq!(
        record,
        &overridden_semantics.views[&root].controls[0].record
    );
    assert_eq!(
        read_control(&mut host, world, entity).unwrap().value,
        ControlValue::Bool(true)
    );
    let CanvasPrimitive::Box {
        fill,
        ..
    } = primitive(&overridden.entries[0])
    else {
        unreachable!()
    };
    assert_eq!(*fill, CanvasShapeFill::Solid([0.5, 0.5, 0.5, 1.0]));
    assert_eq!(primitive(&overridden.entries[0]).style().identity, identity);
}

#[test]
fn labels_use_ordinary_font_demand_and_retain_glyphs_across_visual_only_changes() {
    let (mut host, world, root) = fixture();
    host.register_stream_resource_provider("gui-font").unwrap();
    let top = support::top_level_root(&mut host, world);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(top),
            ComponentValue::GuiFont(GuiFont {
                source: "gui-font:///body.ippf".into(),
                variant: 0,
                font_size: 10.0,
            }),
        )],
    )
    .result
    .unwrap();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiButton(GuiButton {
            label: "AA".into(),
            ..Default::default()
        }),
    );
    frame(&mut host);
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
        if output(&host, root).0.entries.len() == 2 {
            break;
        }
    }
    let (before, _) = output(&host, root);
    let CanvasPrimitive::Glyphs {
        glyphs,
        font,
        ..
    } = primitive(&before.entries[1])
    else {
        panic!("expected label")
    };
    assert_eq!(glyphs.len(), 2);
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), *font)
            .is_some()
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 9.0,
                scale_x: 2.0,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (after, _) = output(&host, root);
    let CanvasPrimitive::Glyphs {
        glyphs: after_glyphs,
        ..
    } = primitive(&after.entries[1])
    else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(glyphs, after_glyphs));
    assert_eq!(after.hits[0].position, [9.0, 0.0]);
    assert_eq!(after.hits[0].scale, [2.0, 1.0]);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .latest
            .reflows,
        0
    );
    let top = support::top_level_root(&mut host, world);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(top),
            ComponentValue::GuiFont(GuiFont {
                source: "gui-font:///pending.ippf".into(),
                variant: 0,
                font_size: 10.0,
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (pending, _) = output(&host, root);
    let CanvasPrimitive::Glyphs {
        glyphs: pending_glyphs,
        font: pending_font,
        ..
    } = primitive(&pending.entries[1])
    else {
        panic!("retained font")
    };
    assert_eq!(pending_font, font);
    assert!(Arc::ptr_eq(glyphs, pending_glyphs));
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), *font)
            .is_some()
    );
}

#[test]
fn checkbox_base_icon_is_checked_only_while_plain_button_icon_stays_visible() {
    let (mut host, world, root) = fixture();
    let mut parts = Rows::new();
    parts
        .push(part(GuiPartId::base(GuiPrimitivePart::Icon), [1.0; 4]))
        .unwrap();
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })],
        None,
    );
    let entities = [
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        ComponentValue::GuiButton(GuiButton::default()),
    ]
    .map(|value| {
        let entity = control(&mut host, world, root, value);
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    ..Default::default()
                }),
            )],
        )
        .result
        .unwrap();
        entity
    });
    frame(&mut host);
    let (canvas, _) = output(&host, root);
    assert_eq!(canvas.entries.len(), 3);
    assert_eq!(canvas.hits.len(), 2);
    assert_eq!(host.world_mut(world).unwrap().entities().len(), 4);
    assert_eq!(
        read_control(&mut host, world, entities[0]).unwrap().value,
        ControlValue::Bool(false)
    );
    assert!(control_part(&canvas, entities[0], CanvasPart::Icon).is_none());
    assert!(control_part(&canvas, entities[1], CanvasPart::Icon).is_some());
    replace(&mut host, world, entities[0], ControlValue::Bool(true));
    frame(&mut host);
    let (checked, _) = output(&host, root);
    let CanvasPrimitive::Box {
        fill,
        ..
    } = control_part(&checked, entities[0], CanvasPart::Icon).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(*fill, CanvasShapeFill::Solid([1.0; 4]));
    assert_eq!(checked.entries.len(), 4);
    replace(&mut host, world, entities[0], ControlValue::Bool(false));
    frame(&mut host);
    let (unchecked, _) = output(&host, root);
    assert!(control_part(&unchecked, entities[0], CanvasPart::Icon).is_none());
    assert!(control_part(&unchecked, entities[1], CanvasPart::Icon).is_some());
}

fn control_part(
    canvas: &CanvasPublication,
    entity: EntityId,
    part: CanvasPart,
) -> Option<&CanvasPrimitive> {
    canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .find(|primitive| {
            let identity = primitive.style().identity;
            identity.target.entity == entity && identity.part == part
        })
}

#[test]
fn checkbox_unchecked_variant_requires_exact_state_and_accepts_color_or_opacity() {
    for (state, opacity_only) in [
        (GuiSkinState::Idle, false),
        (GuiSkinState::Idle, true),
        (GuiSkinState::Disabled, false),
        (GuiSkinState::Disabled, true),
    ] {
        let (mut host, world, root) = fixture();
        let mut parts = Rows::new();
        let mut icon = GuiPaintPart::keyed(GuiPartId::variant(
            GuiPrimitivePart::Icon,
            state,
            GuiPartVariant::Unchecked,
        ))
        .unwrap();
        if opacity_only {
            icon.opacity = Some(0.35);
        } else {
            icon.color = Some([0.3, 0.7, 0.2, 1.0]);
        }
        parts.push(icon).unwrap();
        let theme = create(
            &mut host,
            world,
            vec![ComponentValue::GuiTheme(GuiTheme {
                parts,
                ..Default::default()
            })],
            None,
        );
        let entity = control(
            &mut host,
            world,
            root,
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        );
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    ..Default::default()
                }),
            )],
        )
        .result
        .unwrap();
        frame(&mut host);
        let (canvas, _) = output(&host, root);
        let icon = control_part(&canvas, entity, CanvasPart::Icon);
        if state == GuiSkinState::Idle {
            let CanvasPrimitive::Box {
                style,
                fill,
                ..
            } = icon.expect("explicit matching unchecked appearance must paint")
            else {
                unreachable!()
            };
            assert_eq!(
                style.opacity,
                if opacity_only {
                    0.35
                } else {
                    1.0
                }
            );
            // An opacity-only row fades the default look's dark check mark
            // (sRGB #00131c).
            assert_eq!(
                *fill,
                CanvasShapeFill::Solid(if opacity_only {
                    [0.0, 0.006_512_090_6, 0.011_612_245, 1.0]
                } else {
                    [0.3, 0.7, 0.2, 1.0]
                })
            );
        } else {
            assert!(
                icon.is_none(),
                "a disabled-only variant cannot activate an idle unchecked icon"
            );
        }
    }
}

#[test]
fn checkbox_unchecked_asset_requires_an_explicit_variant_not_a_ready_base_asset() {
    let (mut host, world, root) = fixture();
    host.register_stream_resource_provider("gui-icon").unwrap();
    let asset = ipp_core::services::asset_management::AssetSource {
        kind: ipp_core::services::asset_management::drawing::DRAWING_TYPE,
        uri: "gui-icon:///mark.ippd".into(),
        variant: 0,
    };
    let mut parts = Rows::new();
    parts
        .push(GuiPaintPart {
            asset: Some(asset.clone()),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Icon)).unwrap()
        })
        .unwrap();
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts: parts.clone(),
            ..Default::default()
        })],
        None,
    );
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let mut requests = host.take_resource_requests();
    for _ in 0..8 {
        if !requests.is_empty() {
            break;
        }
        frame(&mut host);
        requests.extend(host.take_resource_requests());
    }
    assert_eq!(requests.len(), 1);
    host.complete_resource(requests[0].id, Ok(drawing_bytes()))
        .unwrap();
    for _ in 0..8 {
        frame(&mut host);
    }
    let (unchecked, _) = output(&host, root);
    assert!(
        control_part(&unchecked, entity, CanvasPart::Icon).is_none(),
        "ready base asset still denotes checked only"
    );
    parts
        .push(GuiPaintPart {
            asset: Some(asset),
            ..GuiPaintPart::keyed(GuiPartId::variant(
                GuiPrimitivePart::Icon,
                GuiSkinState::Idle,
                GuiPartVariant::Unchecked,
            ))
            .unwrap()
        })
        .unwrap();
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(theme),
            ComponentValue::GuiTheme(GuiTheme {
                parts,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (explicit, _) = output(&host, root);
    let CanvasPrimitive::Drawing {
        drawing,
        ..
    } = control_part(&explicit, entity, CanvasPart::Icon)
        .expect("explicit unchecked source must paint")
    else {
        unreachable!()
    };
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), *drawing)
            .is_some()
    );
}

#[test]
fn checkbox_indicator_centers_wide_square_and_tall_controls_before_visual_mapping() {
    let cases = [
        ([40.0, 10.0], -1.0, [1.0, 1.0], [2.5, 2.5]),
        ([40.0, 10.0], 0.0, [1.0, 1.0], [17.5, 2.5]),
        ([40.0, 10.0], 1.0, [1.0, 1.0], [32.5, 2.5]),
        ([40.0, 10.0], 1.0, [2.0, 0.5], [30.0, 3.75]),
        ([10.0, 10.0], -1.0, [1.0, 1.0], [2.5, 2.5]),
        ([10.0, 10.0], 1.0, [2.0, 0.5], [0.0, 3.75]),
        ([10.0, 40.0], 0.0, [1.0, 1.0], [2.5, 17.5]),
        ([10.0, 40.0], -1.0, [1.0, 1.0], [2.5, 17.5]),
        ([10.0, 40.0], 1.0, [2.0, 0.5], [0.0, 18.75]),
    ];
    for (extent, align_x, scale, expected_position) in cases {
        let (mut host, world, root) = fixture();
        let entity = control(
            &mut host,
            world,
            root,
            ComponentValue::GuiCheckbox(GuiCheckbox {
                checked: true,
                ..Default::default()
            }),
        );
        let mut parts = Rows::new();
        parts
            .push(GuiPaintPart {
                align_x: Some(align_x),
                scale: Some(scale),
                ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Icon)).unwrap()
            })
            .unwrap();
        apply(
            &mut host,
            world,
            vec![
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::GuiLayout(GuiLayout {
                        width: extent[0],
                        height: extent[1],
                        ..Default::default()
                    }),
                ),
                Command::insert_value(
                    EntityRef::Handle(entity),
                    ComponentValue::GuiSkin(GuiSkin {
                        parts,
                        ..Default::default()
                    }),
                ),
            ],
        )
        .result
        .unwrap();
        frame(&mut host);
        let (canvas, _) = output(&host, root);
        let CanvasPrimitive::Box {
            style,
            size,
            ..
        } = control_part(&canvas, entity, CanvasPart::Icon).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(*size, [5.0, 5.0]);
        assert_eq!(
            style.position, expected_position,
            "extent={extent:?}, align={align_x}, scale={scale:?}"
        );
        assert_eq!(style.scale, scale);
        assert_eq!(canvas.hits[0].bounds, [0.0, 0.0, extent[0], extent[1]]);
    }
}

#[test]
fn disabled_and_hidden_controls_remain_observable_without_eligible_hits() {
    let (mut host, world, root) = fixture();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    frame(&mut host);
    let (_, before) = output(&host, root);
    let top = support::top_level_root(&mut host, world);
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(top),
            ComponentValue::GuiBehavior(GuiBehavior {
                enabled: false,
                visible: false,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (canvas, semantics) = output(&host, root);
    let observed = &semantics.views[&root].controls[0];
    assert!(!observed.hit.eligible && !observed.record.enabled && !observed.record.visible);
    assert!(canvas.entries.is_empty());
    assert_eq!(
        observed.record.target,
        before.views[&root].controls[0].record.target
    );
    assert_eq!(observed.record.target.entity, entity);
    let read = read_control(&mut host, world, entity).unwrap();
    assert!(!read.enabled && !read.visible);
    assert_eq!(read.value, ControlValue::Bool(false));
}

#[test]
fn slider_semantic_rail_matches_painted_thumb_under_signed_visual_mapping() {
    let (mut host, world, root) = fixture();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.75,
            ..Default::default()
        }),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 120.0,
                scale_x: -2.0,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (canvas, semantics) = output(&host, root);
    let observed = &semantics.views[&root].controls[0];
    let slider = observed.slider.unwrap();
    assert_eq!(slider.thumb_centers, [7.5, 42.5]);
    assert_eq!(slider.thumb_rect, [26.25, 2.5, 15.0, 15.0]);
    assert_eq!(observed.hit.bounds, [20.0, 0.0, 120.0, 20.0]);
    let CanvasPrimitive::Box {
        style,
        size,
        ..
    } = canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .find(|primitive| primitive.style().identity.part == CanvasPart::Icon)
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(*size, [15.0, 15.0]);
    assert_eq!(style.position, [67.5, 2.5]);
    let center_in_canvas = style.position[0] + size[0] * style.scale[0] * 0.5;
    let center_local = (center_in_canvas - observed.hit.position[0]) / observed.hit.scale[0];
    assert_eq!(
        (center_local - slider.thumb_centers[0])
            / (slider.thumb_centers[1] - slider.thumb_centers[0]),
        0.75
    );
    assert_eq!(style.clip, observed.hit.clip);
}

#[test]
fn slider_range_that_excludes_its_value_presents_a_clamped_thumb_and_keeps_the_value() {
    let (mut host, world, root) = fixture();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.75,
            ..Default::default()
        }),
    );
    frame(&mut host);

    // The 50 x 20 rail carries a 15 x 15 thumb whose centre travels from 7.5
    // to 42.5. A range above or below the stored value presents the thumb at
    // the nearer end, in paint and in the published hit geometry alike.
    for (min, max, end) in [(0.0_f32, 0.5_f32, 1usize), (1.0, 2.0, 0)] {
        let bound = |offset: usize, value: f32| Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_SLIDER,
            field: FieldWrite {
                offset: offset as u32,
                value: FieldValue::F32(value),
            },
        };
        apply(
            &mut host,
            world,
            vec![
                // Each write keeps min <= max: the upper bound moves first.
                bound(std::mem::offset_of!(GuiSlider, max), max),
                bound(std::mem::offset_of!(GuiSlider, min), min),
            ],
        )
        .result
        .unwrap();
        frame(&mut host);
        assert_eq!(
            read_control(&mut host, world, entity).unwrap().value,
            ControlValue::Scalar(0.75),
            "a range edit keeps the stored value"
        );

        let (canvas, semantics) = output(&host, root);
        let observed = &semantics.views[&root].controls[0];
        let slider = observed.slider.unwrap();
        assert_eq!([slider.min, slider.max], [min, max]);
        let centre = slider.thumb_centers[end];
        assert_eq!(slider.thumb_rect, [centre - 7.5, 2.5, 15.0, 15.0]);

        let painted = |part| {
            canvas
                .entries
                .iter()
                .map(|entry| primitive(entry))
                .find_map(|primitive| match primitive {
                    CanvasPrimitive::Box {
                        style,
                        size,
                        ..
                    } if style.identity.part == part => Some((style.position, *size)),
                    _ => None,
                })
                .unwrap()
        };
        let (thumb, size) = painted(CanvasPart::Icon);
        let thumb_centre =
            (thumb[0] + size[0] * 0.5 - observed.hit.position[0]) / observed.hit.scale[0];
        assert_eq!(thumb_centre, centre, "range [{min}, {max}]");
        let (fill, size) = painted(CanvasPart::Fill);
        assert_eq!(
            fill[0] + size[0] - observed.hit.position[0],
            centre,
            "the fill ends under the clamped thumb"
        );
    }
}

/// Control-local `[x, y, width, height]` of the boxes painted for `part` of
/// `entity`'s control, in paint order.
fn painted_parts(
    canvas: &CanvasPublication,
    semantics: &GuiCanvasPublication,
    root: OutputRef,
    entity: EntityId,
    part: CanvasPart,
) -> Vec<[f32; 4]> {
    let observed = semantics.views[&root]
        .controls
        .iter()
        .find(|control| control.record.target.entity == entity)
        .unwrap();
    canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .filter_map(|primitive| match primitive {
            CanvasPrimitive::Box {
                style,
                size,
                ..
            } if style.identity.part == part && style.identity.target.entity == entity => Some([
                style.position[0] - observed.hit.position[0],
                style.position[1] - observed.hit.position[1],
                size[0],
                size[1],
            ]),
            _ => None,
        })
        .collect()
}

/// A slider in a `20 x 80` box under the row root.
fn upright_slider(
    host: &mut HostRuntime,
    world: WorldId,
    root: OutputRef,
    slider: GuiSlider,
) -> EntityId {
    let parent = support::top_level_root(host, root.world().id());
    create(
        host,
        world,
        vec![
            ComponentValue::GuiSlider(slider),
            ComponentValue::GuiLayout(GuiLayout {
                width: 20.0,
                height: 80.0,
                ..Default::default()
            }),
        ],
        Some(parent),
    )
}

#[test]
fn a_vertical_slider_paints_and_publishes_its_rail_with_the_minimum_at_the_bottom() {
    let (mut host, world, root) = fixture();
    let entity = upright_slider(
        &mut host,
        world,
        root,
        GuiSlider {
            value: 0.25,
            axis: 1,
            ..Default::default()
        },
    );
    frame(&mut host);
    let (canvas, semantics) = output(&host, root);
    let observed = &semantics.views[&root].controls[0];

    // A 15-unit thumb travels from 72.5 at the bottom to 7.5 at the top; the
    // hit stays the whole control, however thin the painted rail.
    let slider = observed.slider.unwrap();
    assert_eq!(slider.axis, 1);
    assert_eq!(slider.thumb_centers, [72.5, 7.5]);
    assert_eq!(slider.thumb_rect, [2.5, 48.75, 15.0, 15.0]);
    let bounds = observed.hit.bounds;
    assert_eq!([bounds[2] - bounds[0], bounds[3] - bounds[1]], [20.0, 80.0]);

    // The rail is one bar (half the 14-unit default font) thick along the
    // whole control; the fill rises from its bottom to the thumb centre.
    let parts = |part| painted_parts(&canvas, &semantics, root, entity, part);
    assert_eq!(parts(CanvasPart::Background), [[6.5, 0.0, 7.0, 80.0]]);
    assert_eq!(parts(CanvasPart::Fill), [[6.5, 56.25, 7.0, 23.75]]);
    assert_eq!(parts(CanvasPart::Icon), [[2.5, 48.75, 15.0, 15.0]]);
}

#[test]
fn a_fill_origin_inside_the_range_fills_only_between_it_and_the_thumb() {
    let (mut host, world, root) = fixture();
    let bipolar = GuiSlider {
        min: -100.0,
        max: 100.0,
        origin: 0.0,
        axis: 1,
        ..Default::default()
    };
    let entity = upright_slider(&mut host, world, root, bipolar);

    // From the origin at the middle (40) down to -50 or up to +50, and
    // nothing while the value is at the origin; an origin outside the range
    // clamps to its bound.
    for (origin, value, fill) in [
        (0.0, -50.0, vec![[6.5, 40.0, 7.0, 16.25]]),
        (0.0, 50.0, vec![[6.5, 23.75, 7.0, 16.25]]),
        (0.0, 0.0, Vec::new()),
        (500.0, 50.0, vec![[6.5, 0.0, 7.0, 23.75]]),
        (f32::MIN, -100.0, vec![[6.5, 72.5, 7.0, 7.5]]),
    ] {
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiSlider(GuiSlider {
                    origin,
                    value,
                    ..bipolar
                }),
            )],
        )
        .result
        .unwrap();
        frame(&mut host);
        let (canvas, semantics) = output(&host, root);
        assert_eq!(
            painted_parts(&canvas, &semantics, root, entity, CanvasPart::Fill),
            fill,
            "origin {origin}, value {value}"
        );
    }
}

#[test]
fn an_unsized_slider_measures_along_its_axis_and_remeasures_when_it_turns() {
    let (mut host, world, root) = fixture();
    let parent = support::top_level_root(&mut host, world);
    let entity = create(
        &mut host,
        world,
        vec![
            ComponentValue::GuiSlider(GuiSlider::default()),
            ComponentValue::GuiFont(GuiFont {
                font_size: 9.0,
                ..Default::default()
            }),
        ],
        Some(parent),
    );
    frame(&mut host);
    let size = |host: &HostRuntime| {
        let (_, semantics) = output(host, root);
        let bounds = semantics.views[&root].controls[0].hit.bounds;
        [bounds[2] - bounds[0], bounds[3] - bounds[1]]
    };

    // Eight ems along the rail and four thirds of an em across it, so the
    // thumb, three quarters of that depth, is an em square.
    let (length, depth) = (8.0 * 9.0, 12.0);
    assert_eq!(size(&host), [length, depth]);
    apply(
        &mut host,
        world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_SLIDER,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSlider, axis) as u32,
                value: FieldValue::U32(1),
            },
        }],
    )
    .result
    .unwrap();
    frame(&mut host);
    assert_eq!(size(&host), [depth, length]);
}

#[test]
fn refused_theme_write_leaves_presentation_and_control_unchanged() {
    let (mut host, world, root) = fixture();
    let mut parts = Rows::new();
    let row = parts
        .push(part(
            GuiPartId::base(GuiPrimitivePart::Background),
            [1.0; 4],
        ))
        .unwrap();
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })],
        None,
    );
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (painted, before) = output(&host, root);
    let stored_theme = |host: &mut ControlHost| {
        host.world_mut(world)
            .unwrap()
            .inspect(theme)
            .unwrap()
            .components
    };
    let theme_before = stored_theme(&mut host);
    let write_opacity = |value| Command::SetField {
        entity: EntityRef::Handle(theme),
        component: ComponentValue::GUI_THEME,
        field: FieldWrite {
            offset: Rows::<GuiPaintPart>::offset(0, row, GuiPartProperty::Opacity.index()).unwrap(),
            value: FieldValue::Dynamic(DynamicValue::F32(value)),
        },
    };
    assert_eq!(
        apply(&mut host, world, vec![write_opacity(-1.0)])
            .result
            .unwrap_err()
            .reason,
        ErrorReason::InvalidValue
    );
    assert_eq!(stored_theme(&mut host), theme_before);
    frame(&mut host);
    let (unchanged, semantics) = output(&host, root);
    assert_eq!(unchanged.entries, painted.entries);
    assert!(semantics.views[&root].controls[0].available);
    assert!(unchanged.hits[0].eligible);
    let read = read_control(&mut host, world, entity).unwrap();
    assert!(read.available);
    assert_eq!(read.value, ControlValue::Bool(false));
    assert_eq!(read.target, before.views[&root].controls[0].record.target);
    apply(&mut host, world, vec![write_opacity(0.5)])
        .result
        .unwrap();
    frame(&mut host);
    let (restyled, semantics) = output(&host, root);
    assert!(restyled.hits[0].eligible);
    assert_ne!(restyled.entries, painted.entries);
    assert_eq!(
        semantics.views[&root].controls[0].record.target,
        read.target
    );
}

fn drawing_bytes() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    for value in [10.0_f32, 20.0, 42.0, 44.0, 10.0, 20.0, 42.0, 44.0, 0.05] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0_u32.to_le_bytes());
    bytes
}

fn asset_skin(source: &str) -> ComponentValue {
    let mut parts = Rows::new();
    parts
        .push(GuiPaintPart {
            asset: Some(ipp_core::services::asset_management::AssetSource {
                kind: ipp_core::services::asset_management::drawing::DRAWING_TYPE,
                uri: source.into(),
                variant: 0,
            }),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap()
        })
        .unwrap();
    ComponentValue::GuiSkin(GuiSkin {
        parts,
        ..Default::default()
    })
}

#[test]
fn ordinary_skin_demand_retains_ready_resource_with_new_geometry_only_in_same_control_lifetime() {
    let (mut host, world, root) = fixture();
    host.register_stream_resource_provider("gui-drawing")
        .unwrap();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            asset_skin("gui-drawing:///first.ippd"),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let mut requests = host.take_resource_requests();
    for _ in 0..8 {
        if !requests.is_empty() {
            break;
        }
        frame(&mut host);
        requests.extend(host.take_resource_requests());
    }
    assert_eq!(requests.len(), 1);
    assert!(output(&host, root).0.entries.is_empty());
    host.complete_resource(requests[0].id, Ok(drawing_bytes()))
        .unwrap();
    for _ in 0..8 {
        frame(&mut host);
        if !output(&host, root).0.entries.is_empty() {
            break;
        }
    }
    let (ready, _) = output(&host, root);
    let CanvasPrimitive::Drawing {
        style,
        drawing,
    } = primitive(&ready.entries[0])
    else {
        panic!("ready drawing part")
    };
    let key = *drawing;
    assert_eq!(style.scale, [50.0 / 32.0, 20.0 / 24.0]);
    assert_eq!(
        style.position,
        [-10.0 * style.scale[0], -20.0 * style.scale[1]]
    );
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), key)
            .is_some()
    );
    apply(
        &mut host,
        world,
        vec![
            Command::insert_value(
                EntityRef::Handle(entity),
                asset_skin("gui-drawing:///replacement.ippd"),
            ),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 100.0,
                    height: 48.0,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::CanvasStyle(CanvasStyle {
                    x: 9.0,
                    ..Default::default()
                }),
            ),
        ],
    )
    .result
    .unwrap();
    frame(&mut host);
    let mut requests = host.take_resource_requests();
    for _ in 0..8 {
        if !requests.is_empty() {
            break;
        }
        frame(&mut host);
        requests.extend(host.take_resource_requests());
    }
    assert_eq!(requests.len(), 1);
    let (pending, _) = output(&host, root);
    let CanvasPrimitive::Drawing {
        style,
        drawing,
    } = primitive(&pending.entries[0])
    else {
        panic!("retained drawing part")
    };
    assert_eq!(*drawing, key);
    assert_eq!(style.scale, [100.0 / 32.0, 2.0]);
    assert_eq!(style.position, [9.0 - 10.0 * style.scale[0], -40.0]);
    assert_eq!(pending.hits[0].bounds, [9.0, 0.0, 109.0, 48.0]);
    assert!(
        host.publication_resource(host.latest_publication(world).unwrap(), key)
            .is_some()
    );
    let target = pending.hits[0].target;
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_BUTTON,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiButton(GuiButton::default()),
            ),
        ],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (new_lifetime, _) = output(&host, root);
    assert!(new_lifetime.entries.is_empty());
    assert_ne!(new_lifetime.hits[0].target, target);
    host.complete_resource(requests[0].id, Ok(drawing_bytes()))
        .unwrap();
    for _ in 0..8 {
        frame(&mut host);
        if !output(&host, root).0.entries.is_empty() {
            break;
        }
    }
    let (replacement, _) = output(&host, root);
    let CanvasPrimitive::Drawing {
        drawing,
        ..
    } = primitive(&replacement.entries[0])
    else {
        unreachable!()
    };
    assert_ne!(*drawing, key);
}

fn scroll_parts(canvas: &CanvasPublication, entity: EntityId) -> Vec<(CanvasPart, [f32; 4])> {
    canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .filter_map(|primitive| match primitive {
            CanvasPrimitive::Box {
                style,
                fill: CanvasShapeFill::Solid(color),
                ..
            } if style.identity.target.entity == entity
                && matches!(
                    style.identity.part,
                    CanvasPart::ScrollTrackX
                        | CanvasPart::ScrollThumbX
                        | CanvasPart::ScrollTrackY
                        | CanvasPart::ScrollThumbY
                ) =>
            {
                Some((style.identity.part, *color))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_scrolling_axis_keeps_its_bar_over_fitting_content_without_input() {
    let (mut host, world, root) = fixture();
    let view = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiScrollView(Default::default()),
    );
    frame(&mut host);
    // The vertical view keeps its default track while its content fits; the
    // track's disabled state hides the thumb (opacity 0) and the other axis
    // shows nothing.
    let (canvas, _) = output(&host, root);
    let parts = scroll_parts(&canvas, view);
    assert_eq!(
        parts.iter().map(|(part, _)| *part).collect::<Vec<_>>(),
        [CanvasPart::ScrollTrackY, CanvasPart::ScrollThumbY]
    );
    let thumb = canvas
        .entries
        .iter()
        .map(|entry| primitive(entry))
        .find(|primitive| {
            primitive.style().identity.target.entity == view
                && primitive.style().identity.part == CanvasPart::ScrollThumbY
        })
        .unwrap();
    assert_eq!(thumb.style().opacity, 0.0);

    // A theme restyles the disabled track; a horizontal track style still
    // shows no bar on an axis the view neither scrolls nor overflows.
    let track = [0.3, 0.3, 0.3, 1.0];
    let mut parts = Rows::new();
    for row in [
        part(
            GuiPartId::base(GuiPrimitivePart::ScrollTrackX),
            [1.0, 0.0, 0.0, 1.0],
        ),
        part(
            GuiPartId::state(GuiPrimitivePart::ScrollTrackY, GuiSkinState::Disabled),
            track,
        ),
    ] {
        parts.push(row).unwrap();
    }
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
            ..Default::default()
        })],
        None,
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(view),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    let (canvas, semantics) = output(&host, root);
    assert_eq!(
        scroll_parts(&canvas, view)[0],
        (CanvasPart::ScrollTrackY, track)
    );
    assert!(
        scroll_parts(&canvas, view)
            .iter()
            .all(|(part, _)| !matches!(part, CanvasPart::ScrollTrackX | CanvasPart::ScrollThumbX))
    );
    // The bar that cannot scroll takes no input.
    assert!(canvas.hits.iter().all(|hit| !matches!(
        hit.kind,
        CanvasHitKind::ScrollTrack { .. } | CanvasHitKind::ScrollThumb { .. }
    )));
    let control = semantics.views[&root]
        .controls
        .iter()
        .find(|control| control.record.target.entity == view)
        .unwrap();
    assert_eq!(control.record.kind, GuiControlKind::ScrollView);
    assert_eq!(control.record.scroll().unwrap().1, [0.0, 0.0]);
}
