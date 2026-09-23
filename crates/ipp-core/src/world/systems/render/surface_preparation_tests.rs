use super::*;
use crate::services::asset_management::drawing::DRAWING_TYPE;
use crate::services::asset_management::{AssetSource, AssetUpload, AssetUploadIdentity};
use crate::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationInterpolation, AnimationKeyframe, AnimationTrack,
    AnimationTrackTarget, AnimationTransitionEasing, AnimationValue,
};
use crate::systems::gui::{
    GuiCommand, GuiContainerKind, GuiControlValue, GuiInputCancelReason, GuiInputCommand,
    GuiInputEffectKind, GuiNodeContent, GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodeStyle,
    GuiPointerButton, GuiRoot, GuiUnhandledReason,
};
use crate::systems::surface::{GuiPrimitivePart, GuiShapeFill, SurfacePrimitiveIdentity};
use crate::{
    Batch, Command, ComponentValue, DynamicValue, EntityMetadata, EntityRef, FieldValue,
    FieldWrite, HostRuntime, Surface, SurfaceCommand, SurfaceGlyph, SurfaceItemContent,
    SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle, SurfaceRenderPrimitive, WorldContext,
    WorldId,
    components::{MeshInstance, Scalar, Transform},
};

fn font_bytes() -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1000_u32.to_le_bytes());
    for value in [800.0_f32, -200.0, 100.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [1_u32, 1, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [500.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&65_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

fn update(world: &mut WorldContext<'_>) -> crate::WorldUpdateReport {
    world.prepare_update(0.0).unwrap();
    world.poll_assets();
    world.step(0.0).unwrap()
}

fn observation(world: &mut WorldContext<'_>) -> (u64, u64, *const SurfaceGlyph, usize) {
    world
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, _| {
            let SurfaceRenderPrimitive::Glyphs {
                glyphs,
                ..
            } = &system.state.surface_items[0].primitives[0]
            else {
                unreachable!()
            };
            (
                system.state.surface_layout_cache.model_preparations,
                system.state.surface_layout_cache.primitive_preparations,
                glyphs.as_ptr(),
                glyphs.capacity(),
            )
        })
        .unwrap()
}

fn setup() -> (
    crate::HostRuntime,
    crate::WorldId,
    crate::EntityId,
    crate::EntityId,
) {
    let mut host = crate::HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    world
        .enqueue_asset(AssetUpload {
            id: 1,
            key: AssetUploadIdentity {
                kind: crate::services::asset_management::font::FONT_TYPE,
                asset: 1,
                variant: 0,
            },
            bytes: font_bytes(),
        })
        .unwrap();
    for _ in 0..32 {
        if !update(&mut world).assets.is_empty() {
            break;
        }
    }
    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::Label("A".repeat(128)),
            SurfaceItemStyle {
                asset: Some(AssetSource {
                    kind: crate::services::asset_management::font::FONT_TYPE,
                    uri: format!(
                        "asset://{}/1",
                        crate::services::asset_management::font::FONT_TYPE.0
                    ),
                    variant: 0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Surface(surface),
                },
                Command::Create {
                    alias: 2,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(2),
                    value: ComponentValue::Scalar(Scalar {
                        value: 1.0,
                    }),
                },
            ],
        })
        .unwrap();
    let report = update(&mut world);
    let surface = report.outcomes[0].result.as_ref().unwrap()[0].1;
    let scalar = report.outcomes[0].result.as_ref().unwrap()[1].1;
    drop(world);
    (host, id, surface, scalar)
}

#[test]
fn retained_surface_preparation_skips_unrelated_work_and_reuses_glyph_storage() {
    let (mut host, id, surface, scalar) = setup();
    let mut world = host.world_mut(id).unwrap();
    update(&mut world);
    update(&mut world);
    let initial = observation(&mut world);

    world
        .enqueue(Batch {
            id: 2,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(scalar),
                component: ComponentValue::SCALAR,
                field: FieldWrite {
                    offset: std::mem::offset_of!(Scalar, value) as u32,
                    value: FieldValue::F32(2.0),
                },
            }],
        })
        .unwrap();
    update(&mut world);
    assert_eq!(observation(&mut world), initial);

    world
        .enqueue(Batch {
            id: 3,
            operations: vec![Command::SetField {
                entity: EntityRef::Handle(surface),
                component: ComponentValue::TRANSFORM,
                field: FieldWrite {
                    offset: std::mem::offset_of!(Transform, x) as u32,
                    value: FieldValue::F32(2.0),
                },
            }],
        })
        .unwrap();
    update(&mut world);
    let model_only = observation(&mut world);
    assert_eq!((model_only.0, model_only.1), (initial.0 + 1, initial.1));
    assert_eq!((model_only.2, model_only.3), (initial.2, initial.3));

    for (opacity, content) in [(Some(0.5), None), (None, Some("A".repeat(64)))] {
        world
            .enqueue_surface_command(
                1,
                SurfaceCommand::Update {
                    entity: surface,
                    id: SurfaceItemId(1),
                    patch: SurfaceItemPatch {
                        opacity,
                        content: content.map(SurfaceItemContent::Label),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        update(&mut world);
    }
    let edited = observation(&mut world);
    assert_eq!(edited.1, initial.1 + 2);
    assert_eq!((edited.2, edited.3), (initial.2, initial.3));

    world
        .enqueue(Batch {
            id: 4,
            operations: vec![
                Command::Create {
                    alias: 3,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(3),
                    value: ComponentValue::MeshInstance(MeshInstance {
                        source: "asset://1/999".into(),
                        variant: 0,
                    }),
                },
            ],
        })
        .unwrap();
    update(&mut world);
    let pending = observation(&mut world);
    update(&mut world);
    update(&mut world);
    assert_eq!(observation(&mut world), pending);
}

#[test]
fn model_recovery_rebuilds_primitives_for_the_reappearing_surface() {
    let (mut host, id, _, _) = setup();
    let mut world = host.world_mut(id).unwrap();
    world
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, access| {
            // An invalid evaluated model removes this record in the preceding
            // preparation. Exercise the following model-only recovery directly.
            system.state.surface_items.clear();
            let before = system.state.surface_layout_cache.primitive_preparations;
            crate::systems::surface::rendering::prepare_surface_render_items(
                access.world,
                access.asset_acquisition,
                &mut system.state.surface_layout_cache,
                &mut system.state.surface_items,
                false,
            );
            assert_eq!(system.state.surface_items.len(), 1);
            assert_eq!(
                system.state.surface_layout_cache.primitive_preparations,
                before + 1
            );
            assert_eq!(system.state.surface_items[0].primitives.len(), 1);
        })
        .unwrap();
}

// ---------------------------------------------------------------------------
// P07b skin paint: interaction resolves into prepared Surface primitives,
// with scroll translation wrapping the skinned output.
// ---------------------------------------------------------------------------

const SESSION: u64 = 7;

fn part_color(root: &mut GuiRoot, node: u32, part: &str, value: [f32; 4]) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "color").unwrap();
    root.properties
        .set(&name, DynamicValue::Vec4(value))
        .unwrap();
}

fn part_asset(root: &mut GuiRoot, node: u32, part: &str, source: AssetSource) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "asset").unwrap();
    root.properties
        .set(&name, DynamicValue::Asset(source))
        .unwrap();
}

fn part_f32(root: &mut GuiRoot, node: u32, part: &str, lane: &str, value: f32) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, lane).unwrap();
    root.properties
        .set(&name, DynamicValue::F32(value))
        .unwrap();
}

fn part_vec2(root: &mut GuiRoot, node: u32, part: &str, lane: &str, value: [f32; 2]) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, lane).unwrap();
    root.properties
        .set(&name, DynamicValue::Vec2(value))
        .unwrap();
}

fn part_motion(root: &mut GuiRoot, node: u32, part: &str, source: AssetSource) {
    let name = GuiRoot::part_property_name(GuiNodeId(node), part, "motion").unwrap();
    root.properties
        .set(&name, DynamicValue::Asset(source))
        .unwrap();
}

