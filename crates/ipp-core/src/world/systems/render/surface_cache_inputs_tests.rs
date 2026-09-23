//! Prepared Surface cache inputs: authored policy publication, paint and
//! resource revisions for each input class, and GUI interaction priority.

use super::*;
use crate::services::asset_management::font::FONT_TYPE;
use crate::services::asset_management::{AssetSource, AssetUpload, AssetUploadIdentity};
use crate::systems::animation::{
    ANIMATION_TYPE, AnimationClip, AnimationControllerDescription, AnimationDriverDescription,
    AnimationInterpolation, AnimationKeyframe, AnimationPlaybackControl, AnimationTrack,
    AnimationTrackTarget, AnimationValue,
};
use crate::{
    Batch, Command, ComponentValue, DynamicValue, EntityRef, ErrorReason, FieldValue, FieldWrite,
    HostRuntime, Surface, SurfaceCache, SurfaceCommand, SurfaceItemContent, SurfaceItemId,
    SurfaceItemPatch, SurfaceItemStyle, WorldContext, WorldId, components::Transform,
};

/// Prepared cache inputs of one Surface: policy, paint and resource revisions.
type Published = (Option<SurfaceCachePolicy>, u64, u64);

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

fn font_source(asset: u64) -> AssetSource {
    AssetSource {
        kind: FONT_TYPE,
        uri: format!("asset://{}/{asset}", FONT_TYPE.0),
        variant: 0,
    }
}

fn update(world: &mut WorldContext<'_>, dt: f64) -> crate::WorldUpdateReport {
    world.prepare_update(dt).unwrap();
    world.poll_assets();
    world.step(dt).unwrap()
}

fn upload_font(world: &mut WorldContext<'_>, asset: u64) {
    world
        .enqueue_asset(AssetUpload {
            id: asset,
            key: AssetUploadIdentity {
                kind: FONT_TYPE,
                asset,
                variant: 0,
            },
            bytes: font_bytes(),
        })
        .unwrap();
    for _ in 0..32 {
        if !update(world, 0.0).assets.is_empty() {
            return;
        }
    }

    panic!("font {asset} did not complete");
}

fn label_surface(font: u64) -> Surface {
    let mut surface = Surface::default();
    surface
        .insert_item(
            0,
            SurfaceItemContent::Label("AAA".into()),
            SurfaceItemStyle {
                asset: Some(font_source(font)),
                ..Default::default()
            },
        )
        .unwrap();
    surface
}

/// A World with one ready font and one labelled Surface, opted in unless
/// `cache` is `None`.
fn label_world(cache: Option<SurfaceCache>) -> (HostRuntime, WorldId, crate::EntityId) {
    let mut host = HostRuntime::new();
    let id = host.create_world(Default::default()).unwrap();
    let mut world = host.world_mut(id).unwrap();
    upload_font(&mut world, 1);
    let mut operations = vec![
        Command::Create {
            alias: 1,
            metadata: Default::default(),
        },
        Command::InsertComponentValue {
            entity: EntityRef::Alias(1),
            value: ComponentValue::Surface(label_surface(1)),
        },
    ];
    operations.extend(cache.map(|cache| Command::InsertComponentValue {
        entity: EntityRef::Alias(1),
        value: ComponentValue::SurfaceCache(cache),
    }));
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let report = update(&mut world, 0.0);
    let entity = report.outcomes[0].result.as_ref().unwrap()[0].1;
    drop(world);
    (host, id, entity)
}

fn item<'a>(world: &'a WorldContext<'_>, entity: crate::EntityId) -> &'a SurfaceRenderItem {
    world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == entity)
        .expect("Surface is prepared")
}

fn published(world: &WorldContext<'_>, entity: crate::EntityId) -> Published {
    let item = item(world, entity);
    (item.cache, item.paint_revision, item.resource_revision)
}

fn run(world: &mut WorldContext<'_>, operations: Vec<Command>) -> Result<(), ErrorReason> {
    world
        .enqueue(Batch {
            id: 90,
            operations,
        })
        .unwrap();
    update(world, 0.0).outcomes[0]
        .result
        .as_ref()
        .map(|_| ())
        .map_err(|failure| failure.reason)
}

fn edit_item(world: &mut WorldContext<'_>, entity: crate::EntityId, patch: SurfaceItemPatch) {
    world
        .enqueue_surface_command(
            1,
            SurfaceCommand::Update {
                entity,
                id: SurfaceItemId(1),
                patch,
            },
        )
        .unwrap();
    update(world, 0.0);
}

