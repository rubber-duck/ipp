//! Focused tests for the per-root units-per-metre client contract.
//!
//! The scheduled pass must evaluate each root with its client-set density,
//! defaulting to [`DEFAULT_UNITS_PER_METRE`] when unset and rejecting
//! non-finite or non-positive values without touching retained output.

use super::super::super::test_support::{font_fixture_bytes, font_source};
use super::*;
use crate::services::asset_management::AssetSource;
use crate::systems::gui::{
    GuiCommand, GuiContainerKind, GuiEvaluatedContent, GuiLayoutDiagnostic, GuiNodeContent,
    GuiNodeId, GuiNodeStyle, GuiRoot,
};
use crate::systems::surface::Surface;
use crate::{Batch, Command, ComponentValue, EntityMetadata, EntityRef, HostRuntime, WorldLimits};

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
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::Surface({
                            let mut surface = Surface::default();
                            surface.width = 4.0;
                            surface.height = 3.0;
                            surface
                        }),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::GuiRoot(GuiRoot::default()),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    (host, world, panel)
}

#[test]
fn unset_roots_keep_default_density() {
    let (mut host, world, panel) = setup();
    let view = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    assert_eq!(view.units_per_metre, DEFAULT_UNITS_PER_METRE);
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0, 3.0]);
}

#[test]
fn client_density_rescales_root_extent() {
    let (mut host, world, panel) = setup();
    host.world_mut(world)
        .unwrap()
        .set_gui_units_per_metre(panel, 2.0)
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let context = host.world_mut(world).unwrap();
    assert_eq!(context.gui_units_per_metre(panel), 2.0);
    let view = context.gui_layout_view(panel).unwrap();
    assert_eq!(view.units_per_metre, 2.0);
    assert_eq!(view.root_bounds, [0.0, 0.0, 8.0, 6.0]);
}

#[test]
fn invalid_density_rejects_without_effect() {
    let (mut host, world, panel) = setup();
    for units in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            host.world_mut(world)
                .unwrap()
                .set_gui_units_per_metre(panel, units)
                .is_err()
        );
    }
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let context = host.world_mut(world).unwrap();
    assert_eq!(context.gui_units_per_metre(panel), DEFAULT_UNITS_PER_METRE);
    let view = context.gui_layout_view(panel).unwrap();
    assert_eq!(view.units_per_metre, DEFAULT_UNITS_PER_METRE);
    assert_eq!(view.root_bounds, [0.0, 0.0, 4.0, 3.0]);
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
                content: GuiNodeContent::Container(GuiContainerKind::Column),
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
                content: GuiNodeContent::Text("A".into()),
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
                content: GuiNodeContent::Container(GuiContainerKind::Column),
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
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
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
        let request = GuiLayoutRequest {
            root: &root,
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
            Command::InsertComponentValue {
                entity: EntityRef::Handle(second),
                value: panel_surface(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Handle(second),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
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
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(first),
            value: ComponentValue::GuiRoot(GuiRoot::default()),
        }],
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
            name: "node_1_opacity".into(),
            value: crate::DynamicValue::F32(0.5),
        }],
    );
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 1);

    apply(
        &mut host,
        world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(panel),
            value: panel_surface(),
        }],
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

    host.world_mut(world)
        .unwrap()
        .set_gui_units_per_metre(panel, 2.0)
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 1);
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(super::super::evaluation::take_fingerprint_passes(), 0);
}
