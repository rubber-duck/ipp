//! GuiRoot identity fences, committed control values, single-content ownership and property lifecycle.
#![cfg(feature = "gui")]

mod support;
use support::WorldTestDriver;

use ipp_core::services::asset_management::{AssetUpload, AssetUploadIdentity};
use ipp_core::services::world_serialization::{
    WorldLoadOptions, WorldPersistenceLimits, WorldSnapshot,
};
use ipp_core::systems::animation::*;
use ipp_core::systems::gui::{
    GuiCommand, GuiContainerKind, GuiControlValue, GuiInputCommand, GuiKey, GuiNodeData,
    GuiNodeDataRow, GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiNodeStyleProperty,
    GuiPartId, GuiPartPatch, GuiPartProperty, GuiRoot, GuiSkinState,
};
use ipp_core::systems::surface::GuiPrimitivePart;
use ipp_core::{
    Batch, Command, ComponentValue, DynamicValue, EntityId, EntityRef, ErrorReason, FieldValue,
    FieldWrite, HostRuntime, Surface, SurfaceCommand, SurfaceItemContent, SurfaceItemId,
    SurfaceItemStyle, WorldContext, WorldId, WorldLimits,
};

const SESSION: u64 = 1;

fn host_world() -> (HostRuntime, WorldId) {
    let mut host = HostRuntime::new();
    let world = host.create_world(WorldLimits::default()).unwrap();
    (host, world)
}

fn submit(
    world: &mut WorldContext<'_>,
    operations: Vec<Command>,
) -> Result<Vec<(u32, EntityId)>, ErrorReason> {
    world
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    world
        .update_for_test(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .map_err(|error| error.reason)
}

fn create_root(world: &mut WorldContext<'_>, surface: Surface) -> EntityId {
    submit(
        world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(surface),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    )
    .unwrap()[0]
        .1
}

fn incarnation(world: &WorldContext<'_>, entity: EntityId) -> u64 {
    world
        .inspect_gui(entity, None, 1, 1)
        .unwrap()
        .root_incarnation
}

fn edit(world: &mut WorldContext<'_>, command: GuiCommand) -> Result<(), ErrorReason> {
    world
        .enqueue_gui_command_with_reply(SESSION, 7, command)
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.system_command_outcomes.len(), 1);
    report.system_command_outcomes[0].result
}

/// Node kind with its kind-specific scalars, as `InsertNode` authors them.
struct Node(GuiNodeData, GuiNodeDataRow);

impl From<GuiNodeData> for Node {
    fn from(data: GuiNodeData) -> Self {
        Self(data, GuiNodeDataRow::default())
    }
}

fn checkbox_node(checked: bool) -> Node {
    Node(GuiNodeData::Checkbox, GuiNodeDataRow::checkbox(checked))
}

fn insert(
    world: &mut WorldContext<'_>,
    entity: EntityId,
    id: u32,
    parent: Option<u32>,
    node: impl Into<Node>,
    style: GuiNodeStyle,
) -> Result<GuiNodeHandle, ErrorReason> {
    let Node(data, values) = node.into();
    let root_incarnation = incarnation(world, entity);
    edit(
        world,
        GuiCommand::InsertNode {
            entity,
            root_incarnation,
            id: GuiNodeId(id),
            parent: parent.map(GuiNodeId),
            index: u32::MAX,
            data,
            values,
            style,
        },
    )?;
    Ok(GuiNodeHandle::new(
        SESSION,
        entity,
        root_incarnation,
        GuiNodeId(id),
    ))
}

fn column() -> GuiNodeData {
    GuiNodeData::Container(GuiContainerKind::Column)
}

fn slider(value: f32, max: f32) -> Node {
    Node(
        GuiNodeData::Slider,
        GuiNodeDataRow::slider(value, 0.0, max, 0.0),
    )
}

/// Field write of one node style property.
fn style_write(node: u32, property: GuiNodeStyleProperty, value: DynamicValue) -> FieldWrite {
    FieldWrite {
        offset: GuiRoot::node_style_offset(GuiNodeId(node), property).unwrap(),
        value: FieldValue::Dynamic(value),
    }
}

fn root(world: &WorldContext<'_>, entity: EntityId) -> GuiRoot {
    world.gui_root(entity).unwrap().clone()
}

#[test]
fn ordered_gui_command_group_stops_at_first_failure() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    let root_incarnation = incarnation(&world, entity);
    let insert = |id| GuiCommand::InsertNode {
        entity,
        root_incarnation,
        id: GuiNodeId(id),
        parent: None,
        index: 0,
        data: column(),
        values: GuiNodeDataRow::default(),
        style: GuiNodeStyle::default(),
    };

    world
        .enqueue_gui_commands_with_reply(SESSION, 99, vec![insert(1), insert(1), insert(2)])
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();

    assert_eq!(report.system_command_outcomes.len(), 1);
    let outcome = &report.system_command_outcomes[0];
    assert_eq!(outcome.request_id, 99);
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.result, Err(ErrorReason::InvalidValue));
    assert!(root(&world, entity).nodes().node(GuiNodeId(1)).is_some());
    assert!(root(&world, entity).nodes().node(GuiNodeId(2)).is_none());
}

fn children(world: &WorldContext<'_>, entity: EntityId, id: u32) -> Vec<u32> {
    root(world, entity)
        .nodes()
        .node(GuiNodeId(id))
        .unwrap()
        .children
        .iter()
        .map(|id| id.0)
        .collect()
}

fn control(world: &WorldContext<'_>, entity: EntityId, id: u32) -> (GuiControlValue, u32) {
    let root = root(world, entity);
    let state = root.control_state(GuiNodeId(id)).unwrap();
    (state.value.clone(), state.revision)
}

