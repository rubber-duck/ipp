//! GuiCommand writes derived from the edited node's lanes equal the writes
//! of the complete-root path they replace, over randomized edit sequences.

use super::*;
use crate::DynamicValue;
use crate::components::schema::SchemaField;
use crate::systems::gui::GuiContainerKind;

const SESSION: u64 = 3;
const INCARNATION: u64 = 5;

fn entity() -> EntityId {
    EntityId::from_bits(9)
}

/// The complete-root path: edit a full copy, validate every lane and diff
/// the complete roots. Returns the writes and the edited root.
fn full_edit(root: &GuiRoot, command: &GuiCommand) -> Result<(Vec<Command>, GuiRoot), ErrorReason> {
    match command {
        GuiCommand::InsertNode {
            root_incarnation,
            ..
        } if *root_incarnation != INCARNATION => return Err(ErrorReason::InvalidValue),
        GuiCommand::InsertNode {
            ..
        } => {}
        GuiCommand::UpdateNode {
            handle,
            ..
        }
        | GuiCommand::MoveNode {
            handle,
            ..
        }
        | GuiCommand::RemoveNode {
            handle,
        }
        | GuiCommand::SetControlValue {
            handle,
            ..
        } => validate_handle(root, INCARNATION, SESSION, handle)?,
    }

    let previous = root.clone();
    let mut next = root.clone();
    match command.clone() {
        GuiCommand::InsertNode {
            id,
            parent,
            index,
            content,
            style,
            ..
        } => {
            validate_node_style(&style).map_err(|_| ErrorReason::InvalidValue)?;
            next.nodes_mut()
                .insert_node(id, parent, index as usize, content.clone())
                .map_err(|_| ErrorReason::InvalidValue)?;
            next.controls_mut().insert_initial(id, &content);
            next.install_node_style(id, &style)?;
        }
        GuiCommand::UpdateNode {
            handle,
            patch,
        } => {
            if let Some(content) = patch.content.clone() {
                next.nodes_mut()
                    .replace_content(handle.node_id, content.clone())
                    .map_err(|_| ErrorReason::InvalidValue)?;
                next.controls_mut()
                    .reconcile_content(handle.node_id, &content)
                    .map_err(|_| ErrorReason::InvalidValue)?;
            }
            next.apply_patch(handle.node_id, &patch)?;
            let style = next
                .style(handle.node_id)
                .ok_or(ErrorReason::InvalidValue)?;
            validate_node_style(&style).map_err(|_| ErrorReason::InvalidValue)?;
        }
        GuiCommand::MoveNode {
            handle,
            parent,
            index,
        } => {
            next.nodes_mut()
                .move_node(handle.node_id, parent, index as usize)
                .map_err(|_| ErrorReason::InvalidValue)?;
        }
        GuiCommand::RemoveNode {
            handle,
        } => {
            let removed = next
                .nodes_mut()
                .remove_node(handle.node_id)
                .map_err(|_| ErrorReason::InvalidValue)?;
            for id in removed {
                next.controls_mut().remove(id);
                next.remove_node_properties(id);
            }
        }
        GuiCommand::SetControlValue {
            handle,
            expected_revision,
            value,
        } => {
            commit_control_value(
                &mut next,
                INCARNATION,
                SESSION,
                &handle,
                expected_revision,
                &value,
            )?;
        }
    }
    next.validate_complete()?;

    let mut commands = Vec::new();
    if previous.nodes() != next.nodes() {
        let crate::components::schema::FieldValue::Bytes(bytes) = next.nodes().to_value() else {
            unreachable!("GUI node tree is bytes")
        };
        commands.push(Command::SetField {
            entity: crate::EntityRef::Handle(entity()),
            component: crate::ComponentValue::GUI_ROOT,
            field: FieldWrite {
                offset: GuiRoot::nodes_field(),
                value: FieldValue::Bytes(bytes),
            },
        });
    }
    for (name, descriptor) in previous.properties.descriptors() {
        if !next.properties.descriptors().contains_key(name) {
            commands.push(Command::RemoveDynamicProperty {
                entity: crate::EntityRef::Handle(entity()),
                component: crate::ComponentValue::GUI_ROOT,
                name: name.clone(),
            });
        } else if next.properties.get(name) != previous.properties.get_key(descriptor.key) {
            commands.push(set_property(entity(), &next, name));
        }
    }
    for name in next.properties.descriptors().keys() {
        if !previous.properties.descriptors().contains_key(name) {
            commands.push(set_property(entity(), &next, name));
        }
    }
    Ok((commands, next))
}

struct Random(u32);

impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        self.next() as usize % bound.max(1)
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.next() % 100 < percent
    }

    fn unit(&mut self) -> f32 {
        (self.next() % 1000) as f32 / 1000.0
    }
}

fn content(random: &mut Random) -> GuiNodeContent {
    match random.below(7) {
        0 => GuiNodeContent::Container(GuiContainerKind::Column),
        1 => GuiNodeContent::Container(GuiContainerKind::Row),
        2 => GuiNodeContent::Text(format!("label {}", random.below(100))),
        3 => GuiNodeContent::Checkbox {
            checked: random.chance(50),
        },
        4 => GuiNodeContent::Slider {
            value: random.unit(),
            min: 0.0,
            max: 1.0,
            step: 0.0,
        },
        5 => GuiNodeContent::Button {
            label: "go".into(),
        },
        _ => GuiNodeContent::TextInput {
            text: "text".into(),
            placeholder: String::new(),
        },
    }
}