fn default_policy() -> SurfaceCachePolicy {
    SurfaceCachePolicy::new(&SurfaceCache::default()).unwrap()
}

#[test]
fn direct_surfaces_publish_revisions_that_follow_paint_edits() {
    let (mut host, id, entity) = label_world(None);
    let mut world = host.world_mut(id).unwrap();
    update(&mut world, 0.0);
    let (policy, paint, resource) = published(&world, entity);
    assert_eq!(policy, None);
    assert!(paint > 0 && resource > 0 && paint != resource);
    assert!(!item(&world, entity).primitives.is_empty());

    // Unchanged frames hold both revisions.
    for _ in 0..3 {
        update(&mut world, 1.0 / 60.0);
        assert_eq!(published(&world, entity), (None, paint, resource));
    }

    // A paint edit advances only the paint revision, which then holds.
    edit_item(
        &mut world,
        entity,
        SurfaceItemPatch {
            opacity: Some(0.5),
            ..Default::default()
        },
    );
    let (_, edited, edited_resource) = published(&world, entity);
    assert!(edited > paint);
    assert_eq!(edited_resource, resource);
    update(&mut world, 1.0 / 60.0);
    assert_eq!(published(&world, entity), (None, edited, resource));
}

#[test]
fn opted_in_revisions_hold_across_unchanged_and_placement_only_frames() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let mut world = host.world_mut(id).unwrap();
    let (policy, paint, resource) = published(&world, entity);
    assert_eq!(policy, Some(default_policy()));
    assert!(paint > 0 && resource > 0 && paint != resource);

    for _ in 0..3 {
        update(&mut world, 1.0 / 60.0);
        assert_eq!(published(&world, entity), (policy, paint, resource));
    }

    let model = item(&world, entity).model;
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::Transform(Transform {
                x: 3.0,
                ..Default::default()
            }),
        }],
    )
    .unwrap();
    assert_ne!(
        item(&world, entity).model,
        model,
        "placement was re-prepared"
    );
    assert_eq!(published(&world, entity), (policy, paint, resource));
}

#[test]
fn authored_item_edits_and_resizing_advance_only_the_paint_revision() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let mut world = host.world_mut(id).unwrap();
    let (_, mut paint, resource) = published(&world, entity);

    for patch in [
        SurfaceItemPatch {
            opacity: Some(0.5),
            ..Default::default()
        },
        SurfaceItemPatch {
            color: Some([0.2, 0.4, 0.6, 1.0]),
            ..Default::default()
        },
        SurfaceItemPatch {
            content: Some(SurfaceItemContent::Label("AA".into())),
            ..Default::default()
        },
    ] {
        edit_item(&mut world, entity, patch.clone());
        let (_, next, next_resource) = published(&world, entity);
        assert!(next > paint, "{patch:?} advances the paint revision");
        assert_eq!(next_resource, resource, "{patch:?} keeps the same font");
        paint = next;
    }

    run(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
            field: FieldWrite {
                offset: std::mem::offset_of!(Surface, width) as u32,
                value: FieldValue::F32(2.5),
            },
        }],
    )
    .unwrap();
    let (_, resized, _) = published(&world, entity);
    assert!(resized > paint, "clip size is painted input");
    assert_eq!(item(&world, entity).clip_size[0], 2.5);

    // Rewriting an identical value re-prepares without a new revision.
    edit_item(
        &mut world,
        entity,
        SurfaceItemPatch {
            opacity: Some(0.5),
            ..Default::default()
        },
    );
    assert_eq!(published(&world, entity).1, resized);
}