fn set_field(entity: EntityId, offset: u32, bytes: Vec<u8>) -> Command {
    Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_ROOT,
        field: FieldWrite {
            offset,
            value: FieldValue::Bytes(bytes),
        },
    }
}

fn gui_field_bytes(root: &GuiRoot) -> (u32, Vec<u8>) {
    use ipp_core::components::schema::SchemaComponent;

    let (offset, value) = root.fields().into_iter().next().unwrap();
    let ipp_core::components::schema::FieldValue::Bytes(bytes) = value else {
        panic!("GUI structural field is bytes")
    };
    (offset, bytes)
}

#[test]
fn reordering_preserves_identity_and_handles_are_fenced() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    let parent = insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    let first = insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeData::Button {
            label: "A".into(),
        },
        Default::default(),
    )
    .unwrap();
    insert(
        &mut world,
        entity,
        3,
        Some(1),
        slider(0.5, 1.0),
        Default::default(),
    )
    .unwrap();
    assert_eq!(children(&world, entity, 1), [2, 3]);

    // Only the next allocated identity may be inserted.
    assert!(insert(&mut world, entity, 9, Some(1), column(), Default::default()).is_err());
    assert!(insert(&mut world, entity, 2, Some(1), column(), Default::default()).is_err());

    edit(
        &mut world,
        GuiCommand::MoveNode {
            handle: first,
            parent: Some(GuiNodeId(1)),
            index: 1,
        },
    )
    .unwrap();
    assert_eq!(children(&world, entity, 1), [3, 2]);
    assert!(world.validate_gui_node_handle(&first, SESSION).is_ok());

    let other_session = GuiNodeHandle {
        session: SESSION + 1,
        ..first
    };
    assert!(
        world
            .validate_gui_node_handle(&other_session, SESSION)
            .is_err()
    );
    assert!(
        edit(
            &mut world,
            GuiCommand::RemoveNode {
                handle: other_session,
            },
        )
        .is_err()
    );

    // A second root node, a cycle, or promotion beside the root are rejected before mutation.
    assert!(insert(&mut world, entity, 4, None, column(), Default::default()).is_err());
    assert!(
        edit(
            &mut world,
            GuiCommand::MoveNode {
                handle: parent,
                parent: Some(GuiNodeId(2)),
                index: 0,
            },
        )
        .is_err()
    );
    assert!(
        edit(
            &mut world,
            GuiCommand::MoveNode {
                handle: first,
                parent: None,
                index: 0,
            },
        )
        .is_err()
    );
    assert_eq!(children(&world, entity, 1), [3, 2]);

    // Replacing the GuiRoot component restarts identities under a new incarnation.
    let old_incarnation = incarnation(&world, entity);
    submit(
        &mut world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_ROOT,
        }],
    )
    .unwrap();
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::GuiRoot(GuiRoot::default()),
        }],
    )
    .unwrap();
    assert_ne!(incarnation(&world, entity), old_incarnation);
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    insert(&mut world, entity, 2, Some(1), column(), Default::default()).unwrap();
    assert!(world.validate_gui_node_handle(&first, SESSION).is_err());
    assert!(
        edit(
            &mut world,
            GuiCommand::RemoveNode {
                handle: first,
            },
        )
        .is_err()
    );
    assert_eq!(root(&world, entity).node_count(), 2);
}

#[test]
fn control_values_are_revision_gated_and_survive_compatible_content_edits() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    let handle = insert(
        &mut world,
        entity,
        2,
        Some(1),
        slider(0.25, 1.0),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.25), 1)
    );
    assert!(root(&world, entity).control_state(GuiNodeId(1)).is_none());

    let set = |expected_revision, value| GuiCommand::SetControlValue {
        handle,
        expected_revision,
        value: GuiControlValue::Scalar(value),
    };
    edit(&mut world, set(1, 0.75)).unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.75), 2)
    );

    // A stale revision or an out-of-range value conflicts without changing the committed value.
    assert!(edit(&mut world, set(1, 0.1)).is_err());
    assert!(edit(&mut world, set(2, 3.0)).is_err());
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.75), 2)
    );

    // Replaying authored data keeps the newer committed value.
    let update = |node: Node| GuiCommand::UpdateNode {
        handle,
        patch: GuiNodePatch {
            data: Some(node.0),
            values: Some(node.1),
            ..Default::default()
        },
    };
    edit(&mut world, update(slider(0.25, 1.0))).unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.75), 2)
    );

    // A same-kind bounds edit that excludes the runtime-owned value is
    // rejected atomically; neither content nor value/revision changes.
    assert!(edit(&mut world, update(slider(0.2, 0.5))).is_err());
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.75), 2)
    );
    edit(&mut world, set(2, 0.3)).unwrap();

    let inspected = world.inspect_gui(entity, Some(GuiNodeId(2)), 1, 1).unwrap();
    assert_eq!(
        inspected.nodes[0].control_value,
        GuiControlValue::Scalar(0.3)
    );
    assert_eq!(inspected.nodes[0].control_revision, 3);

    // Control -> non-control -> control keeps one monotonic revision per node
    // identity, so a delayed write from the first control cannot apply later.
    edit(&mut world, update(GuiNodeData::Text("off".into()).into())).unwrap();
    assert_eq!(control(&world, entity, 2), (GuiControlValue::None, 4));
    assert!(edit(&mut world, set(4, 0.4)).is_err());
    edit(&mut world, update(slider(0.1, 1.0))).unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.1), 5)
    );
    for stale in [1, 3, 4] {
        assert!(
            edit(&mut world, set(stale, 0.9)).is_err(),
            "revision {stale}"
        );
    }
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Scalar(0.1), 5)
    );
    edit(&mut world, set(5, 0.9)).unwrap();
}