/// GuiRoot with checkbox skin parts preset for node 2: idle, hovered,
/// pressed and disabled colors under the production paint part.
fn skinned_root() -> GuiRoot {
    let mut root = GuiRoot::default();
    part_color(&mut root, 2, "background", [0.1, 0.1, 0.1, 1.0]);
    part_color(
        &mut root,
        2,
        "background_idle_unchecked",
        [0.25, 0.25, 0.25, 1.0],
    );
    part_color(&mut root, 2, "background_hovered", [0.0, 1.0, 0.0, 1.0]);
    part_color(&mut root, 2, "background_pressed", [1.0, 0.0, 0.0, 1.0]);
    part_color(&mut root, 2, "background_disabled", [0.5, 0.5, 0.5, 1.0]);
    root
}

fn skin_motion_source(name: &str) -> AssetSource {
    AssetSource {
        kind: ANIMATION_TYPE,
        uri: format!("fixture:///{name}.ippa"),
        variant: 0,
    }
}

fn skin_motion_clip(color: [f32; 4]) -> AnimationClip {
    AnimationClip::new(1.0, skin_motion_tracks(color)).unwrap()
}

fn skin_motion_tracks(color: [f32; 4]) -> Vec<AnimationTrack<AnimationValue>> {
    let dynamic =
        |value| AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(value));
    let track = |lane: &str, value: DynamicValue| AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::GUI_ROOT,
            name: GuiRoot::part_property_name(GuiNodeId(2), "background", lane).unwrap(),
        },
        keys: vec![AnimationKeyframe {
            time: 1.0,
            value: dynamic(value),
            interpolation: AnimationInterpolation::Step,
        }],
    };
    vec![
        track("color", DynamicValue::Vec4(color)),
        track("opacity", DynamicValue::F32(1.0)),
        track("scale", DynamicValue::Vec2([1.0, 1.0])),
    ]
}

fn animated_skinned_root(sources: &[AssetSource; 3]) -> GuiRoot {
    let mut root = skinned_root();
    part_color(&mut root, 2, "background", [0.25, 0.25, 0.25, 1.0]);
    part_f32(&mut root, 2, "background", "opacity", 1.0);
    part_vec2(&mut root, 2, "background", "scale", [1.0, 1.0]);
    for (part, source) in [
        ("background", sources[0].clone()),
        ("background_hovered", sources[1].clone()),
        ("background_pressed", sources[2].clone()),
    ] {
        part_motion(&mut root, 2, part, source);
        part_f32(&mut root, 2, part, "duration", 1.0);
        part_f32(&mut root, 2, part, "easing", 0.0);
        part_f32(&mut root, 2, part, "track", 0.0);
        part_f32(&mut root, 2, part, "time", 1.0);
    }
    root
}

fn register_skin_motion_assets(host: &mut HostRuntime, world: WorldId) -> [AssetSource; 3] {
    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover"),
        skin_motion_source("skin-pressed"),
    ];
    for (source, color) in sources.iter().zip([
        [0.25, 0.25, 0.25, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
    ]) {
        host.asset_resources_mut()
            .register_client_source(world, source.clone(), skin_motion_clip(color).encode())
            .unwrap();
    }
    sources
}

fn skin_panel() -> (HostRuntime, WorldId, crate::EntityId) {
    skin_panel_with_root(skinned_root())
}

fn skin_panel_with_root(root: GuiRoot) -> (HostRuntime, WorldId, crate::EntityId) {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default()).unwrap();
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
                        value: ComponentValue::GuiRoot(root),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };

    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;

        for command in [
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
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
                root_incarnation: incarnation,
                id: GuiNodeId(2),
                parent: Some(GuiNodeId(1)),
                index: 0,
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
                style: GuiNodeStyle {
                    width: Some(2.0),
                    height: Some(1.0),
                    background_color: Some([0.2, 0.2, 0.2, 1.0]),
                    ..Default::default()
                },
            },
        ] {
            context.enqueue_gui_command(SESSION, command).unwrap();
        }

        context.step(0.0).unwrap();
    }

    (host, world, panel)
}

/// Retained rectangle centre of one evaluated node in logical units.
fn node_centre(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    node: GuiNodeId,
) -> [f32; 2] {
    let view = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .unwrap();
    let rect = view
        .nodes
        .iter()
        .find(|record| record.node == node)
        .map(|record| record.rect)
        .unwrap();
    assert!(rect[2] > 0.0 && rect[3] > 0.0, "node has no hit area");
    [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0]
}

/// Painted background fill of node 2 in the prepared Surface primitives.
///
/// The renderer paints a box from its `fill`; the style colour lane only
/// joins its revision. Both must carry the same (possibly sampled) colour.
fn panel_box_color(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) -> [f32; 4] {
    host.world_mut(world)
        .unwrap()
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, _| {
            system
                .state
                .surface_items
                .iter()
                .find(|item| item.entity == panel)
                .unwrap()
                .primitives
                .iter()
                .find_map(|primitive| match primitive {
                    SurfaceRenderPrimitive::Box {
                        style,
                        fill,
                        ..
                    } if matches!(
                        style.identity,
                        SurfacePrimitiveIdentity::Gui(id)
                            if id.node == GuiNodeId(2)
                                && id.lifetime == 1
                                && id.part == GuiPrimitivePart::Background
                    ) =>
                    {
                        let GuiShapeFill::Solid(painted) = fill else {
                            panic!("background box must paint a solid fill, got {fill:?}");
                        };

                        assert_eq!(
                            style.color, *painted,
                            "painted fill diverged from the style colour lane"
                        );
                        Some(*painted)
                    }
                    _ => None,
                })
                .unwrap()
        })
        .unwrap()
}

fn pointer_down(pointer: u32, position: [f32; 2]) -> GuiInputCommand {
    GuiInputCommand::PointerDown {
        pointer,
        panel: None,
        position,
        button: GuiPointerButton::Primary,
        blockers: Vec::new(),
        panel_distance: None,
    }
}

fn pointer_up(pointer: u32, position: [f32; 2]) -> GuiInputCommand {
    GuiInputCommand::PointerUp {
        pointer,
        panel: None,
        position,
        button: GuiPointerButton::Primary,
        blockers: Vec::new(),
        panel_distance: None,
    }
}

fn pointer_move(pointer: u32, position: [f32; 2]) -> GuiInputCommand {
    GuiInputCommand::PointerMove {
        pointer,
        panel: None,
        position,
        blockers: Vec::new(),
        panel_distance: None,
    }
}

fn committed_bool(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
) -> (GuiControlValue, u32) {
    let inspected = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, Some(GuiNodeId(2)), 1, 4)
        .unwrap();
    let node = inspected
        .nodes
        .iter()
        .find(|inspected| inspected.id == GuiNodeId(2))
        .unwrap();
    (node.control_value.clone(), node.control_revision)
}

#[test]
fn skinned_paint_resolves_hover_and_pressed_in_prepared_primitives() {
    let (mut host, world, panel) = skin_panel();

    // Idle paints the unchecked variant part.
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0]
    );

    // Hover paints the hovered part without committing anything.
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));

    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_move(1, at))
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(false), 1)
    );

    // Press keeps hover but paints pressed: pressed wins over hovered.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_down(1, at))
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(false), 1)
    );

    // An in-bounds release commits exactly once and returns to hover paint.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_up(1, at))
            .unwrap();
        context.step(0.0).unwrap();
    }

    let report = host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(
        report
            .gui_input_effects
            .iter()
            .any(|effect| matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
    );
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(true), 2)
    );
}

fn assert_color_near(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 0.02,
            "color {actual:?} did not approach {expected:?}"
        );
    }
}

fn background_skin_owner(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
) -> crate::systems::animation::GuiSkinAnimationOwner {
    let root_incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    crate::systems::animation::GuiSkinAnimationOwner {
        entity: panel,
        primitive: crate::systems::surface::GuiPrimitiveId {
            root_incarnation,
            node: GuiNodeId(2),
            lifetime: 1,
            part: GuiPrimitivePart::Background,
        },
    }
}

