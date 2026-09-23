use super::*;
use crate::components::schema::FieldError;

fn test_node(id: u32, parent: Option<u32>, children: Vec<u32>) -> GuiNode {
    GuiNode {
        id: GuiNodeId(id),
        parent: parent.map(GuiNodeId),
        children: children.into_iter().map(GuiNodeId).collect(),
        data: GuiNodeData::Container(GuiContainerKind::Column),
    }
}

#[test]
fn validate_empty_tree_succeeds() {
    let nodes = GuiNodes::default();
    assert_eq!(nodes.validate(), Ok(()));
}

#[test]
fn validate_single_root_succeeds() {
    let nodes = GuiNodes {
        values: vec![test_node(1, None, vec![])],
        next_id: 2,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Ok(()));
}

#[test]
fn validate_rejects_disconnected_roots() {
    let nodes = GuiNodes {
        values: vec![test_node(1, None, vec![]), test_node(2, None, vec![])],
        next_id: 3,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Err(FieldError::WrongType));
}

#[test]
fn validate_rejects_duplicate_children() {
    let nodes = GuiNodes {
        values: vec![
            test_node(1, None, vec![2, 2]),
            test_node(2, Some(1), vec![]),
        ],
        next_id: 3,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Err(FieldError::WrongType));
}

#[test]
fn validate_rejects_parent_child_mismatch() {
    // 1 claims 2 as child, but 2 claims parent is None
    let nodes = GuiNodes {
        values: vec![test_node(1, None, vec![2]), test_node(2, None, vec![])],
        next_id: 3,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Err(FieldError::WrongType));

    // 1 claims 2 as child, but 2 claims parent is 3
    let nodes2 = GuiNodes {
        values: vec![
            test_node(1, None, vec![2]),
            test_node(2, Some(3), vec![]),
            test_node(3, Some(1), vec![]),
        ],
        next_id: 4,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes2.validate(), Err(FieldError::WrongType));
}

#[test]
fn validate_rejects_cycle() {
    // 1 and 2 claim each other as parent and child
    let nodes = GuiNodes {
        values: vec![
            test_node(1, Some(2), vec![2]),
            test_node(2, Some(1), vec![1]),
        ],
        next_id: 3,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Err(FieldError::WrongType));
}

#[test]
fn validate_rejects_unreachable_cycle() {
    // 1 is root with no children. 2 and 3 form an unreachable reciprocal cycle.
    let nodes = GuiNodes {
        values: vec![
            test_node(1, None, vec![]),
            test_node(2, Some(3), vec![3]),
            test_node(3, Some(2), vec![2]),
        ],
        next_id: 4,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(nodes.validate(), Err(FieldError::WrongType));
}

#[test]
fn move_node_rejects_promoting_child_when_root_exists() {
    let mut nodes = GuiNodes {
        values: vec![test_node(1, None, vec![2]), test_node(2, Some(1), vec![])],
        next_id: 3,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(
        nodes.move_node(GuiNodeId(2), None, 0),
        Err(FieldError::WrongType)
    );
}

#[test]
fn move_node_rejects_moving_ancestor_under_descendant() {
    let mut nodes = GuiNodes {
        values: vec![
            test_node(1, None, vec![2]),
            test_node(2, Some(1), vec![3]),
            test_node(3, Some(2), vec![]),
        ],
        next_id: 4,
        root_node: Some(GuiNodeId(1)),
        controls: Default::default(),
    };
    assert_eq!(
        nodes.move_node(GuiNodeId(1), Some(GuiNodeId(3)), 0),
        Err(FieldError::WrongType)
    );
}

#[test]
fn text_limit_accepts_boundary_and_rejects_authored_or_decoded_oversize() {
    let boundary = "a".repeat(MAX_GUI_TEXT_BYTES);
    assert_eq!(validate_node_data(&GuiNodeData::Text(boundary)), Ok(()));
    assert_eq!(
        validate_node_data(&GuiNodeData::Button {
            label: "a".repeat(MAX_GUI_TEXT_BYTES + 1),
        }),
        Err(FieldError::WrongType)
    );

    let mut encoded = vec![2];
    encoded.extend_from_slice(&((MAX_GUI_TEXT_BYTES + 1) as u32).to_le_bytes());
    let mut input = encoded.as_slice();
    assert_eq!(decode_node_data(&mut input), Err(FieldError::WrongType));
}

#[test]
fn tree_codec_round_trips_kinds_and_control_records() {
    let mut nodes = GuiNodes::default();
    nodes
        .insert_node(
            GuiNodeId(1),
            None,
            0,
            GuiNodeData::Container(GuiContainerKind::Row),
        )
        .unwrap();
    for data in [
        GuiNodeData::Image,
        GuiNodeData::Checkbox,
        GuiNodeData::Slider,
        GuiNodeData::TextInput {
            text: "seed".into(),
            placeholder: "name".into(),
        },
    ] {
        let id = GuiNodeId(nodes.next_node_id());
        nodes
            .insert_node(id, Some(GuiNodeId(1)), usize::MAX, data.clone())
            .unwrap();
        nodes.controls.insert_initial(id, &data);
    }
    let decoded = GuiNodes::decode(&nodes.encode()).unwrap();
    assert_eq!(decoded, nodes);
    assert_eq!(
        decoded
            .controls()
            .get(GuiNodeId(5))
            .and_then(|entry| entry.text.as_deref()),
        Some("seed")
    );
    assert_eq!(
        decoded
            .controls()
            .get(GuiNodeId(3))
            .map(|entry| entry.text.clone()),
        Some(None)
    );
    assert!(decoded.controls().get(GuiNodeId(2)).is_none());
}
