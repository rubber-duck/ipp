//! Shared fixtures for GUI input system tests.

use super::super::super::test_support::{font_fixture_bytes, font_source, test_font};
use super::*;
use crate::services::asset_management::font::FontAsset;
use crate::services::asset_management::{AssetKey, AssetSource};
use crate::systems::surface::Surface;
use crate::systems::surface::SurfaceRenderResource;
use crate::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef, HostRuntime, WorldId,
    WorldLimits,
};
use crate::{
    GuiCommand, GuiContainerKind, GuiEvaluatedContent, GuiFontResolution, GuiNodeContent,
    GuiNodeId, GuiNodeStyle, GuiResourceResolver,
};
use std::collections::BTreeMap;

pub(super) const SESSION: u64 = 7;

pub(super) struct Fixture {
    pub(super) host: HostRuntime,
    pub(super) world: WorldId,
    pub(super) panel: EntityId,
}

pub(super) fn setup() -> Fixture {
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
                            surface.width = 10.0;
                            surface.height = 10.0;
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
    Fixture {
        host,
        world,
        panel,
    }
}

pub(super) fn world<'a>(fixture: &'a mut Fixture) -> crate::WorldContext<'a> {
    fixture.host.world_mut(fixture.world).unwrap()
}

pub(super) fn incarnation(fixture: &mut Fixture) -> u64 {
    let panel = fixture.panel;
    world(fixture)
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation
}

pub(super) fn insert_nodes(fixture: &mut Fixture, with_slider: bool, with_second: bool) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let mut commands = vec![GuiCommand::InsertNode {
        entity: panel,
        root_incarnation,
        id: GuiNodeId(1),
        parent: None,
        index: 0,
        content: GuiNodeContent::Container(GuiContainerKind::Column),
        style: GuiNodeStyle {
            width: Some(10.0),
            height: Some(10.0),
            ..Default::default()
        },
    }];
    commands.push(GuiCommand::InsertNode {
        entity: panel,
        root_incarnation,
        id: GuiNodeId(2),
        parent: Some(GuiNodeId(1)),
        index: 0,
        content: GuiNodeContent::Checkbox {
            checked: false,
        },
        style: GuiNodeStyle::default(),
    });
    if with_slider {
        commands.push(GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(3),
            parent: Some(GuiNodeId(1)),
            index: 1,
            content: GuiNodeContent::Slider {
                value: 0.5,
                min: 0.0,
                max: 1.0,
                step: 0.0,
            },
            style: GuiNodeStyle::default(),
        });
    }
    if with_second {
        commands.push(GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(if with_slider {
                4
            } else {
                3
            }),
            parent: Some(GuiNodeId(1)),
            index: 2,
            content: GuiNodeContent::Checkbox {
                checked: false,
            },
            style: GuiNodeStyle::default(),
        });
    }
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

/// Centre of one evaluated node in logical units.
pub(super) fn node_centre(fixture: &mut Fixture, node: GuiNodeId) -> [f32; 2] {
    let rect = node_rect(fixture, fixture.panel, node);
    assert!(rect[2] > 0.0 && rect[3] > 0.0, "node has no hit area");
    [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0]
}

/// Retained rectangle of one evaluated node in logical units.
pub(super) fn node_rect(fixture: &mut Fixture, panel: EntityId, node: GuiNodeId) -> [f32; 4] {
    world(fixture)
        .gui_layout_view(panel)
        .unwrap()
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == node)
        .map(|evaluated| evaluated.rect)
        .unwrap()
}

/// Retained evaluated content of one node from the current layout view.
pub(super) fn evaluated_content(fixture: &mut Fixture, node: GuiNodeId) -> GuiEvaluatedContent {
    let panel = fixture.panel;
    world(fixture)
        .gui_layout_view(panel)
        .unwrap()
        .nodes
        .iter()
        .find(|evaluated| evaluated.node == node)
        .map(|evaluated| evaluated.content.clone())
        .unwrap()
}

