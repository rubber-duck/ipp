//! GuiCommand writes derived from the edited node's rows and part lanes
//! produce the same root as applying the edit to the complete root, over
//! randomized edit sequences.

use super::*;
use crate::DynamicValue;
use crate::systems::gui::GuiContainerKind;
use crate::systems::gui::test_support::{AuthoredNode, checkbox_node, slider_node};

const SESSION: u64 = 3;
const INCARNATION: u64 = 5;

fn entity() -> EntityId {
    EntityId::from_bits(9)
}

/// The complete-root path: apply the edit to a full copy and validate the
/// complete result, rows and part lanes included.
fn full_edit(root: &GuiRoot, command: &GuiCommand) -> Result<GuiRoot, ErrorReason> {
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

    let mut next = root.clone();
    match command {
        GuiCommand::InsertNode {
            id,
            parent,
            index,
            data,
            values,
            style,
            ..
        } => next.insert_node(
            *id,
            *parent,
            *index as usize,
            data.clone(),
            values.clone(),
            style,
        )?,
        GuiCommand::UpdateNode {
            handle,
            patch,
        } => next.update_node(handle.node_id, patch)?,
        GuiCommand::MoveNode {
            handle,
            parent,
            index,
        } => next
            .nodes_mut()
            .move_node(handle.node_id, *parent, *index as usize)
            .map_err(|_| ErrorReason::InvalidValue)?,
        GuiCommand::RemoveNode {
            handle,
        } => {
            next.remove_node(handle.node_id)?;
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
                handle,
                *expected_revision,
                value,
            )?;
        }
    }
    next.validate_complete()?;
    Ok(next)
}

/// Apply ordinary authored writes to a root the way the World applies them:
/// field writes through the registry (tree writes sync rows, row writes are
/// validated where they land) and named part lanes directly. Every write
/// must leave a complete valid root, as debug preparation checks after each
/// operation.
fn apply_writes(root: &GuiRoot, commands: &[Command]) -> Result<GuiRoot, ErrorReason> {
    let mut value = crate::ComponentValue::GuiRoot(root.clone());
    for command in commands {
        match command {
            Command::SetField {
                field,
                ..
            } => crate::components::registry::write(&mut value, field)?,
            Command::SetDynamicProperty {
                name,
                value: property,
                ..
            } => {
                value
                    .dynamic_properties_mut()
                    .expect("GUI roots carry part lanes")
                    .set(name, property.clone())
                    .map_err(|_| ErrorReason::InvalidValue)?;
            }
            Command::RemoveDynamicProperty {
                name,
                ..
            } => {
                value
                    .dynamic_properties_mut()
                    .expect("GUI roots carry part lanes")
                    .remove(name);
            }
            other => panic!("unexpected GUI write {other:?}"),
        }
        let crate::ComponentValue::GuiRoot(root) = &value else {
            unreachable!("GUI root writes keep the component type")
        };
        assert_eq!(root.validate_complete(), Ok(()), "after {command:?}");
    }
    let crate::ComponentValue::GuiRoot(root) = value else {
        unreachable!("GUI root writes keep the component type")
    };
    root.validate_complete()?;
    Ok(root)
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

fn node(random: &mut Random) -> AuthoredNode {
    match random.below(7) {
        0 => GuiNodeData::Container(GuiContainerKind::Column).into(),
        1 => GuiNodeData::Container(GuiContainerKind::Row).into(),
        2 => GuiNodeData::Text(format!("label {}", random.below(100))).into(),
        3 => checkbox_node(random.chance(50)),
        // Occasionally a range that excludes a committed value.
        4 => slider_node(random.unit(), random.unit() * 0.5, 1.0, 0.0),
        5 => GuiNodeData::Button {
            label: "go".into(),
        }
        .into(),
        _ => GuiNodeData::TextInput {
            text: "text".into(),
            placeholder: String::new(),
        }
        .into(),
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
    let node = random.chance(20).then(|| node(random));
    let values_only = node.is_none() && random.chance(10);
    GuiNodePatch {
        values: match &node {
            Some(node) => random.chance(80).then(|| node.values.clone()),
            None if values_only => Some(slider_node(random.unit(), 0.0, 1.0, 0.1).values),
            None => None,
        },
        data: node.map(|node| node.data),
        position: random.chance(20).then(|| [random.unit(), 0.0]),
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
    // Occasionally stale: a removed or never-allocated node fails the fence.
    let id = if random.chance(5) {
        GuiNodeId(root.next_node_id())
    } else {
        node.id
    };
    Some(GuiNodeHandle::new(SESSION, entity(), INCARNATION, id))
}

fn command(root: &GuiRoot, random: &mut Random) -> Option<GuiCommand> {
    let live: Vec<GuiNodeId> = root.nodes().as_slice().iter().map(|node| node.id).collect();
    let pick = |random: &mut Random| live.get(random.below(live.len())).copied();
    Some(match random.below(6) {
        0 | 1 => {
            let node = node(random);
            GuiCommand::InsertNode {
                entity: entity(),
                root_incarnation: INCARNATION,
                id: GuiNodeId(root.next_node_id()),
                parent: pick(random),
                index: random.below(4) as u32,
                data: node.data,
                values: node.values,
                style: style(random),
            }
        }
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
fn scoped_command_writes_reach_the_complete_root_result() {
    for seed in [0x9e37_79b9_u32, 0x2545_f491, 0x1b87_3593] {
        let mut random = Random(seed);
        let mut root = GuiRoot::default();
        let mut applied = 0;
        let mut peak = 0;

        for _ in 0..1_500 {
            if random.chance(8) {
                author_part_lanes(&mut root, &mut random);
            }
            let Some(command) = command(&root, &mut random) else {
                continue;
            };

            let scoped = edit_commands(entity(), &root, INCARNATION, SESSION, &command)
                .and_then(|writes| apply_writes(&root, &writes));
            let full = full_edit(&root, &command);
            assert_eq!(scoped, full, "{command:?}");
            if let Ok(next) = full {
                root = next;
                applied += 1;
                peak = peak.max(root.node_count());
            }
        }

        assert!(
            applied > 500,
            "seed {seed:#x} applied only {applied} commands"
        );
        assert!(peak > 5, "seed {seed:#x} reached only {peak} nodes");
    }
}
