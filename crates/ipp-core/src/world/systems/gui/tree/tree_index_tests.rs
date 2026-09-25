use super::*;
use crate::systems::gui::{GuiContainerKind, GuiNodeData, GuiNodeStyle};

fn column() -> GuiNodeData {
    GuiNodeData::Container(GuiContainerKind::Column)
}

/// Root with node 1 as the root and `count` further nodes appended under
/// `parents[i]`.
fn root_with(parents: &[u32]) -> GuiRoot {
    let mut root = GuiRoot::default();
    root.insert_node(
        GuiNodeId(1),
        None,
        0,
        column(),
        Default::default(),
        &GuiNodeStyle::default(),
    )
    .unwrap();
    for (offset, &parent) in parents.iter().enumerate() {
        root.insert_node(
            GuiNodeId(offset as u32 + 2),
            Some(GuiNodeId(parent)),
            usize::MAX,
            column(),
            Default::default(),
            &GuiNodeStyle::default(),
        )
        .unwrap();
    }
    root
}

/// Write one node's sibling order key as a field write does.
fn set_order(root: &mut GuiRoot, id: u32, order: u32) {
    use crate::components::schema::{FieldValue, SchemaComponent};

    let offset =
        GuiRoot::node_tree_offset(GuiNodeId(id), super::super::GuiNodeTreeProperty::Order).unwrap();
    root.set_field(offset, FieldValue::Dynamic(crate::DynamicValue::U32(order)))
        .unwrap();
}

fn ids(values: &[u32]) -> Vec<GuiNodeId> {
    values.iter().copied().map(GuiNodeId).collect()
}

#[test]
fn derives_children_by_order_then_identity() {
    let mut root = root_with(&[1, 1, 1, 2]);
    // Equal keys order by identity; node 4 sorts first by its key.
    for id in [2, 3] {
        set_order(&mut root, id, 7);
    }
    set_order(&mut root, 4, 3);
    let index = GuiTreeIndex::new(&root, 9);
    assert_eq!(index.incarnation(), 9);
    assert_eq!(index.root(), Some(GuiNodeId(1)));
    assert_eq!(index.children(GuiNodeId(1)), ids(&[4, 2, 3]));
    assert_eq!(index.children(GuiNodeId(2)), ids(&[5]));
    assert!(index.children(GuiNodeId(5)).is_empty());
    assert_eq!(index.subtree(GuiNodeId(1)), ids(&[1, 4, 2, 5, 3]));
    assert_eq!(index.len(), 5);
}

#[test]
fn updates_match_a_rebuild_after_moves_insertions_and_removals() {
    let mut root = root_with(&[1, 1, 2, 2, 3]);
    let mut index = GuiTreeIndex::new(&root, 1);

    root.move_node(GuiNodeId(5), Some(GuiNodeId(1)), 0).unwrap();
    index.update(&root, GuiNodeId(5));
    assert_eq!(index, GuiTreeIndex::new(&root, 1));
    assert_eq!(index.children(GuiNodeId(1)), ids(&[5, 2, 3]));

    root.insert_node(
        GuiNodeId(7),
        Some(GuiNodeId(3)),
        0,
        column(),
        Default::default(),
        &GuiNodeStyle::default(),
    )
    .unwrap();
    index.update(&root, GuiNodeId(7));
    assert_eq!(index, GuiTreeIndex::new(&root, 1));

    let removed = root.remove_node(GuiNodeId(2)).unwrap();
    assert_eq!(removed, ids(&[2, 4]));
    index.update(&root, GuiNodeId(2));
    assert_eq!(index, GuiTreeIndex::new(&root, 1));
    assert!(!index.contains(GuiNodeId(4)));

    root.remove_node(GuiNodeId(1)).unwrap();
    index.update(&root, GuiNodeId(1));
    assert_eq!(index, GuiTreeIndex::new(&root, 1));
    assert!(index.is_empty());
    assert_eq!(index.root(), None);
}

#[test]
fn sibling_renumbering_keeps_the_index_ordered() {
    let mut root = root_with(&[1, 1]);
    let mut index = GuiTreeIndex::new(&root, 1);
    // Squeeze a gap shut, then insert into it.
    set_order(&mut root, 2, 10);
    set_order(&mut root, 3, 11);
    index.update(&root, GuiNodeId(2));
    index.update(&root, GuiNodeId(3));
    root.insert_node(
        GuiNodeId(4),
        Some(GuiNodeId(1)),
        1,
        column(),
        Default::default(),
        &GuiNodeStyle::default(),
    )
    .unwrap();
    for id in [2, 3, 4] {
        index.update(&root, GuiNodeId(id));
    }
    assert_eq!(index, GuiTreeIndex::new(&root, 1));
    assert_eq!(index.children(GuiNodeId(1)), ids(&[2, 4, 3]));
}
