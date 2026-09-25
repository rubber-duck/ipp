//! Focused tests for the scheduled layout pass: the per-root
//! `GuiRoot::units_per_metre` density read from the effective component
//! (ordinary writes, insertion before or after the Surface, animation and
//! StateOverlays all reflow the root; invalid values are rejected at the
//! command boundary), resource readiness, retained-output reuse and root
//! membership.

use super::super::super::test_support::{font_fixture_bytes, font_source};
use super::super::evaluation::DEFAULT_UNITS_PER_METRE;
use super::*;
use crate::GuiNodeDataRow;
use crate::services::asset_management::AssetSource;
use crate::systems::gui::{
    GuiCommand, GuiContainerKind, GuiEvaluatedContent, GuiLayoutDiagnostic, GuiNodeData, GuiNodeId,
    GuiNodeStyle, GuiRoot,
};
use crate::systems::surface::Surface;
use crate::{
    Batch, BatchOutcome, Command, ComponentValue, EntityMetadata, EntityRef, HostRuntime,
    WorldLimits,
};

fn setup() -> (HostRuntime, WorldId, EntityId) {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    let panel = {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 1,
                operations: vec![
                    Command::Create {
                        alias: 1,
                        metadata: EntityMetadata::default(),
                    },
                    Command::insert_value(
                        EntityRef::Alias(1),
                        ComponentValue::Surface({
                            let mut surface = Surface::default();
                            surface.width = 4.0;
                            surface.height = 3.0;
                            surface
                        }),
                    ),
                    Command::insert_value(
                        EntityRef::Alias(1),
                        ComponentValue::GuiRoot(GuiRoot::default()),
                    ),
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    (host, world, panel)
}

fn root_bounds(host: &mut HostRuntime, world: WorldId, panel: EntityId) -> [f32; 4] {
    host.world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap()
        .root_bounds
}

fn density_write(panel: EntityId, units: f32) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(panel),
        component: ComponentValue::GUI_ROOT,
        field: crate::FieldWrite {
            offset: GuiRoot::units_per_metre_field(),
            value: crate::FieldValue::F32(units),
        },
    }
}

fn root_with_density(units: f32) -> GuiRoot {
    let mut root = GuiRoot::default();
    root.units_per_metre = units;
    root
}

fn submit(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    let mut context = host.world_mut(world).unwrap();
    let id = context.tick() + 1;
    context
        .enqueue(Batch {
            id,
            operations,
        })
        .unwrap();
    context.step(0.0).unwrap().outcomes.remove(0)
}

/// A 1x1 logical checkbox (node 2) at the root origin under a Column, so hit
/// regions follow the density: it covers Surface content `[0, 1/U]` on both
/// axes.
fn insert_box(host: &mut HostRuntime, world: WorldId, panel: EntityId) {
    let mut context = host.world_mut(world).unwrap();
    let root_incarnation = context
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    for command in [
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            data: GuiNodeData::Container(GuiContainerKind::Column),
            values: GuiNodeDataRow::default(),
            style: GuiNodeStyle::default(),
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(2),
            parent: Some(GuiNodeId(1)),
            index: 0,
            data: GuiNodeData::Checkbox,
            values: GuiNodeDataRow::checkbox(false),
            style: GuiNodeStyle {
                width: Some(1.0),
                height: Some(1.0),
                background_color: Some([0.2, 0.3, 0.4, 1.0]),
                ..Default::default()
            },
        },
    ] {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

/// Retained paint in Surface content metres.
fn box_paint(
    host: &mut HostRuntime,
    world: WorldId,
    panel: EntityId,
) -> Vec<crate::systems::surface::SurfaceRenderPrimitive> {
    host.world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap()
        .surface_primitives()
}

/// Density-1 box paint mapped to a density `units`: positions and every box
/// length divide by the density once, because authored lengths are logical
/// units. The root clip is the Surface rectangle at every density.
fn paint_at_density(
    paint: &[crate::systems::surface::SurfaceRenderPrimitive],
    units: f32,
) -> Vec<crate::systems::surface::SurfaceRenderPrimitive> {
    use crate::systems::surface::SurfaceRenderPrimitive;

    paint
        .iter()
        .map(|primitive| {
            let SurfaceRenderPrimitive::Box {
                style,
                size,
                corner_radius,
                border_width,
                border_color,
                fill,
                glow,
            } = primitive
            else {
                panic!("box panel paints boxes only: {primitive:?}");
            };
            let mut style = *style;
            style.position = style.position.map(|lane| lane / units);
            SurfaceRenderPrimitive::Box {
                style,
                size: size.map(|lane| lane / units),
                corner_radius: corner_radius.map(|lane| lane / units),
                border_width: border_width / units,
                border_color: *border_color,
                fill: *fill,
                glow: *glow,
            }
        })
        .collect()
}

/// Authored lanes are logical units, so density rescales the root extent
/// while the 1x1 box keeps its logical rectangle: paint and hit regions
/// shrink together on the Surface, covering content `[0, 1/U]`, and paint
/// equals the density-1 `paint` divided by `U`.
fn assert_box_at_density(
    host: &mut HostRuntime,
    world: WorldId,
    panel: EntityId,
    units: f32,
    paint: &[crate::systems::surface::SurfaceRenderPrimitive],
) {
    let view = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert_eq!(view.units_per_metre, units);
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0 * units, 3.0 * units]);
    let node = view
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(2))
        .unwrap();
    assert_eq!(node.rect, [0.0, 0.0, 1.0, 1.0]);

    let hit = view.hit_test_content([0.75 / units, 0.75 / units]).unwrap();
    assert_eq!(hit.node, GuiNodeId(2));
    assert_eq!(hit.position, [0.75, 0.75]);
    assert_ne!(
        view.hit_test_content([1.25 / units, 0.25 / units])
            .map(|hit| hit.node),
        Some(GuiNodeId(2))
    );
    assert_eq!(view.surface_primitives(), paint_at_density(paint, units));
}