#[test]
fn numeric_item_animation_advances_the_paint_revision_until_it_holds() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let mut world = host.world_mut(id).unwrap();
    let name = Surface::property_name(SurfaceItemId(1), "opacity").unwrap();
    let key = |time: f64, value: f32, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(crate::components::schema::FieldValue::Dynamic(
            DynamicValue::F32(value),
        )),
        interpolation,
    };
    let clip = AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: AnimationTrackTarget::DynamicProperty {
                component: ComponentValue::SURFACE,
                name: name.clone(),
            },
            keys: vec![
                key(0.0, 1.0, AnimationInterpolation::Linear),
                key(1.0, 0.0, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap();
    world
        .enqueue_asset(AssetUpload {
            id: 91,
            key: AssetUploadIdentity {
                kind: ANIMATION_TYPE,
                asset: 91,
                variant: 0,
            },
            bytes: clip.encode(),
        })
        .unwrap();
    for _ in 0..32 {
        if !update(&mut world, 0.0).assets.is_empty() {
            break;
        }
    }

    let controller = world
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: format!("asset://{}/91", ANIMATION_TYPE.0),
                variant: 0,
                track: 0,
                target: entity,
                property: AnimationTrackTarget::DynamicProperty {
                    component: ComponentValue::SURFACE,
                    name,
                },
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            ..Default::default()
        })
        .unwrap();
    world
        .enqueue_playback(controller, AnimationPlaybackControl::Play)
        .unwrap();
    update(&mut world, 0.0);

    let (_, mut paint, resource) = published(&world, entity);
    for _ in 0..3 {
        update(&mut world, 0.25);
        let (_, next, next_resource) = published(&world, entity);
        assert!(next > paint, "each sampled opacity repaints");
        assert_eq!(next_resource, resource);
        paint = next;
    }

    // The clip completes, then holds its final value.
    update(&mut world, 0.5);
    let held = published(&world, entity).1;
    update(&mut world, 0.25);
    update(&mut world, 0.25);
    assert_eq!(published(&world, entity).1, held);
}

#[test]
fn resource_readiness_and_replacement_advance_the_resource_revision() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let (_, paint, resource) = published(&host.world_mut(id).unwrap(), entity);
    let pending = AssetSource {
        kind: FONT_TYPE,
        uri: "fixture:///pending-font.ippf".into(),
        variant: 0,
    };

    // A pending font suppresses the label: its identity leaves the item.
    edit_item(
        &mut host.world_mut(id).unwrap(),
        entity,
        SurfaceItemPatch {
            asset: Some(Some(pending.clone())),
            ..Default::default()
        },
    );
    let world = host.world_mut(id).unwrap();
    assert!(item(&world, entity).primitives.is_empty());
    let (_, pending_paint, pending_resource) = published(&world, entity);
    assert!(pending_paint > paint && pending_resource > resource);
    drop(world);

    // Readiness brings the replacement identity into the item.
    host.asset_resources_mut()
        .register_client_source(id, pending, font_bytes())
        .unwrap();
    for _ in 0..8 {
        host.progress_assets();
        let mut world = host.world_mut(id).unwrap();
        update(&mut world, 0.0);
        if !item(&world, entity).primitives.is_empty() {
            break;
        }
    }

    let mut world = host.world_mut(id).unwrap();
    assert_eq!(item(&world, entity).primitives.len(), 1);
    let (_, ready_paint, ready_resource) = published(&world, entity);
    assert!(ready_paint > pending_paint && ready_resource > pending_resource);
    update(&mut world, 0.0);
    assert_eq!(
        published(&world, entity),
        (Some(default_policy()), ready_paint, ready_resource)
    );

    // Returning to the first font replaces the identity again.
    edit_item(
        &mut world,
        entity,
        SurfaceItemPatch {
            asset: Some(Some(font_source(1))),
            ..Default::default()
        },
    );
    let (_, _, replaced) = published(&world, entity);
    assert!(replaced > ready_resource);
    update(&mut world, 0.0);
    assert_eq!(published(&world, entity).2, replaced);
}