fn skin_controller_snapshot(
    host: &mut HostRuntime,
    world: WorldId,
    owner: crate::systems::animation::GuiSkinAnimationOwner,
) -> Option<crate::systems::animation::AnimationControllerSnapshot> {
    host.world_mut(world)
        .unwrap()
        .with_system::<crate::systems::animation::AnimationSystem, _>(
            crate::systems::animation::AnimationSystem::ID,
            |system, _| {
                system
                    .skin_controller(owner)
                    .map(|(_, controller, _)| controller.clone())
            },
        )
        .unwrap()
}

#[test]
fn skin_controller_description_has_exactly_three_consecutive_drivers() {
    let entity = crate::EntityId::from_bits(7);
    let primitive = crate::systems::surface::GuiPrimitiveId {
        root_incarnation: 3,
        node: GuiNodeId(2),
        lifetime: 5,
        part: GuiPrimitivePart::Background,
    };
    let motion = crate::systems::gui::GuiPartMotion {
        source: skin_motion_source("track-limit"),
        duration_secs: 1.0,
        easing: AnimationTransitionEasing::Linear,
        base_track: 4_294_967_040,
        sample_time: 0.0,
    };

    let description = skin_controller_description(entity, primitive, &motion);
    assert_eq!(description.drivers.len(), 3);
    assert_eq!(
        description
            .drivers
            .iter()
            .map(|driver| driver.track)
            .collect::<Vec<_>>(),
        vec![4_294_967_040, 4_294_967_041, 4_294_967_042]
    );
    assert_eq!(
        description
            .drivers
            .iter()
            .map(|driver| match &driver.property {
                AnimationTrackTarget::DynamicProperty {
                    name,
                    ..
                } => name.as_str(),
                _ => panic!("expected GUI dynamic-property driver"),
            })
            .collect::<Vec<_>>(),
        vec![
            "node_2_part_background_color",
            "node_2_part_background_opacity",
            "node_2_part_background_scale",
        ]
    );
}

#[test]
fn automatic_skin_motion_uses_animation_samples_and_interrupts_continuously() {
    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover"),
        skin_motion_source("skin-pressed"),
    ];
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    assert_eq!(register_skin_motion_assets(&mut host, world), sources);
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
        if sources.iter().all(|source| {
            host.asset_resources()
                .find(source)
                .and_then(|key| host.asset_resources().get(key))
                .is_some_and(|resource| resource.decoded_available())
        }) {
            break;
        }
    }
    assert!(sources.iter().all(|source| {
        host.asset_resources()
            .find(source)
            .and_then(|key| host.asset_resources().get(key))
            .is_some_and(|resource| resource.decoded_available())
    }));

    let owner = background_skin_owner(&mut host, world, panel);

    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(4, at))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0],
    );

    // The private controller mutation applies at the next ordinary boundary;
    // AnimationSystem alone samples the half-time numeric result.
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.5).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.125, 0.625, 0.125, 1.0],
    );
    assert!(
        host.world_mut(world)
            .unwrap()
            .animation_controllers()
            .is_empty()
    );
    let transition = skin_controller_snapshot(&mut host, world, owner)
        .unwrap()
        .transition
        .unwrap();
    assert_eq!(transition.easing, AnimationTransitionEasing::Linear);
    assert!((transition.elapsed - 0.5).abs() <= f64::EPSILON);

    // Interrupt the active hover blend with press. The first destination
    // frame starts from AnimationSystem's frozen composite without a reset.
    let interrupted = panel_box_color(&mut host, world, panel);
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_down(4, at))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_color_near(panel_box_color(&mut host, world, panel), interrupted);
    host.world_mut(world).unwrap().step(0.5).unwrap();
    let toward_pressed = panel_box_color(&mut host, world, panel);
    assert!(toward_pressed[0] > interrupted[0]);
    assert!(toward_pressed[1] < interrupted[1]);

    // Cancellation returns to the authored idle destination. The lookup is
    // reconstructed from AnimationSystem's underlying producer, never from
    // the effective in-flight base lanes.
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(
            SESSION,
            GuiInputCommand::PointerCancel {
                pointer: 4,
            },
        )
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(1.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0],
    );
    assert!(
        skin_controller_snapshot(&mut host, world, owner)
            .unwrap()
            .transition
            .is_none()
    );

    // Root departure drops the full-incarnation presentation and deletes its
    // controller before a replacement can reuse node/lifetime values.
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 88,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
            }],
        })
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
}

#[test]
fn skin_controller_commands_fence_ordinary_access_lifecycle_and_item_failures() {
    use crate::systems::animation::{
        AnimationCommand, AnimationControllerCommand, AnimationControllerTransition,
        AnimationInternalCommand, AnimationPlaybackControl, AnimationSystem,
        AnimationTransitionStartTime, GuiSkinAnimationOwner,
    };

    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover"),
        skin_motion_source("skin-pressed"),
    ];
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    register_skin_motion_assets(&mut host, world);
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    let root_incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let primitive = crate::systems::surface::GuiPrimitiveId {
        root_incarnation,
        node: GuiNodeId(2),
        lifetime: 1,
        part: GuiPrimitivePart::Background,
    };
    let owner = GuiSkinAnimationOwner {
        entity: panel,
        primitive,
    };
    let from = crate::systems::gui::GuiPartMotion {
        source: sources[0].clone(),
        duration_secs: 1.0,
        easing: AnimationTransitionEasing::Linear,
        base_track: 0,
        sample_time: 1.0,
    };
    let to = crate::systems::gui::GuiPartMotion {
        source: sources[1].clone(),
        ..from.clone()
    };
    let ensure = |owner, request| AnimationInternalCommand::EnsureSkinTransition {
        owner,
        request,
        source: skin_controller_description(panel, primitive, &from),
        source_time: from.sample_time,
        source_sample: crate::systems::animation::GuiSkinAnimationSample {
            color: [0.25, 0.25, 0.25, 1.0],
            opacity: 1.0,
            scale: [1.0, 1.0],
        },
        transition: AnimationControllerTransition {
            description: skin_controller_description(panel, primitive, &to),
            duration: to.duration_secs,
            easing: to.easing,
            start_time: AnimationTransitionStartTime::Seek(to.sample_time),
        },
        destination_sample: crate::systems::animation::GuiSkinAnimationSample {
            color: [0.0, 1.0, 0.0, 1.0],
            opacity: 1.0,
            scale: [1.0, 1.0],
        },
    };

    let mut stale = owner;
    stale.primitive.root_incarnation += 1;
    let ordinary = host
        .world_mut(world)
        .unwrap()
        .create_animation_controller(skin_controller_description(panel, primitive, &from))
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(
            AnimationSystem::ID,
            0,
            AnimationCommand::Internal(vec![ensure(stale, 1), ensure(owner, 2)]),
        )
        .unwrap();
    let report = host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(report.playback_events.is_empty());
    let controllers = host.world_mut(world).unwrap().animation_controllers();
    assert_eq!(controllers.len(), 1);
    assert_eq!(controllers[0].id, ordinary);
    let private = skin_controller_snapshot(&mut host, world, owner).unwrap();
    let private_id = private.id;

    {
        let mut context = host.world_mut(world).unwrap();
        assert!(context.animation_controller(private_id).is_none());
        assert!(
            context
                .animation_controller_page(0, private_id.to_bits(), 1)
                .is_empty()
        );
        assert_eq!(
            context.animation_controller_page(0, 0, 8),
            vec![controllers[0].clone()]
        );

        assert_eq!(
            context.update_animation_controller(private_id, private.description.clone()),
            Err(crate::ErrorReason::InvalidValue)
        );
        assert_eq!(
            context.transition_animation_controller(
                private_id,
                AnimationControllerTransition {
                    description: skin_controller_description(panel, primitive, &to),
                    duration: to.duration_secs,
                    easing: to.easing,
                    start_time: AnimationTransitionStartTime::Seek(to.sample_time),
                },
            ),
            Err(crate::ErrorReason::InvalidValue)
        );
        assert_eq!(
            context.control_animation_controller(private_id, AnimationPlaybackControl::Pause),
            Err(crate::ErrorReason::InvalidValue)
        );
        assert_eq!(
            context.remove_animation_controller(private_id),
            Err(crate::ErrorReason::InvalidValue)
        );

        for (request_id, command) in [
            (
                101,
                AnimationControllerCommand::Update {
                    id: private_id,
                    description: private.description.clone(),
                },
            ),
            (
                102,
                AnimationControllerCommand::Transition {
                    id: private_id,
                    transition: AnimationControllerTransition {
                        description: skin_controller_description(panel, primitive, &to),
                        duration: to.duration_secs,
                        easing: to.easing,
                        start_time: AnimationTransitionStartTime::Seek(to.sample_time),
                    },
                },
            ),
            (
                103,
                AnimationControllerCommand::Delete {
                    id: private_id,
                },
            ),
            (
                104,
                AnimationControllerCommand::Control {
                    id: private_id,
                    control: AnimationPlaybackControl::Stop,
                },
            ),
        ] {
            context
                .enqueue_animation_controller(request_id, command)
                .unwrap();
        }
        context
            .enqueue_playback(private_id, AnimationPlaybackControl::Stop)
            .unwrap();
    }
    let report = host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(report.animation_controller_outcomes.len(), 4);
    assert!(
        report
            .animation_controller_outcomes
            .iter()
            .all(|outcome| { outcome.result == Err(crate::ErrorReason::InvalidValue) })
    );
    assert_eq!(
        skin_controller_snapshot(&mut host, world, owner),
        Some(private)
    );
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(ordinary),
        Some(controllers[0].clone())
    );

    let replacement = panel_root(&mut host, world, panel);
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 91,
            operations: vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(panel),
                value: ComponentValue::GuiRoot(replacement),
            }],
        })
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(
            AnimationSystem::ID,
            0,
            AnimationCommand::Internal(vec![ensure(owner, 3)]),
        )
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let controllers = host.world_mut(world).unwrap().animation_controllers();
    assert_eq!(controllers.len(), 1);
    assert_eq!(controllers[0].id, ordinary);
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
}