#[test]
fn gui_text_limit_is_shared_by_authorship_controls_and_composition() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    let maximum = "a".repeat(ipp_core::MAX_GUI_TEXT_BYTES);
    let oversized = "a".repeat(ipp_core::MAX_GUI_TEXT_BYTES + 1);
    let handle = insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeData::TextInput {
            text: maximum.clone(),
            placeholder: String::new(),
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Text(maximum), 1)
    );

    assert!(
        edit(
            &mut world,
            GuiCommand::SetControlValue {
                handle,
                expected_revision: 1,
                value: GuiControlValue::Text(oversized.clone()),
            },
        )
        .is_err()
    );
    assert_eq!(control(&world, entity, 2).1, 1);
    assert!(
        insert(
            &mut world,
            entity,
            3,
            Some(1),
            GuiNodeData::Text(oversized.clone()),
            Default::default(),
        )
        .is_err()
    );
    assert_eq!(root(&world, entity).next_node_id(), 3);

    world
        .enqueue_gui_input_command_with_reply(
            SESSION,
            9,
            GuiInputCommand::UpdateComposition {
                text: oversized,
                caret_start: 0,
                caret_end: 0,
            },
        )
        .unwrap();
    let report = world.update_for_test(0.0).unwrap();
    assert_eq!(report.system_command_outcomes.len(), 1);
    assert_eq!(
        report.system_command_outcomes[0].result,
        Err(ErrorReason::InvalidValue)
    );
    assert_eq!(control(&world, entity, 2).1, 1);
}

#[test]
fn world_snapshot_preserves_gui_allocator_and_values_but_excludes_input_state() {
    let (mut host, world_id) = host_world();
    {
        let mut world = host.world_mut(world_id).unwrap();
        let mut surface = Surface::default();
        surface.width = 10.0;
        surface.height = 10.0;
        let entity = create_root(&mut world, surface);
        insert(
            &mut world,
            entity,
            1,
            None,
            column(),
            GuiNodeStyle {
                width: Some(10.0),
                height: Some(10.0),
                ..Default::default()
            },
        )
        .unwrap();
        let removed = insert(
            &mut world,
            entity,
            2,
            Some(1),
            checkbox_node(false),
            Default::default(),
        )
        .unwrap();
        let handle = insert(
            &mut world,
            entity,
            3,
            Some(1),
            checkbox_node(false),
            Default::default(),
        )
        .unwrap();
        edit(
            &mut world,
            GuiCommand::RemoveNode {
                handle: removed,
            },
        )
        .unwrap();
        edit(
            &mut world,
            GuiCommand::SetControlValue {
                handle,
                expected_revision: 1,
                value: GuiControlValue::Bool(true),
            },
        )
        .unwrap();

        world
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Focus {
                    handle,
                },
            )
            .unwrap();
        world.update_for_test(0.0).unwrap();
        world.update_for_test(0.0).unwrap();
        assert_eq!(world.gui_input_focus().unwrap().target.node, GuiNodeId(3));

        // Route but do not apply a toggle. The durable capture must retain
        // the committed `true` value and exclude this pending input action.
        world
            .enqueue_gui_input_command(
                SESSION,
                GuiInputCommand::Key {
                    key: GuiKey::Space,
                    pressed: true,
                },
            )
            .unwrap();
        world.update_for_test(0.0).unwrap();
        assert!(world.gui_input_has_deferred());
        assert_eq!(control(&world, entity, 3), (GuiControlValue::Bool(true), 2));
        assert_eq!(root(&world, entity).next_node_id(), 4);
    }

    let limits = WorldPersistenceLimits::default();
    let bytes = host.save_world(world_id, 123, limits).unwrap();
    let snapshot = WorldSnapshot::decode(&bytes, 123, limits).unwrap();
    let saved_root = snapshot.entities[0]
        .components
        .iter()
        .find_map(|component| match component {
            ComponentValue::GuiRoot(root) => Some(root),
            _ => None,
        })
        .unwrap();
    assert_eq!(saved_root.next_node_id(), 4);
    assert_eq!(
        saved_root.control_state(GuiNodeId(3)).unwrap().value,
        GuiControlValue::Bool(true)
    );

    let mut restored_host = HostRuntime::new();
    let restored_id = restored_host
        .load_world(
            &bytes,
            123,
            WorldLoadOptions::default(),
            WorldLimits::default(),
            limits,
        )
        .unwrap();
    let mut restored = restored_host.world_mut(restored_id).unwrap();
    let restored_entity = restored.entities()[0].id;
    let restored_root = root(&restored, restored_entity);
    assert_eq!(
        restored_root
            .nodes()
            .as_slice()
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![GuiNodeId(1), GuiNodeId(3)]
    );
    assert_eq!(restored_root.next_node_id(), 4);
    assert_eq!(
        control(&restored, restored_entity, 3),
        (GuiControlValue::Bool(true), 2)
    );
    assert!(restored.gui_input_focus().is_none());
    assert!(!restored.gui_input_has_deferred());

    let restored_incarnation = incarnation(&restored, restored_entity);
    insert(
        &mut restored,
        restored_entity,
        4,
        Some(1),
        checkbox_node(false),
        Default::default(),
    )
    .unwrap();
    assert_eq!(root(&restored, restored_entity).next_node_id(), 5);
    assert_ne!(restored_incarnation, 0);
}