#[test]
fn unset_roots_keep_default_density() {
    let (mut host, world, panel) = setup();
    assert_eq!(GuiRoot::default().units_per_metre, DEFAULT_UNITS_PER_METRE);
    let view = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert_eq!(view.units_per_metre, DEFAULT_UNITS_PER_METRE);
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0, 3.0]);
}

#[test]
fn component_density_write_rescales_root_extent() {
    let (mut host, world, panel) = setup();
    let outcome = submit(&mut host, world, vec![density_write(panel, 2.0)]);
    assert!(outcome.result.is_ok(), "{outcome:?}");

    let context = host.world_mut(world).unwrap();
    assert_eq!(context.gui_root(panel).unwrap().units_per_metre, 2.0);
    let view = context.gui_layout_view(panel).unwrap();
    assert_eq!(view.units_per_metre, 2.0);
    assert_eq!(view.root_bounds, [0.0, 0.0, 8.0, 6.0]);
}

#[test]
fn invalid_density_rejects_without_effect() {
    let (mut host, world, panel) = setup();
    for units in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let outcome = submit(&mut host, world, vec![density_write(panel, units)]);
        assert!(outcome.result.is_err(), "{units} accepted");

        let root = root_with_density(units);
        let outcome = submit(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(panel),
                ComponentValue::GuiRoot(root),
            )],
        );
        assert!(outcome.result.is_err(), "{units} inserted");
    }

    let context = host.world_mut(world).unwrap();
    assert_eq!(
        context.gui_root(panel).unwrap().units_per_metre,
        DEFAULT_UNITS_PER_METRE
    );
    let view = context.gui_layout_view(panel).unwrap();
    assert_eq!(view.units_per_metre, DEFAULT_UNITS_PER_METRE);
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0, 3.0]);
}

/// Formerly the density lived in a layout side map that dropped values set
/// before the root was first evaluated. It now travels with the root, so a
/// root attached to an already evaluated Surface uses its own density.
#[test]
fn density_set_before_attach_applies_on_first_evaluation() {
    let (mut host, world, _) = setup();
    let panel = apply(
        &mut host,
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: EntityMetadata::default(),
            },
            Command::insert_value(EntityRef::Alias(1), panel_surface()),
        ],
    )[0];
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(!evaluated(&mut host, world).contains(&panel));

    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(panel),
            ComponentValue::GuiRoot(root_with_density(2.0)),
        )],
    );
    assert_eq!(root_bounds(&mut host, world, panel), [0.0, 0.0, 4.0, 2.0]);

    // Removing and reattaching the root starts from the attached value.
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
            },
            Command::insert_value(
                EntityRef::Handle(panel),
                ComponentValue::GuiRoot(GuiRoot::default()),
            ),
            density_write(panel, 3.0),
        ],
    );
    assert_eq!(root_bounds(&mut host, world, panel), [0.0, 0.0, 6.0, 3.0]);
}