pub(super) fn down_up(pointer: u32, position: [f32; 2]) -> [GuiInputCommand; 2] {
    [
        GuiInputCommand::PointerDown {
            pointer,
            panel: None,
            position,
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
        GuiInputCommand::PointerUp {
            pointer,
            panel: None,
            position,
            button: GuiPointerButton::Primary,
            blockers: Vec::new(),
            panel_distance: None,
        },
    ]
}

pub(super) fn committed_bool(fixture: &mut Fixture, node: GuiNodeId) -> (GuiControlValue, u32) {
    committed_bool_on(fixture, fixture.panel, node)
}

pub(super) fn committed_bool_on(
    fixture: &mut Fixture,
    panel: EntityId,
    node: GuiNodeId,
) -> (GuiControlValue, u32) {
    let inspected = world(fixture).inspect_gui(panel, Some(node), 1, 4).unwrap();
    let node = inspected
        .nodes
        .iter()
        .find(|inspected| inspected.id == node)
        .unwrap();
    (node.control_value.clone(), node.control_revision)
}

/// Resolver serving the shared fixture font with a stable fake asset key. The
/// world path measures through the Host asset service; `register_font` pins
/// parity between the two below.
pub(super) struct TestResolver<'a> {
    pub(super) font: Option<(&'a FontAsset, AssetKey)>,
    pub(super) pending_font: bool,
    pub(super) resources: BTreeMap<String, SurfaceRenderResource>,
}

impl<'a> TestResolver<'a> {
    fn with_font(font: &'a FontAsset) -> Self {
        Self {
            font: Some((
                font,
                AssetKey {
                    slot: 3,
                    generation: 1,
                },
            )),
            pending_font: false,
            resources: BTreeMap::new(),
        }
    }
}

impl GuiResourceResolver for TestResolver<'_> {
    fn text_font(&self, source: &AssetSource) -> GuiFontResolution<'_> {
        if source.uri != "test-font" {
            return GuiFontResolution::Missing;
        }

        match self.font {
            Some((font, key)) => GuiFontResolution::Ready {
                key,
                font,
            },
            None if self.pending_font => GuiFontResolution::Pending {
                key: AssetKey {
                    slot: 3,
                    generation: 1,
                },
            },
            None => GuiFontResolution::Missing,
        }
    }

    fn surface_resource(&self, source: &AssetSource) -> Option<SurfaceRenderResource> {
        self.resources.get(&source.uri).cloned()
    }
}

/// Register the fixture font for one world and pump shared loading until
/// the decoded payload is observable. World-level tests cannot use a local
/// resolver: the retained views measure through `SystemResolver`, so the
/// font must be a live Host asset before any font-backed node is inserted.
pub(super) fn register_font(fixture: &mut Fixture) {
    // Fixture parity with layout_tests: the unit resolver must decode the
    // same metrics the world path measures against.
    let font = test_font();
    let resolver = TestResolver::with_font(&font);
    let GuiFontResolution::Ready {
        key,
        ..
    } = resolver.text_font(&font_source())
    else {
        panic!("font fixture must resolve to a ready font");
    };
    assert_eq!(
        key,
        AssetKey {
            slot: 3,
            generation: 1,
        }
    );

    let world = fixture.world;
    fixture
        .host
        .asset_resources_mut()
        .register_client_source(world, font_source(), font_fixture_bytes())
        .unwrap();
    // The memory source is synchronous; pump shared loading until the
    // decoded payload is observable, bounding the drain.
    for _ in 0..8 {
        fixture.host.progress_assets();
        if font_decoded(fixture) {
            break;
        }
    }
    assert!(
        font_decoded(fixture),
        "font fixture must decode through shared loading"
    );
}

pub(super) fn font_decoded(fixture: &Fixture) -> bool {
    fixture
        .host
        .asset_resources()
        .find(&font_source())
        .and_then(|key| fixture.host.asset_resources().get(key))
        .is_some_and(|provider| provider.decoded_available())
}

pub(super) fn font_style() -> GuiNodeStyle {
    GuiNodeStyle {
        asset: Some(font_source()),
        ..Default::default()
    }
}

/// Insert a column panel holding one font-backed control node.
pub(super) fn insert_font_control(fixture: &mut Fixture, content: GuiNodeContent) {
    let root_incarnation = incarnation(fixture);
    let panel = fixture.panel;
    let commands = vec![
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(1),
            parent: None,
            index: 0,
            content: GuiNodeContent::Container(GuiContainerKind::Column),
            style: GuiNodeStyle {
                width: Some(10.0),
                height: Some(10.0),
                ..Default::default()
            },
        },
        GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(2),
            parent: Some(GuiNodeId(1)),
            index: 0,
            content,
            style: font_style(),
        },
    ];
    let mut context = world(fixture);
    for command in commands {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}