#[test]
fn live_restore_invalidates_skin_ownership_without_commandeering_reused_id() {
    use crate::systems::animation::{
        AnimationControllerSnapshot, AnimationPersistentState, AnimationPlaybackStatus,
    };

    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover"),
        skin_motion_source("skin-pressed"),
    ];
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    register_skin_motion_assets(&mut host, world);
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }

    let owner = background_skin_owner(&mut host, world, panel);
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(7, at))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.5).unwrap();
    let interrupted = panel_box_color(&mut host, world, panel);
    assert!(interrupted[1] > interrupted[0]);
    let private = skin_controller_snapshot(&mut host, world, owner).unwrap();

    // A refused replacement leaves the live derived controller and its owner
    // association unchanged.
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .restore_animation_controllers(AnimationPersistentState {
                next_id: 0,
                ..Default::default()
            }),
        Err(crate::ErrorReason::InvalidValue)
    );
    assert_eq!(
        skin_controller_snapshot(&mut host, world, owner),
        Some(private.clone())
    );

    // Reuse the private controller's numeric identity for an ordinary stopped
    // controller. Successful live replacement must invalidate the owner map
    // before that ID can be observed or acted on as a skin controller.
    let collision = AnimationControllerSnapshot {
        id: private.id,
        description: private.description.clone(),
        state: AnimationPlaybackStatus::Stopped,
        time: 0.0,
        transition: None,
    };
    host.world_mut(world)
        .unwrap()
        .restore_animation_controllers(AnimationPersistentState {
            next_id: private.id.to_bits() + 1,
            controllers: vec![collision.clone()],
            transitions: Vec::new(),
            directional_starts: Vec::new(),
        })
        .unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(private.id),
        Some(collision.clone())
    );
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0],
    );
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(private.id),
        Some(collision.clone())
    );
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_persistent_state()
            .controllers,
        vec![collision.clone()]
    );

    // A later state change derives a fresh private controller and completes
    // normally; loss recovery does not strand or permanently disable motion.
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(7, [9.0, 9.0]))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_some());
    host.world_mut(world).unwrap().step(1.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0],
    );
    assert_eq!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(private.id),
        Some(collision)
    );

    // The same successful replacement boundary also clears an association
    // whose transition has already completed and is holding its endpoint.
    let completed = skin_controller_snapshot(&mut host, world, owner).unwrap();
    assert!(completed.transition.is_none());
    let persistent = host.world_mut(world).unwrap().animation_persistent_state();
    host.world_mut(world)
        .unwrap()
        .restore_animation_controllers(persistent.clone())
        .unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        host.world_mut(world).unwrap().animation_persistent_state(),
        persistent
    );
}

#[test]
fn mismatched_skin_endpoint_settles_static_and_corrected_motion_recovers() {
    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover-mismatch"),
        skin_motion_source("skin-pressed"),
    ];
    let corrected = skin_motion_source("skin-hover-corrected");
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    for (source, color) in [
        (&sources[0], [0.25, 0.25, 0.25, 1.0]),
        (&sources[1], [1.0, 0.0, 0.0, 1.0]),
        (&sources[2], [1.0, 0.0, 0.0, 1.0]),
        (&corrected, [0.0, 1.0, 0.0, 1.0]),
    ] {
        host.asset_resources_mut()
            .register_client_source(world, source.clone(), skin_motion_clip(color).encode())
            .unwrap();
    }
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }

    let owner = background_skin_owner(&mut host, world, panel);
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(8, at))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0],
    );
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());

    // A corrected motion reference is a fresh intent. It establishes a new
    // private controller without replaying the permanently refused request.
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 120,
            operations: vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
                name: GuiRoot::part_property_name(GuiNodeId(2), "background_hovered", "motion")
                    .unwrap(),
                value: DynamicValue::Asset(corrected),
            }],
        })
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_some());
    host.world_mut(world).unwrap().step(1.0).unwrap();
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0],
    );
}

#[test]
fn malformed_ready_skin_tracks_refuse_once_and_use_static_destination() {
    let sources = [
        skin_motion_source("skin-idle"),
        skin_motion_source("skin-hover-malformed"),
        skin_motion_source("skin-pressed"),
    ];
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    host.asset_resources_mut()
        .register_client_source(
            world,
            sources[0].clone(),
            skin_motion_clip([0.25, 0.25, 0.25, 1.0]).encode(),
        )
        .unwrap();
    let mut malformed = skin_motion_tracks([0.0, 1.0, 0.0, 1.0]);
    malformed.pop();
    host.asset_resources_mut()
        .register_client_source(
            world,
            sources[1].clone(),
            AnimationClip::new(1.0, malformed).unwrap().encode(),
        )
        .unwrap();
    host.asset_resources_mut()
        .register_client_source(
            world,
            sources[2].clone(),
            skin_motion_clip([1.0, 0.0, 0.0, 1.0]).encode(),
        )
        .unwrap();
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }

    let owner = background_skin_owner(&mut host, world, panel);
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(9, at))
        .unwrap();
    for _ in 0..5 {
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0],
    );
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    assert!(
        host.world_mut(world)
            .unwrap()
            .animation_controllers()
            .is_empty()
    );
}