#[test]
fn tracked_revisions_follow_paint_and_resource_identity_but_not_placement() {
    let drawing = |entity, generation| SurfaceRenderItem {
        entity: crate::EntityId::from_bits(entity),
        model: [0.0; 16],
        anchor: [0.0; 3],
        clip_size: [1.0, 1.0],
        primitives: vec![SurfaceRenderPrimitive::Drawing {
            style: crate::systems::surface::SurfacePrimitiveStyle {
                identity: crate::systems::surface::SurfacePrimitiveIdentity::Authored(
                    SurfaceItemId(1),
                ),
                position: [0.0, 0.0],
                scale: [1.0, 1.0],
                color: [1.0; 4],
                opacity: 1.0,
                clip: None,
            },
            drawing: SurfaceRenderResource {
                key: AssetKey {
                    slot: 4,
                    generation,
                },
                source: font_source(4),
            },
        }],
        cache: None,
        paint_revision: 0,
        resource_revision: 0,
        #[cfg(feature = "gui")]
        interaction: false,
    };
    let revisions = |items: &[SurfaceRenderItem]| {
        items
            .iter()
            .map(|item| (item.paint_revision, item.resource_revision))
            .collect::<Vec<_>>()
    };
    let mut inputs = SurfaceCacheInputs::default();
    let mut items = vec![drawing(1, 1), drawing(2, 1)];
    let policy = |_| Some(default_policy());

    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(1, 2), (3, 4)]);
    assert!(
        items
            .iter()
            .all(|item| item.cache == Some(default_policy()))
    );

    // Placement and primitive identity never reach the revisions.
    items[0].model[12] = 5.0;
    items[0].anchor = [5.0, 0.0, 0.0];
    let SurfaceRenderPrimitive::Drawing {
        style,
        ..
    } = &mut items[0].primitives[0]
    else {
        unreachable!()
    };
    style.identity = crate::systems::surface::SurfacePrimitiveIdentity::Authored(SurfaceItemId(9));
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(1, 2), (3, 4)]);

    // A painted field advances only the paint revision.
    let SurfaceRenderPrimitive::Drawing {
        style,
        ..
    } = &mut items[1].primitives[0]
    else {
        unreachable!()
    };
    style.opacity = 0.5;
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(1, 2), (5, 4)]);

    // A replacement identity advances both.
    items[0] = SurfaceRenderItem {
        paint_revision: items[0].paint_revision,
        resource_revision: items[0].resource_revision,
        ..drawing(1, 2)
    };
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(6, 7), (5, 4)]);

    // Releasing a resource drops its primitive and identity.
    items[1].primitives.clear();
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(6, 7), (8, 9)]);
    items[1] = SurfaceRenderItem {
        paint_revision: items[1].paint_revision,
        resource_revision: items[1].resource_revision,
        ..drawing(2, 1)
    };
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(6, 7), (10, 11)]);

    // Opting out and back in each take fresh revisions.
    inputs.publish_with(&mut items, |entity| {
        (entity != crate::EntityId::from_bits(1)).then(default_policy)
    });
    assert_eq!(revisions(&items), [(12, 13), (10, 11)]);
    assert_eq!(items[0].cache, None);
    inputs.publish_with(&mut items, policy);
    assert_eq!(revisions(&items), [(14, 15), (10, 11)]);

    // A dropped item stops being tracked.
    items.remove(1);
    inputs.publish_with(&mut items, policy);
    assert_eq!(inputs.tracked.len(), 1);
}

#[test]
fn removing_and_restoring_the_policy_or_surface_takes_fresh_revisions() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let mut world = host.world_mut(id).unwrap();
    let (_, paint, resource) = published(&world, entity);
    let issued = paint.max(resource);

    run(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE_CACHE,
        }],
    )
    .unwrap();
    let (removed, paint, resource) = published(&world, entity);
    assert_eq!(removed, None);
    assert!(paint > issued && resource > issued);
    let issued = paint.max(resource);

    let policy = SurfaceCache {
        direct_distance: 0.0,
        texels_per_metre: 64.0,
        max_refresh_hz: 5.0,
    };
    run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::SurfaceCache(policy),
        }],
    )
    .unwrap();
    let (restored, paint, resource) = published(&world, entity);
    assert_eq!(restored, Some(SurfaceCachePolicy::new(&policy).unwrap()));
    assert!(paint > issued && resource > issued);
    let issued = paint.max(resource);

    // Deleting the entity drops its item; a replacement never reuses values.
    run(
        &mut world,
        vec![
            Command::Delete {
                entity: EntityRef::Handle(entity),
            },
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(label_surface(1)),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::SurfaceCache(policy),
            },
        ],
    )
    .unwrap();
    assert!(
        world
            .surface_render_items()
            .iter()
            .all(|item| item.entity != entity)
    );
    let replacement = world.surface_render_items()[0].entity;
    let (_, paint, resource) = published(&world, replacement);
    assert!(paint > issued && resource > issued);
}