#[test]
fn structural_fields_of_a_live_root_change_only_through_gui_commands() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    let handle = insert(
        &mut world,
        entity,
        2,
        Some(1),
        checkbox_node(false),
        Default::default(),
    )
    .unwrap();
    let saved = root(&world, entity);
    edit(
        &mut world,
        GuiCommand::SetControlValue {
            handle,
            expected_revision: 1,
            value: GuiControlValue::Bool(true),
        },
    )
    .unwrap();

    // Writing an older saved tree back cannot bypass revision or identity fences,
    // and neither can clearing the tree to restart identities.
    let (offset, bytes) = gui_field_bytes(&saved);
    assert_eq!(
        submit(&mut world, vec![set_field(entity, offset, bytes)]),
        Err(ErrorReason::InvalidValue)
    );
    let (offset, bytes) = gui_field_bytes(&GuiRoot::default());
    assert!(submit(&mut world, vec![set_field(entity, offset, bytes)]).is_err());
    assert_eq!(control(&world, entity, 2), (GuiControlValue::Bool(true), 2));

    // A new incarnation may carry a complete tree and committed values, as snapshot restore does.
    submit(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(entity),
            value: ComponentValue::GuiRoot(saved.clone()),
        }],
    )
    .unwrap();
    assert_eq!(
        control(&world, entity, 2),
        (GuiControlValue::Bool(false), 1)
    );
    assert!(world.validate_gui_node_handle(&handle, SESSION).is_err());

    // Control records must match their nodes exactly: drop the checkbox's
    // trailing record (id, revision, tag, value) and its count.
    use ipp_core::components::schema::SchemaComponent;
    let (offset, mut bytes) = gui_field_bytes(&saved);
    bytes.truncate(bytes.len() - 14);
    bytes.extend(0u32.to_le_bytes());
    let mut mismatched = saved.clone();
    assert!(
        mismatched
            .set_field(
                offset,
                ipp_core::components::schema::FieldValue::Bytes(bytes)
            )
            .is_err()
    );
}

#[test]
fn surface_content_has_one_owner_while_a_gui_root_is_attached() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let mut populated = Surface::default();
    populated
        .insert_item(0, SurfaceItemContent::Drawing, SurfaceItemStyle::default())
        .unwrap();
    let raw = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(populated.clone()),
            },
        ],
    )
    .unwrap()[0]
        .1;

    // Attaching even an empty GuiRoot to raw content is rejected without replacing it.
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(raw),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            }],
        )
        .is_err()
    );
    assert!(world.gui_root(raw).is_none());
    assert_eq!(world.surface(raw).unwrap().items().len(), 1);

    // Raw item authoring is rejected while an empty GuiRoot owns the Surface.
    let entity = create_root(&mut world, Surface::default());
    world
        .enqueue_surface_command(
            SESSION,
            SurfaceCommand::Insert {
                entity,
                id: SurfaceItemId(1),
                index: 0,
                content: SurfaceItemContent::Drawing,
                style: SurfaceItemStyle::default(),
            },
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    assert!(world.surface(entity).unwrap().items().is_empty());
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Surface(populated),
            }],
        )
        .is_err()
    );

    // Surface dimensions remain ordinary authored state.
    let width = {
        use ipp_core::components::schema::SchemaComponent;
        Surface::default().fields()[0].0
    };
    submit(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::SURFACE,
            field: FieldWrite {
                offset: width,
                value: FieldValue::F32(3.0),
            },
        }],
    )
    .unwrap();
    assert_eq!(world.surface(entity).unwrap().width, 3.0);
}

#[test]
fn style_is_stored_once_and_patches_preserve_omitted_values() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    let handle = insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeData::Text("A".into()),
        GuiNodeStyle {
            width: Some(2.0),
            color: [0.5, 0.25, 1.0, 1.0],
            font_size: 0.2,
            ..Default::default()
        },
    )
    .unwrap();

    // An ordinary row property write is not overwritten by a later data edit.
    submit(
        &mut world,
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_ROOT,
            field: style_write(
                1,
                GuiNodeStyleProperty::Color,
                DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0]),
            ),
        }],
    )
    .unwrap();
    edit(
        &mut world,
        GuiCommand::UpdateNode {
            handle,
            patch: GuiNodePatch {
                data: Some(GuiNodeData::Text("B".into())),
                opacity: Some(0.5),
                ..Default::default()
            },
        },
    )
    .unwrap();
    let style = root(&world, entity).style(GuiNodeId(1)).unwrap();
    assert_eq!(style.color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(style.opacity, 0.5);
    assert_eq!(style.font_size, 0.2);
    assert_eq!(style.width, Some(2.0));

    // Clearing an optional property makes it absent in the row.
    edit(
        &mut world,
        GuiCommand::UpdateNode {
            handle,
            patch: GuiNodePatch {
                width: Some(None),
                ..Default::default()
            },
        },
    )
    .unwrap();
    let snapshot = root(&world, entity);
    assert_eq!(snapshot.style(GuiNodeId(1)).unwrap().width, None);
    assert_eq!(snapshot.style_row(GuiNodeId(1)).unwrap().width, None);
    assert!(snapshot.properties.descriptors().is_empty());

    // Row properties only accept their declared type and range, and optional
    // properties alone may be cleared.
    for field in [
        style_write(1, GuiNodeStyleProperty::Width, DynamicValue::Vec4([1.0; 4])),
        style_write(1, GuiNodeStyleProperty::Opacity, DynamicValue::F32(2.0)),
        FieldWrite {
            offset: GuiRoot::node_style_offset(GuiNodeId(1), GuiNodeStyleProperty::Opacity)
                .unwrap(),
            value: FieldValue::Unset,
        },
        style_write(2, GuiNodeStyleProperty::Opacity, DynamicValue::F32(0.5)),
    ] {
        assert!(
            submit(
                &mut world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::GUI_ROOT,
                    field: field.clone(),
                }],
            )
            .is_err(),
            "{field:?}"
        );
    }
    assert_eq!(
        root(&world, entity).style(GuiNodeId(1)).unwrap().opacity,
        0.5
    );

    // Named properties are application extension values: node style names
    // are ordinary names that never reach the rows.
    for (name, value) in [
        ("node_1_opacity", DynamicValue::F32(0.25)),
        ("node_1_part_background_color", DynamicValue::F32(1.0)),
        ("unrelated", DynamicValue::F32(1.0)),
    ] {
        submit(
            &mut world,
            vec![Command::SetDynamicProperty {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_ROOT,
                name: name.into(),
                value,
            }],
        )
        .unwrap();
    }
    assert_eq!(
        root(&world, entity).style(GuiNodeId(1)).unwrap().opacity,
        0.5
    );
    assert_eq!(root(&world, entity).part_state().len(), 0);
}