#[test]
fn delayed_skin_assets_retry_and_terminal_failure_stays_private() {
    let sources = ["idle", "hover", "pressed"].map(|name| AssetSource {
        kind: ANIMATION_TYPE,
        uri: format!("https://skins.test/{name}.ippa"),
        variant: 0,
    });
    let (mut host, world, panel) = skin_panel_with_root(animated_skinned_root(&sources));
    host.world_mut(world)
        .unwrap()
        .register_stream_resource_provider("https")
        .unwrap();
    let owner = background_skin_owner(&mut host, world, panel);
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(10, at))
        .unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0],
    );

    let mut requests = Vec::new();
    for _ in 0..16 {
        host.progress_assets();
        requests.extend(host.world_mut(world).unwrap().take_resource_requests());
        if requests.len() == sources.len() {
            break;
        }
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    assert_eq!(requests.len(), sources.len());
    for request in requests {
        let color = if request.source.ends_with("idle.ippa") {
            [0.25, 0.25, 0.25, 1.0]
        } else if request.source.ends_with("hover.ippa") {
            [0.0, 1.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0, 1.0]
        };
        host.world_mut(world)
            .unwrap()
            .complete_resource(request.id, Ok(skin_motion_clip(color).encode()))
            .unwrap();
    }
    for _ in 0..16 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
        if skin_controller_snapshot(&mut host, world, owner).is_some() {
            break;
        }
    }
    let private = skin_controller_snapshot(&mut host, world, owner).unwrap();
    assert!(
        host.world_mut(world)
            .unwrap()
            .animation_controller(private.id)
            .is_none()
    );

    let hover_key = host.asset_resources().find(&sources[1]).unwrap();
    host.asset_resources_mut().unload(hover_key);
    let mut recovery = None;
    for _ in 0..16 {
        host.progress_assets();
        recovery = host
            .world_mut(world)
            .unwrap()
            .take_resource_requests()
            .into_iter()
            .find(|request| request.source == sources[1].uri);
        if recovery.is_some() {
            break;
        }
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    let recovery = recovery.unwrap();
    host.world_mut(world)
        .unwrap()
        .complete_resource(recovery.id, Err("InvalidAsset".into()))
        .unwrap();
    let mut playback_events = Vec::new();
    for _ in 0..16 {
        host.progress_assets();
        let report = host.world_mut(world).unwrap().step(0.0).unwrap();
        playback_events.extend(report.playback_events);
        if skin_controller_snapshot(&mut host, world, owner).is_none() {
            break;
        }
    }
    assert!(
        playback_events
            .iter()
            .all(|event| event.controller.id != private.id)
    );
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    assert_color_near(
        panel_box_color(&mut host, world, panel),
        [0.0, 1.0, 0.0, 1.0],
    );
}

#[test]
fn skinned_paint_discards_drag_off_and_cancelled_presses() {
    let (mut host, world, panel) = skin_panel();
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));
    // Press, then drag outside the press-time rectangle: the tap dies with
    // a cancellation and pressed paint falls back to idle.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_down(1, at))
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [1.0, 0.0, 0.0, 1.0]
    );

    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_move(1, [9.0, 9.0]))
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_input_cancellations
                .iter()
                .any(|cancellation| cancellation.reason == GuiInputCancelReason::GestureCancelled)
        );
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0]
    );

    // Releasing outside commits nothing.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_up(1, [9.0, 9.0]))
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert!(
            report
                .gui_input_effects
                .iter()
                .all(|effect| !matches!(effect.kind, GuiInputEffectKind::ControlCommitted { .. }))
        );
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(false), 1)
    );

    // A fresh press cancelled explicitly also commits nothing.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_down(1, at))
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [1.0, 0.0, 0.0, 1.0]
    );

    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::PointerCancel {
                    pointer: 1,
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(false), 1)
    );
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.25, 0.25, 0.25, 1.0]
    );
}

#[test]
fn disabled_nodes_never_activate_and_paint_disabled() {
    let (mut host, world, panel) = skin_panel();
    let at = node_centre(&mut host, world, panel, GuiNodeId(2));

    // Authored disable through the ordinary patch path.
    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(2), 1),
                    patch: GuiNodePatch {
                        enabled: Some(false),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.5, 0.5, 0.5, 1.0]
    );

    // A press on the disabled node never activates.
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_input_command(SESSION, pointer_down(1, at))
            .unwrap();
        let report = context.step(0.0).unwrap();
        assert_eq!(report.gui_unhandled_inputs.len(), 1);
        assert_eq!(
            report.gui_unhandled_inputs[0].reason,
            GuiUnhandledReason::NotFocusable
        );
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        committed_bool(&mut host, world, panel),
        (GuiControlValue::Bool(false), 1)
    );
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.5, 0.5, 0.5, 1.0]
    );
}

/// Minimal zero-layer IPPD payload: magic, version 1, view box, bounds,
/// tolerance and no layers.
fn drawing_fixture_bytes() -> Vec<u8> {
    let mut bytes = b"IPPD".to_vec();
    bytes.extend(1u32.to_le_bytes());

    for bound in [0.0f32, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 10.0] {
        bytes.extend(bound.to_le_bytes());
    }

    bytes.extend(0.5f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes
}

fn drawing_source(uri: &str) -> AssetSource {
    AssetSource {
        kind: DRAWING_TYPE,
        uri: uri.to_owned(),
        variant: 0,
    }
}

fn drawing_decoded(host: &HostRuntime, source: &AssetSource) -> bool {
    host.asset_resources()
        .find(source)
        .and_then(|key| host.asset_resources().get(key))
        .is_some_and(|provider| provider.decoded_available())
}

fn register_drawing(host: &mut HostRuntime, world: WorldId, uri: &str, pump: bool) -> AssetSource {
    let source = drawing_source(uri);
    host.asset_resources_mut()
        .register_client_source(world, source.clone(), drawing_fixture_bytes())
        .unwrap();

    if pump {
        for _ in 0..16 {
            host.progress_assets();

            if drawing_decoded(host, &source) {
                break;
            }
        }

        assert!(
            drawing_decoded(host, &source),
            "drawing fixture must decode through shared loading"
        );
    }

    source
}

/// Drawing panel with a ready base asset and a skin part asset behind the
/// base part: the part swaps the prepared resource through the runtime
/// resource mechanism. Pending parts retain the prior resource while
/// resolved color lanes still apply.
fn drawing_panel(
    part_uri: &str,
    part_ready: bool,
) -> (
    HostRuntime,
    WorldId,
    crate::EntityId,
    AssetSource,
    AssetSource,
) {
    let mut host = HostRuntime::new();
    let world = host.create_world(Default::default()).unwrap();
    let base = drawing_source("skin-base");
    host.asset_resources_mut()
        .register_client_source(world, base.clone(), drawing_fixture_bytes())
        .unwrap();

    for _ in 0..16 {
        host.progress_assets();

        if drawing_decoded(&host, &base) {
            break;
        }
    }

    assert!(
        drawing_decoded(&host, &base),
        "drawing fixture must decode through shared loading"
    );

    let part = register_drawing(&mut host, world, part_uri, part_ready);

    let mut root = GuiRoot::default();
    part_color(&mut root, 2, "icon", [1.0, 0.0, 0.0, 1.0]);
    part_asset(&mut root, 2, "icon", part.clone());

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
                        value: ComponentValue::GuiRoot(root),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };

    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        insert_drawing_tree(&mut context, panel, incarnation, base.clone());
    }

    (host, world, panel, base, part)
}

/// Drawing panel whose base and skin resources have only ordinary component
/// demand. The external stream owns recovery bytes; no client upload or
/// prepared-source producer pins either resource in the catalog.
fn demand_only_drawing_panel(
    prefix: &str,
) -> (
    HostRuntime,
    WorldId,
    crate::EntityId,
    AssetSource,
    AssetSource,
) {
    let mut host = HostRuntime::new();
    assert_eq!(host.asset_resources().idle_resident_bytes_target(), 0);
    host.register_stream_resource_provider("skin-retention")
        .unwrap();
    let world = host.create_world(Default::default()).unwrap();
    let base = drawing_source(&format!("skin-retention:///{prefix}-base.ippd"));
    let ready = drawing_source(&format!("skin-retention:///{prefix}-ready.ippd"));

    let mut root = GuiRoot::default();
    part_color(&mut root, 2, "icon", [1.0, 0.0, 0.0, 1.0]);
    part_asset(&mut root, 2, "icon", ready.clone());
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
                        value: ComponentValue::GuiRoot(root),
                    },
                ],
            })
            .unwrap();
        let report = context.step(0.0).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        insert_drawing_tree(&mut context, panel, incarnation, base.clone());
    }

    host.progress_assets();
    let requests = host.take_resource_requests();
    assert_eq!(requests.len(), 2);
    for source in [&base, &ready] {
        let request = requests
            .iter()
            .find(|request| request.source == source.uri)
            .expect("ordinary drawing demand must issue a source request");
        host.complete_resource(request.id, Ok(drawing_fixture_bytes()))
            .unwrap();
    }
    for _ in 0..16 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
        if drawing_decoded(&host, &base) && drawing_decoded(&host, &ready) {
            break;
        }
    }
    assert!(drawing_decoded(&host, &base));
    assert!(drawing_decoded(&host, &ready));
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);

    (host, world, panel, base, ready)
}