fn style(random: &mut Random) -> GuiNodeStyle {
    GuiNodeStyle {
        width: random.chance(60).then(|| random.unit() * 4.0),
        height: random.chance(60).then(|| random.unit()),
        padding: random.chance(30).then_some([0.01; 4]),
        background_color: random.chance(50).then(|| [random.unit(), 0.2, 0.3, 1.0]),
        opacity: random.unit(),
        ..Default::default()
    }
}

fn patch(random: &mut Random) -> GuiNodePatch {
    GuiNodePatch {
        content: random.chance(20).then(|| content(random)),
        enabled: random.chance(20).then(|| random.chance(50)),
        width: random
            .chance(40)
            .then(|| random.chance(80).then(|| random.unit() * 4.0)),
        // Occasionally invalid: a negative height fails validation.
        height: random.chance(10).then(|| Some(-1.0)),
        color: random.chance(30).then(|| [random.unit(), 0.5, 0.5, 1.0]),
        background_color: random
            .chance(30)
            .then(|| random.chance(70).then(|| [0.1, random.unit(), 0.1, 1.0])),
        opacity: random.chance(30).then(|| random.unit()),
        ..Default::default()
    }
}

fn handle(root: &GuiRoot, random: &mut Random) -> Option<GuiNodeHandle> {
    let nodes = root.nodes().as_slice();
    let node = nodes.get(random.below(nodes.len()))?;
    // Occasionally stale: a wrong lifetime fails the fence.
    let lifetime = node.lifetime + u32::from(random.chance(5));
    Some(GuiNodeHandle::new(
        SESSION,
        entity(),
        INCARNATION,
        node.id,
        lifetime,
    ))
}

fn command(root: &GuiRoot, random: &mut Random) -> Option<GuiCommand> {
    let live: Vec<GuiNodeId> = root.nodes().as_slice().iter().map(|node| node.id).collect();
    let pick = |random: &mut Random| live.get(random.below(live.len())).copied();
    Some(match random.below(6) {
        0 | 1 => GuiCommand::InsertNode {
            entity: entity(),
            root_incarnation: INCARNATION,
            id: GuiNodeId(root.next_node_id()),
            parent: pick(random),
            index: random.below(4) as u32,
            content: content(random),
            style: style(random),
        },
        2 => GuiCommand::UpdateNode {
            handle: handle(root, random)?,
            patch: patch(random),
        },
        3 => GuiCommand::MoveNode {
            handle: handle(root, random)?,
            parent: pick(random),
            index: random.below(4) as u32,
        },
        4 => GuiCommand::RemoveNode {
            handle: handle(root, random)?,
        },
        _ => {
            let handle = handle(root, random)?;
            let revision = root
                .control_state(handle.node_id)
                .map_or(0, |state| state.revision);
            GuiCommand::SetControlValue {
                handle,
                expected_revision: revision.saturating_sub(u32::from(random.chance(10))),
                value: match random.below(3) {
                    0 => GuiControlValue::Bool(random.chance(50)),
                    1 => GuiControlValue::Scalar(random.unit()),
                    _ => GuiControlValue::Text("typed".into()),
                },
            }
        }
    })
}

/// Skin authoring writes part lanes directly, outside GuiCommand.
fn author_part_lanes(root: &mut GuiRoot, random: &mut Random) {
    let nodes = root.nodes().as_slice();
    let Some(node) = nodes.get(random.below(nodes.len())).map(|node| node.id) else {
        return;
    };
    for (part, lane, value) in [
        (
            "background",
            "color",
            DynamicValue::Vec4([0.2, 0.3, 0.4, 1.0]),
        ),
        ("background_hovered", "opacity", DynamicValue::F32(0.5)),
        ("focusRing", "border_width", DynamicValue::F32(0.01)),
    ] {
        if random.chance(60) {
            let name = GuiRoot::part_property_name(node, part, lane).unwrap();
            root.properties.set(&name, value).unwrap();
        }
    }
}

#[test]
fn scoped_command_writes_equal_complete_root_diffs() {
    for seed in [0x9e37_79b9_u32, 0x2545_f491, 0x1b87_3593] {
        let mut random = Random(seed);
        let mut root = GuiRoot::default();
        let mut applied = 0;

        for _ in 0..1_500 {
            if random.chance(8) {
                author_part_lanes(&mut root, &mut random);
            }
            let Some(command) = command(&root, &mut random) else {
                continue;
            };

            let scoped = edit_commands(entity(), &root, INCARNATION, SESSION, &command);
            let full = full_edit(&root, &command);
            assert_eq!(
                scoped,
                full.as_ref()
                    .map(|(commands, _)| commands.clone())
                    .map_err(|error| *error),
                "{command:?}"
            );
            if let Ok((_, next)) = full {
                root = next;
                applied += 1;
            }
        }

        assert!(
            applied > 500,
            "seed {seed:#x} applied only {applied} commands"
        );
        assert!(root.node_count() > 5);
    }
}