#[test]
fn removing_a_subtree_removes_node_and_part_rows() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    let button = insert(
        &mut world,
        entity,
        2,
        Some(1),
        GuiNodeData::Button {
            label: "Go".into(),
        },
        GuiNodeStyle {
            min_width: Some(1.0),
            ..Default::default()
        },
    )
    .unwrap();
    insert(
        &mut world,
        entity,
        3,
        Some(2),
        GuiNodeData::Text("x".into()),
        Default::default(),
    )
    .unwrap();
    edit(
        &mut world,
        GuiCommand::UpdatePart {
            handle: button,
            part: GuiPrimitivePart::Background,
            patch: GuiPartPatch::default().set(
                GuiPartProperty::Color,
                DynamicValue::Vec4([1.0, 0.0, 0.0, 1.0]),
            ),
        },
    )
    .unwrap();
    assert!(
        root(&world, entity)
            .part_row(GuiNodeId(2), GuiPrimitivePart::Background)
            .is_some()
    );

    edit(
        &mut world,
        GuiCommand::RemoveNode {
            handle: button,
        },
    )
    .unwrap();
    let root = root(&world, entity);
    assert_eq!(root.node_count(), 1);
    assert!(root.part_state().is_empty());
    assert!(world.validate_gui_node_handle(&button, SESSION).is_err());
    // Identities are never reused after removal.
    assert!(insert(&mut world, entity, 2, Some(1), column(), Default::default()).is_err());
    insert(&mut world, entity, 4, Some(1), column(), Default::default()).unwrap();
}

#[test]
fn inspection_is_bounded_by_depth_and_count() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    for id in 2..=4 {
        insert(
            &mut world,
            entity,
            id,
            Some(1),
            column(),
            Default::default(),
        )
        .unwrap();
    }
    insert(&mut world, entity, 5, Some(2), column(), Default::default()).unwrap();
    let ids = |depth, limit| {
        world
            .inspect_gui(entity, None, depth, limit)
            .unwrap()
            .nodes
            .iter()
            .map(|node| node.id.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(1, 256), [1]);
    assert_eq!(ids(2, 256), [1, 2, 3, 4]);
    assert_eq!(ids(3, 256), [1, 2, 3, 4, 5]);
    assert_eq!(ids(32, 2), [1, 2]);
    let subtree = world
        .inspect_gui(entity, Some(GuiNodeId(2)), 32, 256)
        .unwrap();
    assert_eq!(subtree.nodes.len(), 2);
    assert!(world.inspect_gui(entity, Some(GuiNodeId(9)), 1, 1).is_err());
}

fn opacity_clip(start: f32, end: f32) -> AnimationClip {
    let key = |time, value, interpolation| AnimationKeyframe {
        time,
        value: AnimationValue::Field(ipp_core::components::schema::FieldValue::Dynamic(
            DynamicValue::F32(value),
        )),
        interpolation,
    };
    AnimationClip::new(
        1.0,
        vec![AnimationTrack {
            target: opacity_target(),
            keys: vec![
                key(0.0, start, AnimationInterpolation::Linear),
                key(1.0, end, AnimationInterpolation::Step),
            ],
        }],
    )
    .unwrap()
}

/// Node 1's opacity, addressed by its row offset.
fn opacity_target() -> AnimationTrackTarget {
    AnimationTrackTarget::AnimationProperty(AnimationProperty {
        component: ComponentValue::GUI_ROOT,
        offsets: vec![
            GuiRoot::node_style_offset(GuiNodeId(1), GuiNodeStyleProperty::Opacity).unwrap(),
        ],
    })
}

fn opacity_driver(target: EntityId, asset: u64) -> AnimationControllerDescription {
    AnimationControllerDescription {
        drivers: vec![AnimationDriverDescription {
            source: format!("asset://10/{asset}"),
            variant: 0,
            track: 0,
            target,
            property: opacity_target(),
            weight: 1.0,
            additive: false,
            reference_time: 0.0,
            repeat: false,
        }],
        speed: 1.0,
        ..Default::default()
    }
}