#[test]
fn invalid_policies_are_rejected_without_changing_published_inputs() {
    let (mut host, id, entity) = label_world(Some(SurfaceCache::default()));
    let mut world = host.world_mut(id).unwrap();
    let before = published(&world, entity);

    for (offset, value) in [
        (
            std::mem::offset_of!(SurfaceCache, direct_distance),
            -1.0_f32,
        ),
        (std::mem::offset_of!(SurfaceCache, texels_per_metre), 0.0),
        (std::mem::offset_of!(SurfaceCache, max_refresh_hz), f32::NAN),
        (std::mem::offset_of!(SurfaceCache, max_refresh_hz), 1.0e6),
    ] {
        let result = run(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::SURFACE_CACHE,
                field: FieldWrite {
                    offset: offset as u32,
                    value: FieldValue::F32(value),
                },
            }],
        );
        assert_eq!(result, Err(ErrorReason::InvalidValue), "{offset} = {value}");
        assert_eq!(published(&world, entity), before);
    }

    // An invalid insertion on a direct Surface leaves it direct.
    let (mut host, id, entity) = label_world(None);
    let mut world = host.world_mut(id).unwrap();
    let before = published(&world, entity);
    let result = run(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::SurfaceCache(SurfaceCache {
                texels_per_metre: -4.0,
                ..Default::default()
            }),
        }],
    );
    assert_eq!(result, Err(ErrorReason::InvalidValue));
    assert_eq!(published(&world, entity), before);
    assert_eq!(before.0, None);
}

#[cfg(feature = "gui")]
mod gui {
    use super::*;
    use crate::systems::gui::{
        GuiCommand, GuiContainerKind, GuiInputCommand, GuiNodeContent, GuiNodeHandle, GuiNodeId,
        GuiNodePatch, GuiNodeStyle, GuiPointerButton, GuiRoot,
    };

    const SESSION: u64 = 7;

    fn skin_color(root: &mut GuiRoot, part: &str, color: [f32; 4]) {
        let name = GuiRoot::part_property_name(GuiNodeId(2), part, "color").unwrap();
        root.properties
            .set(&name, DynamicValue::Vec4(color))
            .unwrap();
    }

    /// Checkbox (node 2) skin colours for idle, hovered and pressed paint.
    fn skinned_root() -> GuiRoot {
        let mut root = GuiRoot::default();
        skin_color(&mut root, "background", [0.1, 0.1, 0.1, 1.0]);
        skin_color(
            &mut root,
            "background_idle_unchecked",
            [0.25, 0.25, 0.25, 1.0],
        );
        skin_color(&mut root, "background_hovered", [0.0, 1.0, 0.0, 1.0]);
        skin_color(&mut root, "background_pressed", [1.0, 0.0, 0.0, 1.0]);
        root
    }