fn replace_skin_with_pending_drawing(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    uri: &str,
) -> (AssetSource, u64) {
    let pending = drawing_source(uri);
    let property = GuiRoot::part_property_name(GuiNodeId(2), "icon", "asset").unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::SetDynamicProperty {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                    name: property,
                    value: DynamicValue::Asset(pending.clone()),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    host.progress_assets();
    let request = host
        .take_resource_requests()
        .into_iter()
        .find(|request| request.source == pending.uri)
        .expect("pending replacement must issue ordinary source demand");
    (pending, request.id)
}

fn insert_drawing_tree(
    context: &mut WorldContext<'_>,
    panel: crate::EntityId,
    root_incarnation: u64,
    base: AssetSource,
) {
    for command in [
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
            content: GuiNodeContent::Drawing,
            style: GuiNodeStyle {
                width: Some(2.0),
                height: Some(1.0),
                asset: Some(base),
                ..Default::default()
            },
        },
    ] {
        context.enqueue_gui_command(SESSION, command).unwrap();
    }
    context.step(0.0).unwrap();
}

fn root_with_icon_asset(source: AssetSource) -> GuiRoot {
    let mut root = GuiRoot::default();
    part_asset(&mut root, 2, "icon", source);
    root
}

/// Resource URI and color of node 2 in the prepared Surface primitives.
fn panel_drawing(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
) -> (String, [f32; 4]) {
    host.world_mut(world)
        .unwrap()
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, _| {
            system
                .state
                .surface_items
                .iter()
                .find(|item| item.entity == panel)
                .unwrap()
                .primitives
                .iter()
                .find_map(|primitive| match primitive {
                    SurfaceRenderPrimitive::Drawing {
                        style,
                        drawing,
                    } if matches!(
                        style.identity,
                        SurfacePrimitiveIdentity::Gui(id)
                            if id.node == GuiNodeId(2)
                                && id.lifetime == 1
                                && id.part == GuiPrimitivePart::Icon
                    ) =>
                    {
                        Some((drawing.source.uri.clone(), style.color))
                    }
                    _ => None,
                })
                .unwrap()
        })
        .unwrap()
}

fn panel_root(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) -> GuiRoot {
    host.world_mut(world)
        .unwrap()
        .inspect(panel)
        .unwrap()
        .effective
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::GuiRoot(root) => Some(root),
            _ => None,
        })
        .unwrap()
}

fn render_skin_resources(
    host: &mut HostRuntime,
    world: WorldId,
) -> Vec<crate::systems::surface::SurfaceRenderResource> {
    host.world_mut(world)
        .unwrap()
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, _| {
            system.gui_skin_resources.values().cloned().collect()
        })
        .unwrap()
}

#[test]
fn asset_backed_skin_paints_through_runtime_resources() {
    let (mut host, world, panel, base, part) = drawing_panel("skin-ready", true);
    assert_ne!(base.uri, part.uri);

    // Settled paint swaps the base resource for the skin part asset while
    // the resolved color lane applies. Drawing leaves never carry press
    // cursors (only controls do), so the base part proves the mechanism.
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_drawing(&mut host, world, panel),
        (part.uri.clone(), [1.0, 0.0, 0.0, 1.0])
    );
}

#[test]
fn pending_skin_asset_retains_prior_resource_in_prepared_paint() {
    let (mut host, world, panel, base, part) = drawing_panel("skin-pending", false);
    assert!(
        !drawing_decoded(&host, &part),
        "pending skin asset must stay undecoded"
    );

    // The pending part retains the prior resource while the resolved color
    // lane still applies: the subset rule holds in prepared paint.
    host.world_mut(world).unwrap().step(0.0).unwrap();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_drawing(&mut host, world, panel),
        (base.uri.clone(), [1.0, 0.0, 0.0, 1.0])
    );
}

#[test]
fn pending_skin_replacement_retains_last_ready_skin_resource() {
    let (mut host, world, panel, base, ready) = drawing_panel("skin-ready-prior", true);
    let pending = register_drawing(&mut host, world, "skin-pending-next", false);
    assert_ne!(base.uri, ready.uri);
    assert!(!drawing_decoded(&host, &pending));

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);

    let property = GuiRoot::part_property_name(GuiNodeId(2), "icon", "asset").unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 2,
                operations: vec![Command::SetDynamicProperty {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                    name: property,
                    value: DynamicValue::Asset(pending),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);
}

#[test]
fn demand_only_skin_retention_survives_pending_replacement_and_releases_on_node_removal() {
    let (mut host, world, panel, _, ready) = demand_only_drawing_panel("replace-node");
    let ready_key = host.asset_resources().find(&ready).unwrap();
    let (replacement, request) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///replace-node-next.ippd",
    );

    // The authored lane now demands only the pending replacement. Advancing
    // the zero-budget release barrier must not evict the RenderSystem's
    // last-ready consumer or its prepared appearance.
    host.flush_resource_lifecycle();
    assert_eq!(host.asset_resources().find(&ready), Some(ready_key));
    assert!(drawing_decoded(&host, &ready));
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);

    host.complete_resource(request, Ok(drawing_fixture_bytes()))
        .unwrap();
    for _ in 0..16 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
        if drawing_decoded(&host, &replacement)
            && panel_drawing(&mut host, world, panel).0 == replacement.uri
        {
            break;
        }
    }
    assert_eq!(panel_drawing(&mut host, world, panel).0, replacement.uri);
    host.flush_resource_lifecycle();
    assert!(
        host.asset_resources().find(&ready).is_none(),
        "ready replacement must release the superseded derived consumer"
    );

    let (pending, _) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///replace-node-pending.ippd",
    );
    assert_eq!(panel_drawing(&mut host, world, panel).0, replacement.uri);
    let incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    host.flush_resource_lifecycle();
    assert!(render_skin_resources(&mut host, world).is_empty());
    assert!(host.asset_resources().find(&replacement).is_none());
    assert!(host.asset_resources().find(&pending).is_none());
}

#[test]
fn clearing_demand_only_skin_appearance_releases_before_pending_replacement() {
    let (mut host, world, panel, base, ready) = demand_only_drawing_panel("clear-appearance");
    let color = GuiRoot::part_property_name(GuiNodeId(2), "icon", "color").unwrap();
    let asset = GuiRoot::part_property_name(GuiNodeId(2), "icon", "asset").unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![
                    Command::RemoveDynamicProperty {
                        entity: EntityRef::Handle(panel),
                        component: ComponentValue::GUI_ROOT,
                        name: color,
                    },
                    Command::RemoveDynamicProperty {
                        entity: EntityRef::Handle(panel),
                        component: ComponentValue::GUI_ROOT,
                        name: asset,
                    },
                ],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }

    assert_eq!(panel_drawing(&mut host, world, panel).0, base.uri);
    host.flush_resource_lifecycle();
    assert!(render_skin_resources(&mut host, world).is_empty());
    assert!(host.asset_resources().find(&ready).is_none());

    let (pending, _) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///clear-appearance-pending.ippd",
    );
    assert!(!drawing_decoded(&host, &pending));
    assert_eq!(panel_drawing(&mut host, world, panel).0, base.uri);
    assert!(host.asset_resources().find(&ready).is_none());
    assert!(
        render_skin_resources(&mut host, world)
            .iter()
            .all(|resource| resource.source != ready)
    );
}