#[test]
fn gui_properties_are_sampled_and_transitioned_by_animation() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeData::Text("A".into()),
        Default::default(),
    )
    .unwrap();
    for (asset, clip) in [(1, opacity_clip(1.0, 0.0)), (2, opacity_clip(1.0, 1.0))] {
        world
            .enqueue_asset(AssetUpload {
                id: asset,
                key: AssetUploadIdentity {
                    kind: ANIMATION_TYPE,
                    asset,
                    variant: 0,
                },
                bytes: clip.encode(),
            })
            .unwrap();
        assert!(world.await_upload_for_test().assets[0].result.is_ok());
    }
    let opacity =
        |world: &WorldContext<'_>| root(world, entity).style(GuiNodeId(1)).unwrap().opacity;
    let controller = world
        .create_animation_controller(opacity_driver(entity, 1))
        .unwrap();
    world
        .control_animation_controller(controller, AnimationPlaybackControl::Play)
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    assert!(
        (opacity(&world) - 0.5).abs() < 1.0e-4,
        "{}",
        opacity(&world)
    );

    world
        .transition_animation_controller(
            controller,
            AnimationControllerTransition {
                description: opacity_driver(entity, 2),
                duration: 1.0,
                easing: AnimationTransitionEasing::Linear,
                start_time: AnimationTransitionStartTime::Restart,
            },
        )
        .unwrap();
    world.update_for_test(0.0).unwrap();
    world.update_for_test(0.5).unwrap();
    // Half-way through the fade from the finished source (0.0) toward the destination (1.0).
    assert!(
        (opacity(&world) - 0.5).abs() < 1.0e-4,
        "{}",
        opacity(&world)
    );
    world.update_for_test(0.5).unwrap();
    assert!(
        (opacity(&world) - 1.0).abs() < 1.0e-4,
        "{}",
        opacity(&world)
    );

    // Removing the animated node kills its row slot, which drops the binding
    // before the next sample.
    let handle = GuiNodeHandle::new(SESSION, entity, incarnation(&world, entity), GuiNodeId(1));
    edit(
        &mut world,
        GuiCommand::RemoveNode {
            handle,
        },
    )
    .unwrap();
    world.update_for_test(0.25).unwrap();
    let removed = root(&world, entity);
    assert!(removed.style_row(GuiNodeId(1)).is_none());
    assert_eq!(removed.node_style().len(), 0);
}

#[test]
fn overlays_override_gui_properties_but_not_gui_structure_or_raw_items() {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: ipp_core::EntityMetadata {
                    symbolic_id: Some("panel".into()),
                    classes: Vec::new(),
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(Surface::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    )
    .unwrap()[0]
        .1;
    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeData::Text("A".into()),
        Default::default(),
    )
    .unwrap();
    let declare = |component, fields| {
        vec![
            Command::CreateStateOverlayOwner {
                alias: 1,
            },
            Command::AttachEntityOverlayBinding {
                owner: StateOverlayRef::Alias(1),
                alias: 2,
                symbolic_id: "panel".into(),
                mode: EntityOverlayMode::Bound,
            },
            Command::AttachComponentStateOverlay {
                owner: StateOverlayRef::Alias(1),
                binding: StateOverlayRef::Alias(2),
                alias: 3,
                component,
                mode: ComponentOverlayMode::Bound,
                fields,
            },
        ]
    };

    let (nodes, tree) = gui_field_bytes(&GuiRoot::default());
    let tree_write = FieldWrite {
        offset: nodes,
        value: FieldValue::Bytes(tree),
    };
    assert!(
        submit(
            &mut world,
            declare(ComponentValue::GUI_ROOT, vec![tree_write])
        )
        .is_err()
    );

    let mut raw = Surface::default();
    raw.insert_item(0, SurfaceItemContent::Drawing, SurfaceItemStyle::default())
        .unwrap();
    let items = {
        use ipp_core::components::schema::SchemaComponent;
        raw.fields()
            .into_iter()
            .find_map(|(offset, value)| match value {
                ipp_core::components::schema::FieldValue::Bytes(bytes) => Some(FieldWrite {
                    offset,
                    value: FieldValue::Bytes(bytes),
                }),
                _ => None,
            })
            .unwrap()
    };
    assert!(submit(&mut world, declare(ComponentValue::SURFACE, vec![items])).is_err());
    assert!(world.surface(entity).unwrap().items().is_empty());

    // Committed control values and the whole row tables are GUI-owned.
    let checked = FieldWrite {
        offset: GuiRoot::node_data_offset(GuiNodeId(1), ipp_core::GuiNodeDataProperty::Checked)
            .unwrap(),
        value: FieldValue::Dynamic(DynamicValue::Bool(true)),
    };
    assert!(submit(&mut world, declare(ComponentValue::GUI_ROOT, vec![checked])).is_err());

    // A GUI style override applies through the ordinary sparse overlay path.
    submit(
        &mut world,
        declare(
            ComponentValue::GUI_ROOT,
            vec![style_write(
                1,
                GuiNodeStyleProperty::Opacity,
                DynamicValue::F32(0.25),
            )],
        ),
    )
    .unwrap();
    assert_eq!(
        root(&world, entity).style(GuiNodeId(1)).unwrap().opacity,
        0.25
    );
}

fn overlay_declaration(symbolic_id: &str, component: u16, fields: Vec<FieldWrite>) -> Vec<Command> {
    use ipp_core::{ComponentOverlayMode, EntityOverlayMode, StateOverlayRef};

    vec![
        Command::CreateStateOverlayOwner {
            alias: 1,
        },
        Command::AttachEntityOverlayBinding {
            owner: StateOverlayRef::Alias(1),
            alias: 2,
            symbolic_id: symbolic_id.into(),
            mode: EntityOverlayMode::Bound,
        },
        Command::AttachComponentStateOverlay {
            owner: StateOverlayRef::Alias(1),
            binding: StateOverlayRef::Alias(2),
            alias: 3,
            component,
            mode: ComponentOverlayMode::Bound,
            fields,
        },
    ]
}

fn overlay_batch(world: &mut WorldContext<'_>, operations: Vec<Command>) -> ipp_core::BatchOutcome {
    world
        .enqueue(Batch {
            id: 2,
            operations,
        })
        .unwrap();
    world.update_for_test(0.0).unwrap().outcomes.remove(0)
}