    /// Opted-in GuiRoot panel holding a skinned checkbox (node 2), a ScrollView
    /// (node 3) over taller content with a second checkbox (node 5), and a
    /// font-backed text input (node 6).
    fn panel() -> (HostRuntime, WorldId, crate::EntityId) {
        let mut host = HostRuntime::new();
        let id = host.create_world(Default::default()).unwrap();
        let mut world = host.world_mut(id).unwrap();
        upload_font(&mut world, 1);
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
                        value: ComponentValue::Surface({
                            let mut surface = Surface::default();
                            surface.width = 10.0;
                            surface.height = 10.0;
                            surface
                        }),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::GuiRoot(skinned_root()),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::SurfaceCache(SurfaceCache::default()),
                    },
                ],
            })
            .unwrap();
        let panel = update(&mut world, 0.0).outcomes[0].result.as_ref().unwrap()[0].1;
        let root_incarnation = world
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        let sized = |width: f32, height: f32| GuiNodeStyle {
            width: Some(width),
            height: Some(height),
            background_color: Some([0.2, 0.2, 0.2, 1.0]),
            ..Default::default()
        };
        let node = |id: u32, parent: Option<u32>, index, content, style| GuiCommand::InsertNode {
            entity: panel,
            root_incarnation,
            id: GuiNodeId(id),
            parent: parent.map(GuiNodeId),
            index,
            content,
            style,
        };
        for command in [
            node(
                1,
                None,
                0,
                GuiNodeContent::Container(GuiContainerKind::Column),
                sized(10.0, 10.0),
            ),
            node(
                2,
                Some(1),
                0,
                GuiNodeContent::Checkbox {
                    checked: false,
                },
                sized(2.0, 1.0),
            ),
            node(
                3,
                Some(1),
                1,
                GuiNodeContent::Container(GuiContainerKind::ScrollView),
                sized(10.0, 4.0),
            ),
            node(
                4,
                Some(3),
                0,
                GuiNodeContent::Container(GuiContainerKind::Column),
                GuiNodeStyle::default(),
            ),
            node(
                5,
                Some(4),
                0,
                GuiNodeContent::Checkbox {
                    checked: false,
                },
                sized(10.0, 8.0),
            ),
            node(
                6,
                Some(1),
                2,
                GuiNodeContent::TextInput {
                    text: "AA".into(),
                    placeholder: String::new(),
                },
                GuiNodeStyle {
                    width: Some(6.0),
                    height: Some(1.0),
                    asset: Some(font_source(1)),
                    ..Default::default()
                },
            ),
        ] {
            world.enqueue_gui_command(SESSION, command).unwrap();
        }

        update(&mut world, 0.0);
        update(&mut world, 0.0);
        drop(world);
        (host, id, panel)
    }

    fn handle(world: &mut WorldContext<'_>, panel: crate::EntityId, node: u32) -> GuiNodeHandle {
        let root_incarnation = world
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(node), 1)
    }

    fn centre(world: &mut WorldContext<'_>, panel: crate::EntityId, node: u32) -> [f32; 2] {
        let rect = world
            .gui_layout_view(panel)
            .unwrap()
            .nodes
            .iter()
            .find(|record| record.node == GuiNodeId(node))
            .map(|record| record.rect)
            .unwrap();
        [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0]
    }

    /// Route inputs, then let queued effects apply at the next boundary.
    fn input(world: &mut WorldContext<'_>, commands: Vec<GuiInputCommand>) {
        for command in commands {
            world.enqueue_gui_input_command(SESSION, command).unwrap();
        }

        update(world, 0.0);
        update(world, 0.0);
    }

    fn pointer(kind: u8, position: [f32; 2]) -> GuiInputCommand {
        let (panel, pointer, button, blockers, panel_distance) =
            (None, 1, GuiPointerButton::Primary, Vec::new(), None);
        match kind {
            0 => GuiInputCommand::PointerMove {
                pointer,
                panel,
                position,
                blockers,
                panel_distance,
            },
            1 => GuiInputCommand::PointerDown {
                pointer,
                panel,
                position,
                button,
                blockers,
                panel_distance,
            },
            _ => GuiInputCommand::PointerUp {
                pointer,
                panel,
                position,
                button,
                blockers,
                panel_distance,
            },
        }
    }

    const MOVE: u8 = 0;

    const DOWN: u8 = 1;

    const UP: u8 = 2;

    /// Far outside every panel in logical units.
    const OUTSIDE: [f32; 2] = [500.0, 500.0];

    fn interaction(world: &WorldContext<'_>, panel: crate::EntityId) -> bool {
        item(world, panel).interaction
    }

    #[test]
    fn gui_paint_scroll_skin_and_caret_advance_the_paint_revision() {
        let (mut host, id, panel) = panel();
        let mut world = host.world_mut(id).unwrap();
        let (policy, mut paint, resource) = published(&world, panel);
        assert_eq!(policy, Some(default_policy()));
        assert!(!interaction(&world, panel));

        let mut advanced = |world: &mut WorldContext<'_>, class: &str| {
            let (_, next, next_resource) = published(world, panel);
            assert!(next > paint, "{class} advances the paint revision");
            assert_eq!(next_resource, resource, "{class} keeps resource identity");
            paint = next;
        };

        // Node paint without a layout change, on the unskinned checkbox.
        let unskinned = handle(&mut world, panel, 5);
        world
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: unskinned,
                    patch: GuiNodePatch {
                        background_color: Some(Some([0.9, 0.1, 0.1, 1.0])),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        update(&mut world, 0.0);
        advanced(&mut world, "node paint");

        // Layout: resizing the checkbox reflows the column.
        let checkbox = handle(&mut world, panel, 2);
        world
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: checkbox,
                    patch: GuiNodePatch {
                        width: Some(Some(3.0)),
                        height: Some(Some(1.5)),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        update(&mut world, 0.0);
        advanced(&mut world, "layout");

        // Theme: an authored skin value on the root.
        run(
            &mut world,
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
                name: GuiRoot::part_property_name(
                    GuiNodeId(2),
                    "background_idle_unchecked",
                    "color",
                )
                .unwrap(),
                value: DynamicValue::Vec4([0.3, 0.3, 0.6, 1.0]),
            }],
        )
        .unwrap();
        update(&mut world, 0.0);
        advanced(&mut world, "theme");

        // Hover selects skin paint and interaction priority together.
        let at = centre(&mut world, panel, 2);
        input(&mut world, vec![pointer(MOVE, at)]);
        advanced(&mut world, "hover skin");
        assert!(interaction(&world, panel));
        input(&mut world, vec![pointer(MOVE, OUTSIDE)]);
        advanced(&mut world, "hover exit skin");
        assert!(!interaction(&world, panel));

        // Scrolling translates the ScrollView content without reflow.
        let at = centre(&mut world, panel, 3);
        input(
            &mut world,
            vec![GuiInputCommand::Scroll {
                panel: None,
                position: at,
                delta: [0.0, 2.0],
                blockers: Vec::new(),
                panel_distance: None,
            }],
        );
        advanced(&mut world, "scroll");

        // Focusing the text input paints its caret; moving the caret and
        // composing repaint while committed text is unchanged.
        let text = handle(&mut world, panel, 6);
        input(
            &mut world,
            vec![GuiInputCommand::Focus {
                handle: text,
            }],
        );
        advanced(&mut world, "focus caret");
        assert!(interaction(&world, panel), "keyboard focus has priority");
        input(
            &mut world,
            vec![GuiInputCommand::SetTextSelection {
                start: 0,
                end: 0,
            }],
        );
        advanced(&mut world, "caret");
        input(
            &mut world,
            vec![GuiInputCommand::UpdateComposition {
                text: "A".into(),
                caret_start: 1,
                caret_end: 1,
            }],
        );
        advanced(&mut world, "composition");

        // Unchanged frames keep every revision.
        let settled = published(&world, panel);
        update(&mut world, 1.0 / 60.0);
        update(&mut world, 1.0 / 60.0);
        assert_eq!(published(&world, panel), settled);
    }

    #[test]
    fn interaction_priority_follows_press_capture_and_focus() {
        let (mut host, id, panel) = panel();
        let mut world = host.world_mut(id).unwrap();
        let at = centre(&mut world, panel, 2);

        // A press has priority; after release, hover and the focus the
        // press gave the control keep it.
        input(&mut world, vec![pointer(DOWN, at)]);
        assert!(world.gui_input_pressed(1).is_some());
        assert!(interaction(&world, panel), "press has priority");
        input(&mut world, vec![pointer(UP, at), pointer(MOVE, OUTSIDE)]);
        assert!(world.gui_input_pressed(1).is_none());
        assert!(world.gui_input_hover(1).is_none());
        assert_eq!(
            interaction(&world, panel),
            world.gui_input_focus().is_some()
        );

        let checkbox = handle(&mut world, panel, 2);
        input(
            &mut world,
            vec![GuiInputCommand::Focus {
                handle: checkbox,
            }],
        );
        assert!(interaction(&world, panel), "keyboard focus has priority");

        // Keyboard focus persists without a pointer until blur.
        for _ in 0..3 {
            update(&mut world, 1.0 / 60.0);
            assert!(interaction(&world, panel), "focus is not a transient");
        }

        input(&mut world, vec![GuiInputCommand::Blur]);
        assert!(!interaction(&world, panel));
    }

    #[test]
    fn disabling_or_removing_the_interacted_root_clears_priority() {
        let (mut host, id, panel) = panel();
        let mut world = host.world_mut(id).unwrap();
        let at = centre(&mut world, panel, 2);
        let checkbox = handle(&mut world, panel, 2);

        // Disabling the pressed and focused node cancels both.
        input(
            &mut world,
            vec![
                GuiInputCommand::Focus {
                    handle: checkbox,
                },
                pointer(DOWN, at),
            ],
        );
        assert!(interaction(&world, panel));
        world
            .enqueue_gui_command(
                SESSION,
                GuiCommand::UpdateNode {
                    handle: checkbox,
                    patch: GuiNodePatch {
                        enabled: Some(false),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        update(&mut world, 0.0);
        update(&mut world, 0.0);
        let hovered = world.gui_input_hover(1).is_some();
        assert_eq!(interaction(&world, panel), hovered);
        input(&mut world, vec![pointer(MOVE, OUTSIDE)]);
        assert!(!interaction(&world, panel));

        // Removing the root while hovered leaves the Surface without priority.
        let other = centre(&mut world, panel, 3);
        input(&mut world, vec![pointer(MOVE, other)]);
        assert!(interaction(&world, panel));
        run(
            &mut world,
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
            }],
        )
        .unwrap();
        update(&mut world, 0.0);
        assert!(!interaction(&world, panel));
        assert!(
            published(&world, panel).0.is_some(),
            "policy stays authored"
        );
    }
}