fn density_clip(start: f32, end: f32) -> crate::systems::animation::AnimationClip {
    use crate::systems::animation::*;

    let key = |time, value, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(crate::components::schema::FieldValue::F32(value)),
        interpolation,
    };
    AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: density_target(),
            keys: vec![
                key(0.0, start, AnimationInterpolation::Linear),
                key(1.0, end, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap()
}

fn density_target() -> crate::systems::animation::AnimationTrackTarget {
    crate::systems::animation::AnimationTrackTarget::AnimationProperty(
        crate::systems::animation::AnimationProperty {
            component: ComponentValue::GUI_ROOT,
            offsets: vec![GuiRoot::units_per_metre_field()],
        },
    )
}

fn frame(host: &mut HostRuntime, world: WorldId, dt: f64) {
    let mut context = host.world_mut(world).unwrap();
    context.prepare_update(dt).unwrap();
    context.poll_assets();
    context.step(dt).unwrap();
}

#[test]
fn animated_density_reflows_root_with_paint_and_hit_regions_together() {
    use crate::services::asset_management::{AssetUpload, AssetUploadIdentity};
    use crate::systems::animation::*;

    let (mut host, world, panel) = setup();
    insert_box(&mut host, world, panel);
    let paint = box_paint(&mut host, world, panel);
    assert!(!paint.is_empty());
    assert_box_at_density(&mut host, world, panel, 1.0, &paint);

    host.world_mut(world)
        .unwrap()
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: density_clip(1.0, 3.0).encode(),
        })
        .unwrap();
    for _ in 0..512 {
        let mut context = host.world_mut(world).unwrap();
        context.prepare_update(0.0).unwrap();
        context.poll_assets();
        if !context.step(0.0).unwrap().assets.is_empty() {
            break;
        }
    }

    let controller = {
        let mut context = host.world_mut(world).unwrap();
        let controller = context
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: "asset://10/1".into(),
                    variant: 0,
                    track: 0,
                    target: panel,
                    property: density_target(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                speed: 1.0,
                ..Default::default()
            })
            .unwrap();
        context
            .control_animation_controller(controller, AnimationPlaybackControl::Play)
            .unwrap();
        controller
    };
    frame(&mut host, world, 0.0);
    frame(&mut host, world, 0.5);

    // Animation precedes layout: the sampled density reflows this frame.
    assert_box_at_density(&mut host, world, panel, 2.0, &paint);

    // Removing the controller restores the authored density and reflows.
    host.world_mut(world)
        .unwrap()
        .remove_animation_controller(controller)
        .unwrap();
    frame(&mut host, world, 0.0);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_root(panel)
            .unwrap()
            .units_per_metre,
        DEFAULT_UNITS_PER_METRE
    );
    assert_box_at_density(&mut host, world, panel, 1.0, &paint);
}

#[test]
fn overlaid_density_reflows_root_and_release_restores_authored_value() {
    use crate::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let (mut host, world, panel) = setup();
    insert_box(&mut host, world, panel);
    let paint = box_paint(&mut host, world, panel);
    submit(
        &mut host,
        world,
        vec![
            Command::SetMetadata {
                entity: EntityRef::Handle(panel),
                metadata: EntityMetadata {
                    symbolic_id: Some("density-panel".into()),
                    classes: vec![],
                },
            },
            density_write(panel, 0.5),
        ],
    );
    assert_box_at_density(&mut host, world, panel, 0.5, &paint);

    let outcome = submit(
        &mut host,
        world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "density-panel".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::GUI_ROOT,
                mode: ComponentOverlayMode::Bound,
                fields: vec![crate::FieldWrite {
                    offset: GuiRoot::units_per_metre_field(),
                    value: crate::FieldValue::F32(2.0),
                }],
            },
        ],
    );
    assert!(outcome.result.is_ok(), "{outcome:?}");
    assert_box_at_density(&mut host, world, panel, 2.0, &paint);

    // Inspection reports the authored producer and the overlaid effective
    // density separately.
    let snapshot = host.world_mut(world).unwrap().inspect(panel).unwrap();
    let density = |values: &[ComponentValue]| {
        values.iter().find_map(|value| match value {
            ComponentValue::GuiRoot(root) => Some(root.units_per_metre),
            _ => None,
        })
    };
    assert_eq!(density(&snapshot.base), Some(0.5));
    assert_eq!(density(&snapshot.effective), Some(2.0));

    // An invalid overlay value is rejected at the command boundary.
    let rejected = submit(
        &mut host,
        world,
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "density-panel".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component: ComponentValue::GUI_ROOT,
                mode: ComponentOverlayMode::Bound,
                fields: vec![crate::FieldWrite {
                    offset: GuiRoot::units_per_metre_field(),
                    value: crate::FieldValue::F32(-1.0),
                }],
            },
        ],
    );
    assert!(rejected.result.is_err(), "{rejected:?}");

    let owner = outcome
        .state_overlays
        .iter()
        .find(|alias| alias.alias == 1)
        .unwrap()
        .id;
    let released = submit(
        &mut host,
        world,
        vec![Command::ReleaseStateOverlayOwner {
            owner: StateOverlayRef::Handle(owner),
        }],
    );
    assert!(released.result.is_ok(), "{released:?}");
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_root(panel)
            .unwrap()
            .units_per_metre,
        0.5
    );
    assert_box_at_density(&mut host, world, panel, 0.5, &paint);
}