fn release_owner(outcome: &ipp_core::BatchOutcome) -> Command {
    let owner = outcome
        .state_overlays
        .iter()
        .find(|alias| alias.alias == 1)
        .unwrap()
        .id;
    Command::ReleaseStateOverlayOwner {
        owner: ipp_core::StateOverlayRef::Handle(owner),
    }
}

#[test]
fn raw_items_hidden_by_an_overlay_block_gui_ownership() {
    use ipp_core::components::schema::SchemaComponent;

    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let mut raw = Surface::default();
    raw.insert_item(0, SurfaceItemContent::Drawing, SurfaceItemStyle::default())
        .unwrap();
    let entity = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: ipp_core::EntityMetadata {
                    symbolic_id: Some("hidden".into()),
                    classes: Vec::new(),
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(raw),
            },
        ],
    )
    .unwrap()[0]
        .1;

    // Hide the authored items behind an overlay supplying an empty collection.
    let empty = Surface::default()
        .fields()
        .into_iter()
        .find_map(|(offset, value)| match value {
            ipp_core::components::schema::FieldValue::Bytes(bytes) => Some(FieldWrite {
                offset,
                value: FieldValue::Bytes(bytes),
            }),
            _ => None,
        })
        .unwrap();
    let hidden = overlay_batch(
        &mut world,
        overlay_declaration("hidden", ComponentValue::SURFACE, vec![empty]),
    );
    assert!(hidden.result.is_ok());
    assert!(world.surface(entity).unwrap().items().is_empty());

    // Attaching GUI ownership must see the restorable producer items.
    assert!(
        submit(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            }],
        )
        .is_err()
    );
    assert!(world.gui_root(entity).is_none());

    // Releasing the overlay restores the raw items with a single content owner.
    submit(&mut world, vec![release_owner(&hidden)]).unwrap();
    assert_eq!(world.surface(entity).unwrap().items().len(), 1);
    assert!(world.gui_root(entity).is_none());
}

#[test]
fn gui_properties_are_validated_on_field_and_overlay_writes() {
    use ipp_core::StateOverlayRef;

    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = submit(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: ipp_core::EntityMetadata {
                    symbolic_id: Some("lanes".into()),
                    classes: Vec::new(),
                },
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Surface(Surface::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::GuiRoot(GuiRoot::default()),
            },
        ],
    )
    .unwrap()[0]
        .1;
    insert(
        &mut world,
        entity,
        1,
        None,
        GuiNodeData::Text("A".into()),
        GuiNodeStyle {
            opacity: 0.75,
            ..Default::default()
        },
    )
    .unwrap();
    let opacity_offset =
        GuiRoot::node_style_offset(GuiNodeId(1), GuiNodeStyleProperty::Opacity).unwrap();
    let opacity =
        |world: &WorldContext<'_>| root(world, entity).style(GuiNodeId(1)).unwrap().opacity;

    // A generic field write of an out-of-range property value is rejected before it applies.
    let write = |value: f32| FieldWrite {
        offset: opacity_offset,
        value: FieldValue::Dynamic(DynamicValue::F32(value)),
    };
    assert!(
        submit(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_ROOT,
                field: write(2.0),
            }],
        )
        .is_err()
    );
    assert_eq!(opacity(&world), 0.75);

    // Theme track properties reject the rounded 2^32 F32 boundary and accept
    // the greatest representable F32 whose colour, opacity and scale tracks
    // fit u32, by row offset like any row property.
    let current = incarnation(&world, entity);
    edit(
        &mut world,
        GuiCommand::UpdateTheme {
            entity,
            root_incarnation: current,
            theme: 3,
            part: GuiPartId::base(GuiPrimitivePart::Background),
            patch: GuiPartPatch::default(),
        },
    )
    .unwrap();
    let theme_slot = root(&world, entity).theme_slot(3).unwrap();
    let track_offset = GuiRoot::theme_part_offset(
        GuiRoot::theme_part_slot(theme_slot, GuiPartId::base(GuiPrimitivePart::Background))
            .unwrap(),
        GuiPartProperty::Track,
    )
    .unwrap();
    let track = |value: f32| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_ROOT,
        field: FieldWrite {
            offset: track_offset,
            value: FieldValue::Dynamic(DynamicValue::F32(value)),
        },
    };
    assert!(submit(&mut world, vec![track(4_294_967_296.0)]).is_err());
    let theme_track = |world: &WorldContext<'_>| {
        root(world, entity)
            .theme_row(theme_slot, GuiPartId::base(GuiPrimitivePart::Background))
            .unwrap()
            .track
    };
    assert_eq!(theme_track(&world), None);
    submit(&mut world, vec![track(4_294_967_040.0)]).unwrap();
    assert_eq!(theme_track(&world), Some(4_294_967_040.0));

    // Overlay declarations and overlay updates follow the same property rules.
    assert!(
        overlay_batch(
            &mut world,
            overlay_declaration("lanes", ComponentValue::GUI_ROOT, vec![write(2.0)]),
        )
        .result
        .is_err()
    );
    assert_eq!(opacity(&world), 0.75);
    let mut operations = overlay_declaration("lanes", ComponentValue::GUI_ROOT, Vec::new());
    operations.push(Command::UpdateComponentStateOverlay {
        owner: StateOverlayRef::Alias(1),
        overlay: StateOverlayRef::Alias(3),
        fields: vec![write(-1.0)],
        clear: Vec::new(),
    });
    assert!(overlay_batch(&mut world, operations).result.is_err());
    assert_eq!(opacity(&world), 0.75);
    assert!(world.gui_root(entity).is_some());

    // A valid override applies and release restores the authored value.
    let valid = overlay_batch(
        &mut world,
        overlay_declaration("lanes", ComponentValue::GUI_ROOT, vec![write(0.25)]),
    );
    assert!(valid.result.is_ok());
    assert_eq!(opacity(&world), 0.25);
    submit(&mut world, vec![release_owner(&valid)]).unwrap();
    assert_eq!(opacity(&world), 0.75);
}