#[test]
fn derived_only_skin_demand_withdraws_before_post_unload_service_poll() {
    let (mut host, world, panel, _, ready) = demand_only_drawing_panel("unload-derived-only");
    let ready_key = host.asset_resources().find(&ready).unwrap();
    let (_pending, _) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///unload-derived-only-pending.ippd",
    );
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);

    host.asset_resources_mut().unload(ready_key);
    host.progress_assets();

    assert!(
        host.take_resource_requests()
            .into_iter()
            .all(|request| request.source != ready.uri),
        "applied unload must withdraw derived demand before the service polls"
    );
    assert!(host.asset_resources().find(&ready).is_none());
    assert!(
        render_skin_resources(&mut host, world)
            .iter()
            .all(|resource| resource.key != ready_key)
    );
}

#[test]
fn root_and_world_teardown_release_demand_only_retained_skin_resources() {
    let (mut host, world, panel, _, ready) = demand_only_drawing_panel("root-teardown");
    let (pending, _) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///root-teardown-next.ippd",
    );
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: context.tick() + 1,
                operations: vec![Command::RemoveComponent {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    host.flush_resource_lifecycle();
    assert!(render_skin_resources(&mut host, world).is_empty());
    assert!(host.asset_resources().find(&ready).is_none());
    assert!(host.asset_resources().find(&pending).is_none());

    let (mut host, world, panel, _, ready) = demand_only_drawing_panel("world-teardown");
    let (pending, _) = replace_skin_with_pending_drawing(
        &mut host,
        world,
        panel,
        "skin-retention:///world-teardown-next.ippd",
    );
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);
    assert!(host.destroy_world(world));
    assert!(host.asset_resources().find(&ready).is_none());
    assert!(host.asset_resources().find(&pending).is_none());
}

#[test]
fn root_replacement_cannot_reuse_a_retained_skin_resource() {
    let (mut host, world, panel, base, ready) = drawing_panel("skin-ready-old-root", true);
    let pending = register_drawing(&mut host, world, "skin-pending-new-root", false);

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);

    let mut replacement = panel_root(&mut host, world, panel);
    let property = GuiRoot::part_property_name(GuiNodeId(2), "icon", "asset").unwrap();
    replacement
        .properties
        .set(&property, DynamicValue::Asset(pending))
        .unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 3,
                operations: vec![Command::InsertComponentValue {
                    entity: EntityRef::Handle(panel),
                    value: ComponentValue::GuiRoot(replacement),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }

    assert_eq!(panel_drawing(&mut host, world, panel).0, base.uri);
    assert!(
        render_skin_resources(&mut host, world)
            .iter()
            .all(|resource| resource.source != ready)
    );
}

#[test]
fn root_removal_clears_retained_skin_resources_before_reinsertion() {
    let (mut host, world, panel, base, ready) = drawing_panel("skin-ready-removed-root", true);
    let pending = register_drawing(&mut host, world, "skin-pending-reinserted-root", false);

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 4,
                operations: vec![Command::RemoveComponent {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }
    assert!(render_skin_resources(&mut host, world).is_empty());

    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 5,
                operations: vec![Command::InsertComponentValue {
                    entity: EntityRef::Handle(panel),
                    value: ComponentValue::GuiRoot(root_with_icon_asset(pending)),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        insert_drawing_tree(&mut context, panel, incarnation, base.clone());
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, base.uri);
}

#[test]
fn released_skin_resource_is_purged_before_reacquisition() {
    let (mut host, world, panel, base, ready) = drawing_panel("skin-ready-released", true);
    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, ready.uri);
    let released = host.asset_resources().find(&ready).unwrap();

    host.asset_resources_mut().unload(released);
    host.flush_resource_lifecycle();
    assert!(
        render_skin_resources(&mut host, world)
            .iter()
            .all(|resource| resource.key != released)
    );
    host.world_mut(world)
        .unwrap()
        .with_system::<RenderSystem, _>(RenderSystem::ID, |system, _| {
            assert!(system.state.surface_items.iter().all(|item| {
                item.primitives
                    .iter()
                    .all(|primitive| !surface_primitive_uses_asset(primitive, released))
            }));
        })
        .unwrap();

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(panel_drawing(&mut host, world, panel).0, base.uri);

    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
        if panel_drawing(&mut host, world, panel).0 == ready.uri {
            return;
        }
    }
    panic!("released skin resource did not recover through ordinary demand");
}

#[test]
fn skin_property_edit_repaints_without_hover_or_layout_work() {
    let (mut host, world, panel) = skin_panel();
    host.world_mut(world).unwrap().step(0.0).unwrap();
    let before = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .map(|view| {
            (
                view.layout_revision,
                view.reflow_count,
                view.remeasure_count,
            )
        })
        .unwrap();

    let property =
        GuiRoot::part_property_name(GuiNodeId(2), "background_idle_unchecked", "color").unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 2,
                operations: vec![Command::SetDynamicProperty {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                    name: property,
                    value: DynamicValue::Vec4([0.1, 0.2, 0.9, 1.0]),
                }],
            })
            .unwrap();
        context.step(0.0).unwrap();
    }

    host.world_mut(world).unwrap().step(0.0).unwrap();
    assert_eq!(
        panel_box_color(&mut host, world, panel),
        [0.1, 0.2, 0.9, 1.0]
    );
    let after = host
        .world_mut(world)
        .unwrap()
        .gui_layout_view(panel)
        .map(|view| {
            (
                view.layout_revision,
                view.reflow_count,
                view.remeasure_count,
            )
        })
        .unwrap();
    assert_eq!(after, before);
}

/// One skin destination in a shared motion clip: time, colour, opacity, scale.
type SkinMotionSample = (f64, [f32; 4], f32, [f32; 2]);

/// One motion clip holding every destination at its own sample time, like
/// the gallery skins: idle, hovered, pressed and disabled share the tracks
/// that animate the idle `background` lanes.
fn shared_skin_motion_clip(samples: &[SkinMotionSample]) -> AnimationClip {
    let duration = samples.last().unwrap().0;
    let track = |lane: &str, value: &dyn Fn(&SkinMotionSample) -> DynamicValue| AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::GUI_ROOT,
            name: GuiRoot::part_property_name(GuiNodeId(2), "background", lane).unwrap(),
        },
        keys: samples
            .iter()
            .map(|sample| AnimationKeyframe {
                time: sample.0,
                value: AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
                    value(sample),
                )),
                interpolation: if sample.0 < duration {
                    AnimationInterpolation::Linear
                } else {
                    AnimationInterpolation::Step
                },
            })
            .collect(),
    };

    AnimationClip::new(
        duration,
        vec![
            track("color", &|sample| DynamicValue::Vec4(sample.1)),
            track("opacity", &|sample| DynamicValue::F32(sample.2)),
            track("scale", &|sample| DynamicValue::Vec2(sample.3)),
        ],
    )
    .unwrap()
}

fn set_node_enabled(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId, enabled: bool) {
    let mut context = host.world_mut(world).unwrap();
    let incarnation = context
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    context
        .enqueue_gui_command(
            SESSION,
            GuiCommand::UpdateNode {
                handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(2), 1),
                patch: GuiNodePatch {
                    enabled: Some(enabled),
                    ..Default::default()
                },
            },
        )
        .unwrap();
}

fn skin_controller_refused(
    host: &mut HostRuntime,
    world: WorldId,
    owner: crate::systems::animation::GuiSkinAnimationOwner,
) -> Option<(u64, crate::ErrorReason)> {
    host.world_mut(world)
        .unwrap()
        .with_system::<crate::systems::animation::AnimationSystem, _>(
            crate::systems::animation::AnimationSystem::ID,
            |system, _| {
                system.skin_controller_rejection(owner).or_else(|| {
                    system
                        .skin_controller(owner)
                        .and_then(|(request, _, failure)| failure.map(|reason| (request, reason)))
                })
            },
        )
        .unwrap()
}