// The production resolver reports slot generations, so readiness flips the
// retained fingerprint without authored edits.

const SESSION: u64 = 7;

fn font_decoded(host: &HostRuntime) -> bool {
    host.asset_resources()
        .find(&font_source())
        .and_then(|key| host.asset_resources().get(key))
        .is_some_and(|provider| provider.decoded_available())
}

/// Register the fixture font and pump shared loading until decoding is
/// observable. World-level tests measure through `SystemResolver`, never a
/// local resolver.
fn register_font(host: &mut HostRuntime, world: WorldId) {
    host.asset_resources_mut()
        .register_client_source(world, font_source(), font_fixture_bytes())
        .unwrap();

    for _ in 0..8 {
        host.progress_assets();

        if font_decoded(host) {
            break;
        }
    }

    assert!(
        font_decoded(host),
        "font fixture must decode through shared loading"
    );
}

#[test]
fn pending_font_recovers_without_authored_edits() {
    let (mut host, world, panel) = setup();
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;

    // Insert a font-backed text leaf before the font is registered.
    {
        let mut context = host.world_mut(world).unwrap();

        for command in [
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(1),
                parent: None,
                index: 0,
                data: GuiNodeData::Container(GuiContainerKind::Column),
                values: crate::GuiNodeDataRow::default(),
                style: GuiNodeStyle {
                    width: Some(4.0),
                    height: Some(3.0),
                    ..Default::default()
                },
            },
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(2),
                parent: Some(GuiNodeId(1)),
                index: 0,
                data: GuiNodeData::Text("A".into()),
                values: crate::GuiNodeDataRow::default(),
                style: GuiNodeStyle {
                    asset: Some(font_source()),
                    ..Default::default()
                },
            },
        ] {
            context.enqueue_gui_command(SESSION, command).unwrap();
        }

        context.step(0.0).unwrap();
    }

    // Pending on first evaluation: unavailable with a pending diagnostic
    // and no measurement work.
    let pending = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    let record = pending
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(2))
        .unwrap();
    assert!(!record.available);
    assert!(
        pending.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            GuiLayoutDiagnostic::PendingText {
                node,
            } if *node == GuiNodeId(2)
        )),
        "diagnostics: {:?}",
        pending.diagnostics
    );
    assert_eq!(pending.remeasure_count, 0);

    // The same reference readies with no GUI edit: retained output rebuilds
    // and only the affected branch remeasures.
    register_font(&mut host, world);
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let ready = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    let record = ready
        .nodes
        .iter()
        .find(|node| node.node == GuiNodeId(2))
        .unwrap();
    assert!(record.available);
    assert!(matches!(record.content, GuiEvaluatedContent::Text { .. }));
    assert!(ready.remeasure_count > pending.remeasure_count);
    assert!(
        ready
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic, GuiLayoutDiagnostic::PendingText { .. }))
    );

    // A stable generation with no edits does no further work.
    let settled_layout = ready.layout_revision;
    let settled_measure = ready.remeasure_count;
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let settled = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert_eq!(settled.layout_revision, settled_layout);
    assert_eq!(settled.remeasure_count, settled_measure);
    assert!(
        settled
            .nodes
            .iter()
            .find(|node| node.node == GuiNodeId(2))
            .unwrap()
            .available
    );
}