/// Theme edits reach every node that references the theme without writing
/// the nodes; overrides keep precedence and theme removal leaves the
/// references resolving without a theme until it is defined again.
#[test]
fn theme_edits_re_resolve_referencing_nodes_without_node_writes() {
    let (mut host, world_id) = host_world();
    let mut world = host.world_mut(world_id).unwrap();
    let entity = create_root(&mut world, Surface::default());
    let themed = GuiNodeStyle {
        theme: Some(4),
        ..Default::default()
    };
    insert(&mut world, entity, 1, None, column(), Default::default()).unwrap();
    insert(&mut world, entity, 2, Some(1), column(), themed.clone()).unwrap();
    let overridden = insert(&mut world, entity, 3, Some(1), column(), themed).unwrap();
    let update_theme = |world: &mut WorldContext<'_>, part, color| {
        let root_incarnation = incarnation(world, entity);
        edit(
            world,
            GuiCommand::UpdateTheme {
                entity,
                root_incarnation,
                theme: 4,
                part,
                patch: GuiPartPatch::default()
                    .set(GuiPartProperty::Color, DynamicValue::Vec4(color)),
            },
        )
    };
    let base = GuiPartId::base(GuiPrimitivePart::Background);
    let hovered = GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Hovered);
    update_theme(&mut world, base, [0.1, 0.2, 0.3, 1.0]).unwrap();
    update_theme(&mut world, hovered, [0.9, 0.8, 0.7, 1.0]).unwrap();
    edit(
        &mut world,
        GuiCommand::UpdatePart {
            handle: overridden,
            part: GuiPrimitivePart::Background,
            patch: GuiPartPatch::default().set(
                GuiPartProperty::Color,
                DynamicValue::Vec4([0.0, 1.0, 0.0, 1.0]),
            ),
        },
    )
    .unwrap();

    let resolved = |world: &WorldContext<'_>, node: u32, state| {
        ipp_core::systems::gui::resolve_state_part_style(
            &root(world, entity),
            GuiNodeId(node),
            GuiPrimitivePart::Background,
            state,
            ipp_core::systems::gui::GuiControlVariant::Plain,
        )
        .color
    };
    assert_eq!(
        resolved(&world, 2, GuiSkinState::Idle),
        Some([0.1, 0.2, 0.3, 1.0])
    );
    assert_eq!(
        resolved(&world, 2, GuiSkinState::Hovered),
        Some([0.9, 0.8, 0.7, 1.0])
    );
    assert_eq!(
        resolved(&world, 3, GuiSkinState::Hovered),
        Some([0.0, 1.0, 0.0, 1.0])
    );

    // Editing the theme changes resolution without touching node rows.
    let styles = |world: &WorldContext<'_>| {
        [2, 3].map(|node| root(world, entity).style_row(GuiNodeId(node)).cloned())
    };
    let before = styles(&world);
    update_theme(&mut world, base, [0.4, 0.4, 0.4, 1.0]).unwrap();
    assert_eq!(styles(&world), before);
    assert_eq!(
        resolved(&world, 2, GuiSkinState::Idle),
        Some([0.4, 0.4, 0.4, 1.0])
    );
    assert_eq!(
        resolved(&world, 3, GuiSkinState::Idle),
        Some([0.0, 1.0, 0.0, 1.0])
    );

    // Removing the theme leaves references resolving without it; defining it
    // again at a fresh theme slot restores them.
    let first_slot = root(&world, entity).theme_slot(4).unwrap();
    let current = incarnation(&world, entity);
    edit(
        &mut world,
        GuiCommand::RemoveTheme {
            entity,
            root_incarnation: current,
            theme: 4,
        },
    )
    .unwrap();
    assert_eq!(resolved(&world, 2, GuiSkinState::Idle), None);
    let current = incarnation(&world, entity);
    assert!(
        edit(
            &mut world,
            GuiCommand::RemoveTheme {
                entity,
                root_incarnation: current,
                theme: 4,
            },
        )
        .is_err()
    );
    update_theme(&mut world, base, [0.5, 0.5, 0.5, 1.0]).unwrap();
    assert_ne!(root(&world, entity).theme_slot(4), Some(first_slot));
    assert_eq!(
        resolved(&world, 2, GuiSkinState::Idle),
        Some([0.5, 0.5, 0.5, 1.0])
    );
    assert_eq!(styles(&world), before);

    // Motion is theme-only, and theme handles and whole tables are
    // command-owned on a live root.
    assert!(
        edit(
            &mut world,
            GuiCommand::UpdatePart {
                handle: overridden,
                part: GuiPrimitivePart::Background,
                patch: GuiPartPatch::default()
                    .set(GuiPartProperty::Duration, DynamicValue::F32(1.0)),
            },
        )
        .is_err()
    );
    assert!(
        submit(
            &mut world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_ROOT,
                field: FieldWrite {
                    offset: GuiRoot::node_style_offset(GuiNodeId(2), GuiNodeStyleProperty::Theme)
                        .unwrap(),
                    value: FieldValue::Dynamic(DynamicValue::U32(9)),
                },
            }],
        )
        .is_err()
    );
}