/// Base (authored) and effective idle `background` lanes of node 2.
fn idle_background_lanes(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
) -> [crate::systems::gui::GuiPartStyle; 2] {
    let inspected = host.world_mut(world).unwrap().inspect(panel).unwrap();
    let lanes = |values: Vec<ComponentValue>| {
        let root = values
            .into_iter()
            .find_map(|value| match value {
                ComponentValue::GuiRoot(root) => Some(root),
                _ => None,
            })
            .unwrap();
        crate::systems::gui::part_style(&root, GuiNodeId(2), "background")
    };
    [lanes(inspected.base), lanes(inspected.effective)]
}

/// How an unrelated edit reaches the GuiRoot while a skin transition runs.
#[derive(Clone, Copy, Debug)]
enum UnrelatedGuiEdit {
    /// A GUI command queued for the frame's mutation boundary.
    Queued,
    /// A Host command chunk applied before the frame.
    Streamed,
    /// A routed click that commits another control's value at Accept.
    Input,
}

/// Step one frame after an unrelated GUI edit, as React and input commit
/// other nodes while a skin transition runs.
fn step_with_unrelated_edit(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    frame: u32,
    kind: UnrelatedGuiEdit,
) {
    if let UnrelatedGuiEdit::Input = kind {
        let at = node_centre(host, world, panel, GuiNodeId(3));
        let mut context = host.world_mut(world).unwrap();
        if frame.is_multiple_of(4) {
            context
                .enqueue_gui_input_command(SESSION, pointer_down(9, at))
                .unwrap();
            context
                .enqueue_gui_input_command(SESSION, pointer_up(9, at))
                .unwrap();
        }
        context.step(1.0 / 60.0).unwrap();
        return;
    }

    let mut context = host.world_mut(world).unwrap();
    let incarnation = context
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let edit = GuiCommand::UpdateNode {
        handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(1), 1),
        patch: GuiNodePatch {
            opacity: Some(if frame.is_multiple_of(2) {
                1.0
            } else {
                0.99
            }),
            ..Default::default()
        },
    };
    if let UnrelatedGuiEdit::Queued = kind {
        context.enqueue_gui_command(SESSION, edit).unwrap();
    } else {
        // Like the Host, defer the chunk while earlier ingress drains.
        let mut deferred = 0;
        while context.has_deferred_world_input() {
            context.step(0.0).unwrap();
            deferred += 1;
            assert!(deferred < 4, "ingress did not drain");
        }
        let outcome = context
            .apply_gui_command_chunk(SESSION, u64::from(frame) + 1, vec![edit])
            .unwrap();
        assert_eq!((outcome.applied, outcome.result), (1, Ok(())));
        context.finish_command_stream();
    }
    context.step(1.0 / 60.0).unwrap();
}

#[test]
fn re_enabled_control_transitions_back_to_idle_through_a_shared_motion_clip() {
    let idle = ([0.38, 0.85, 1.0, 0.9], 1.0, [1.0, 1.0]);
    let disabled = ([0.2, 0.35, 0.42, 0.45], 0.45, [1.0, 1.0]);
    let samples = [
        (0.0, idle.0, idle.1, idle.2),
        (0.1, [0.78, 0.96, 1.0, 1.0], 1.0, [1.025, 1.025]),
        (0.2, [0.2, 0.65, 0.88, 0.9], 1.0, [0.985, 0.985]),
        (0.3, disabled.0, disabled.1, disabled.2),
        (0.4, [0.2, 0.8, 0.94, 1.0], 1.0, [1.0, 1.0]),
    ];
    let source = skin_motion_source("skin-shared");
    let mut root = GuiRoot::default();
    for (part, sample) in [
        ("background", samples[0]),
        ("background_hovered", samples[1]),
        ("background_pressed", samples[2]),
        ("background_disabled", samples[3]),
    ] {
        part_color(&mut root, 2, part, sample.1);
        part_f32(&mut root, 2, part, "opacity", sample.2);
        part_vec2(&mut root, 2, part, "scale", sample.3);
        part_motion(&mut root, 2, part, source.clone());
        part_f32(&mut root, 2, part, "duration", 0.2);
        part_f32(&mut root, 2, part, "easing", 0.0);
        part_f32(&mut root, 2, part, "track", 0.0);
        // Lane times are f32 like React's authored lanes.
        part_f32(&mut root, 2, part, "time", sample.0 as f32);
    }
    let (mut host, world, panel) = skin_panel_with_root(root);
    host.asset_resources_mut()
        .register_client_source(
            world,
            source.clone(),
            shared_skin_motion_clip(&samples).encode(),
        )
        .unwrap();
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    // A second checkbox whose routed clicks commit unrelated control values.
    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::InsertNode {
                    entity: panel,
                    root_incarnation: incarnation,
                    id: GuiNodeId(3),
                    parent: Some(GuiNodeId(1)),
                    index: 1,
                    content: GuiNodeContent::Checkbox {
                        checked: false,
                    },
                    style: GuiNodeStyle {
                        width: Some(2.0),
                        height: Some(1.0),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        context.step(0.0).unwrap();
    }
    let owner = background_skin_owner(&mut host, world, panel);
    assert_color_near(panel_box_color(&mut host, world, panel), idle.0);

    // Every frame also edits the GuiRoot elsewhere. Queued edits share each
    // transition request's mutation boundary; streamed edits and committed
    // input stage the GuiRoot between frames while skin output is retained.
    let mut frame = 0;
    for (cycle, edits) in [
        (0, UnrelatedGuiEdit::Queued),
        (1, UnrelatedGuiEdit::Streamed),
        (2, UnrelatedGuiEdit::Input),
    ] {
        // Idle -> disabled settles on the disabled sample.
        set_node_enabled(&mut host, world, panel, false);
        for _ in 0..24 {
            step_with_unrelated_edit(&mut host, world, panel, frame, edits);
            frame += 1;
            assert_eq!(skin_controller_refused(&mut host, world, owner), None);
        }
        assert_color_near(panel_box_color(&mut host, world, panel), disabled.0);
        let [base, effective] = idle_background_lanes(&mut host, world, panel);
        assert!(
            (effective.opacity.unwrap() - disabled.1).abs() <= 1.0e-5,
            "cycle {cycle}"
        );
        assert_eq!(base.color, Some(idle.0), "cycle {cycle}");
        assert_eq!(base.opacity, Some(idle.1), "cycle {cycle}");

        // Disabled -> idle blends from the held disabled sample through
        // intermediate values and restores the authored idle lanes.
        set_node_enabled(&mut host, world, panel, true);
        let mut blended = false;
        for _ in 0..24 {
            step_with_unrelated_edit(&mut host, world, panel, frame, edits);
            frame += 1;
            assert_eq!(skin_controller_refused(&mut host, world, owner), None);
            let [base, _] = idle_background_lanes(&mut host, world, panel);
            assert_eq!(base.color, Some(idle.0), "cycle {cycle}");
            assert_eq!(base.opacity, Some(idle.1), "cycle {cycle}");
            let paint = panel_box_color(&mut host, world, panel);
            blended |=
                paint
                    .iter()
                    .zip(disabled.0.iter().zip(idle.0))
                    .all(|(value, (from, to))| {
                        from == &to || (value - from).abs() > 0.02 && (value - to).abs() > 0.02
                    });
        }
        assert!(blended, "cycle {cycle}: no intermediate transition paint");
        assert_color_near(panel_box_color(&mut host, world, panel), idle.0);
        let [base, effective] = idle_background_lanes(&mut host, world, panel);
        for lanes in [base, effective] {
            assert_color_near(lanes.color.unwrap(), idle.0);
            assert!(
                (lanes.opacity.unwrap() - idle.1).abs() <= 1.0e-5,
                "cycle {cycle}"
            );
            assert_eq!(lanes.scale, Some(idle.2), "cycle {cycle}");
        }
    }

    // The routed clicks committed the other checkbox's value each time.
    let clicked = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, Some(GuiNodeId(3)), 1, 4)
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == GuiNodeId(3))
        .unwrap()
        .control_revision;
    assert!(clicked > 10, "only {clicked} committed clicks");
}