/// Diagnostics counters of the scheduled pass: an unchanged frame does no
/// layout work, a paint-only edit neither reflows nor measures text, and a
/// text edit reflows once and measures only its own leaf.
#[test]
fn layout_statistics_count_reflows_and_text_measurements() {
    use crate::systems::gui::{GuiNodeHandle, GuiNodePatch};

    let (mut host, world, panel) = setup();
    register_font(&mut host, world);
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let text = |id: u32, label: &str| GuiCommand::InsertNode {
        entity: panel,
        root_incarnation: incarnation,
        id: GuiNodeId(id),
        parent: Some(GuiNodeId(1)),
        index: id - 2,
        data: GuiNodeData::Text(label.into()),
        values: crate::GuiNodeDataRow::default(),
        style: GuiNodeStyle {
            asset: Some(font_source()),
            font_size: 0.1,
            ..Default::default()
        },
    };
    let statistics = |host: &mut HostRuntime| {
        host.world_mut(world)
            .unwrap()
            .system::<GuiLayoutSystem>(GuiLayoutSystem::ID)
            .unwrap()
            .statistics()
    };
    let step = |host: &mut HostRuntime, commands: Vec<GuiCommand>| {
        let mut context = host.world_mut(world).unwrap();
        for command in commands {
            context.enqueue_gui_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    };
    step(
        &mut host,
        vec![
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(1),
                parent: None,
                index: 0,
                data: GuiNodeData::Container(GuiContainerKind::Column),
                values: crate::GuiNodeDataRow::default(),
                style: GuiNodeStyle::default(),
            },
            text(2, "A"),
            text(3, "V"),
        ],
    );
    let mounted = statistics(&mut host);
    assert!(mounted.total.reflows >= 1);
    assert_eq!(mounted.total.text_measurements, 2);

    step(&mut host, Vec::new());
    let unchanged = statistics(&mut host);
    assert_eq!(unchanged.latest, Default::default());
    assert_eq!(unchanged.total, mounted.total);

    let update = |id: u32, patch: GuiNodePatch| GuiCommand::UpdateNode {
        handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(id)),
        patch,
    };
    step(
        &mut host,
        vec![update(
            2,
            GuiNodePatch {
                color: Some([1.0, 0.0, 0.0, 1.0]),
                ..Default::default()
            },
        )],
    );
    let painted = statistics(&mut host);
    assert_eq!(painted.latest, Default::default());
    assert_eq!(painted.total, mounted.total);

    step(
        &mut host,
        vec![update(
            3,
            GuiNodePatch {
                data: Some(GuiNodeData::Text("AV".into())),
                ..Default::default()
            },
        )],
    );
    let edited = statistics(&mut host);
    assert_eq!(edited.latest.reflows, 1);
    assert_eq!(edited.latest.text_measurements, 1);
    assert_eq!(edited.total.reflows, mounted.total.reflows + 1);
    assert_eq!(edited.total.text_measurements, 3);
}

struct EmptyResolver;

impl GuiResourceResolver for EmptyResolver {
    fn text_font(&self, _source: &AssetSource) -> GuiFontResolution<'_> {
        GuiFontResolution::Missing
    }

    fn surface_resource(
        &self,
        _source: &AssetSource,
    ) -> Option<crate::systems::surface::SurfaceRenderResource> {
        None
    }
}

#[test]
fn scheduled_pass_recovers_identical_valid_input_after_invalid_cache_entry() {
    let (mut host, world, panel) = setup();
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    {
        let mut context = host.world_mut(world).unwrap();
        for command in [
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(1),
                parent: None,
                index: 0,
                data: GuiNodeData::Container(GuiContainerKind::Column),
                values: crate::GuiNodeDataRow::default(),
                style: GuiNodeStyle {
                    width: Some(4.0),
                    height: Some(3.0),
                    ..Default::default()
                },
            },
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(2),
                parent: Some(GuiNodeId(1)),
                index: 0,
                data: GuiNodeData::Checkbox,
                values: GuiNodeDataRow::checkbox(false),
                style: GuiNodeStyle {
                    width: Some(1.0),
                    height: Some(1.0),
                    background_color: Some([0.2, 0.3, 0.4, 1.0]),
                    ..Default::default()
                },
            },
        ] {
            context.enqueue_gui_command(SESSION, command).unwrap();
        }
        context.step(0.0).unwrap();
    }
    let first = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert!(first.available);
    assert!(first.hit_test([0.5, 0.5]).is_some());
    assert!(!first.surface_primitives().is_empty());

    let unavailable = {
        let mut context = host.world_mut(world).unwrap();
        let root = context.gui_root(panel).unwrap().clone();
        let tree = crate::systems::gui::GuiTreeIndex::new(&root, incarnation);
        let request = GuiLayoutRequest {
            root: &root,
            tree: &tree,
            root_incarnation: incarnation,
            surface_size: [0.0, 3.0],
            units_per_metre: DEFAULT_UNITS_PER_METRE,
            evaluation_tick: context.tick(),
        };
        context
            .with_system::<GuiLayoutSystem, _>(GuiLayoutSystem::ID, |system, _| {
                system
                    .evaluate_for_test(panel, &request, &EmptyResolver)
                    .clone()
            })
            .unwrap()
    };
    assert!(!unavailable.available);
    assert!(unavailable.hit_test([0.5, 0.5]).is_none());
    assert!(unavailable.surface_primitives().is_empty());

    host.world_mut(world).unwrap().step(0.0).unwrap();
    let recovered = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert!(recovered.available);
    assert!(recovered.hit_test([0.5, 0.5]).is_some());
    assert!(!recovered.surface_primitives().is_empty());
    assert!(recovered.reflow_count > first.reflow_count);
}

fn panel_surface() -> ComponentValue {
    let mut surface = Surface::default();
    surface.width = 2.0;
    surface.height = 1.0;
    ComponentValue::Surface(surface)
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> Vec<EntityId> {
    let mut context = host.world_mut(world).unwrap();
    let id = context.tick() + 1;
    context
        .enqueue(Batch {
            id,
            operations,
        })
        .unwrap();
    let report = context.step(0.0).unwrap();
    report.outcomes[0]
        .result
        .as_ref()
        .unwrap()
        .iter()
        .map(|(_, entity)| *entity)
        .collect()
}

fn evaluated(host: &mut HostRuntime, world: WorldId) -> Vec<EntityId> {
    host.world_mut(world)
        .unwrap()
        .system::<GuiLayoutSystem>(GuiLayoutSystem::ID)
        .unwrap()
        .evaluated_entities()
}

#[test]
fn root_membership_follows_component_lifecycle_among_plain_entities() {
    let (mut host, world, first) = setup();
    let plain = apply(
        &mut host,
        world,
        (0..2_000)
            .map(|alias| Command::Create {
                alias,
                metadata: EntityMetadata::default(),
            })
            .collect(),
    );
    assert_eq!(evaluated(&mut host, world), vec![first]);

    // A root added to an existing plain entity joins the next evaluation.
    let second = plain[1_000];
    apply(
        &mut host,
        world,
        vec![
            Command::insert_value(EntityRef::Handle(second), panel_surface()),
            Command::insert_value(
                EntityRef::Handle(second),
                ComponentValue::GuiRoot(GuiRoot::default()),
            ),
        ],
    );
    assert_eq!(evaluated(&mut host, world), vec![first, second]);

    // Removing the component or deleting the entity drops retained output.
    apply(
        &mut host,
        world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(first),
            component: ComponentValue::GUI_ROOT,
        }],
    );
    assert_eq!(evaluated(&mut host, world), vec![second]);

    apply(
        &mut host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(second),
        }],
    );
    assert!(evaluated(&mut host, world).is_empty());

    // A reinserted root evaluates again under its new incarnation.
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(first),
            ComponentValue::GuiRoot(GuiRoot::default()),
        )],
    );
    assert_eq!(evaluated(&mut host, world), vec![first]);
}

#[test]
fn unchanged_roots_keep_output_without_fingerprinting() {
    let (mut host, world, panel) = setup();
    apply(&mut host, world, Vec::new());
    super::super::evaluation::take_fingerprint_passes();

    for _ in 0..5 {
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 0);
    let tick = host.world_mut(world).unwrap().tick();
    let view = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert_eq!(
        view.evaluation_tick,
        tick - 1,
        "no-op frames still advance the tick"
    );

    // A commit to the root, its Surface or its density re-evaluates it once.
    apply(
        &mut host,
        world,
        vec![Command::SetDynamicProperty {
            entity: EntityRef::Handle(panel),
            component: ComponentValue::GUI_ROOT,
            name: "node_1_part_background_opacity".into(),
            value: crate::DynamicValue::F32(0.5),
        }],
    );
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 1);

    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(panel),
            panel_surface(),
        )],
    );
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 1);
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .gui_layout_view(panel)
            .unwrap()
            .root_bounds,
        [0.0, 0.0, 2.0, 1.0]
    );

    apply(&mut host, world, vec![density_write(panel, 2.0)]);
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 1);
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 0);
}
