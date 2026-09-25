use super::*;
use crate::components::rows::row_region_relative;
use crate::systems::gui::{GuiContainerKind, GuiRoot};

fn column(parent: u32, order: u32) -> GuiNodeTreeRow {
    GuiNodeTreeRow::authored(
        (parent != 0).then_some(GuiNodeId(parent)),
        order,
        &GuiNodeData::<String>::Container(GuiContainerKind::Column),
    )
}

fn tree(rows: &[(u32, GuiNodeTreeRow)]) -> GuiNodeTree {
    let mut tree = GuiNodeTree::default();
    for (slot, row) in rows {
        tree.rows_mut().insert(*slot, row.clone()).unwrap();
    }
    tree
}

/// Region-relative address of one property of one node.
fn relative(node: u32, property: GuiNodeTreeProperty) -> u32 {
    let offset = GuiRoot::node_tree_offset(GuiNodeId(node), property).unwrap();
    row_region_relative(offset, GuiRoot::NODE_TREE_FIELD).unwrap()
}

#[test]
fn whole_check_accepts_an_empty_tree_and_a_single_root() {
    assert_eq!(GuiNodeTree::default().validate(), Ok(()));
    assert_eq!(tree(&[(1, column(0, 0))]).validate(), Ok(()));
    assert_eq!(
        tree(&[(1, column(0, 0)), (2, column(1, 5)), (3, column(2, 0))]).validate(),
        Ok(())
    );
}

#[test]
fn whole_check_rejects_broken_structure() {
    // Two roots.
    assert!(
        tree(&[(1, column(0, 0)), (2, column(0, 0))])
            .validate()
            .is_err()
    );
    // A missing parent.
    assert!(
        tree(&[(1, column(0, 0)), (2, column(9, 0))])
            .validate()
            .is_err()
    );
    // Self-parenting.
    assert!(
        tree(&[(1, column(0, 0)), (2, column(2, 0))])
            .validate()
            .is_err()
    );
    // A cycle unreachable from the root.
    assert!(
        tree(&[(1, column(0, 0)), (2, column(3, 0)), (3, column(2, 0))])
            .validate()
            .is_err()
    );
    // No root at all.
    assert!(
        tree(&[(2, column(3, 0)), (3, column(2, 0))])
            .validate()
            .is_err()
    );
    // Node id zero.
    assert!(tree(&[(0, column(0, 0))]).validate().is_err());
}

#[test]
fn row_rules_follow_the_kind() {
    let button = GuiNodeTreeRow::authored(
        Some(GuiNodeId(1)),
        0,
        &GuiNodeData::Button {
            label: "Go".to_owned(),
        },
    );
    assert_eq!(button.validate(), Ok(GuiNodeKind::Button));
    assert_eq!(button.revision, 0);

    let input = GuiNodeTreeRow::authored(
        Some(GuiNodeId(1)),
        0,
        &GuiNodeData::TextInput {
            text: "abc".to_owned(),
            placeholder: "type".to_owned(),
        },
    );
    assert_eq!(input.validate(), Ok(GuiNodeKind::TextInput));
    assert_eq!(input.committed_text.as_deref(), Some("abc"));
    assert_eq!(input.revision, 1);

    let mut missing_label = button.clone();
    missing_label.text = None;
    assert_eq!(missing_label.validate(), Err(ErrorReason::InvalidField));

    let mut stray_text = column(1, 0);
    stray_text.text = Some("x".into());
    assert_eq!(stray_text.validate(), Err(ErrorReason::InvalidField));

    let mut unrevised = input.clone();
    unrevised.revision = 0;
    assert_eq!(unrevised.validate(), Err(ErrorReason::InvalidField));

    let mut unknown = column(1, 0);
    unknown.kind = GuiNodeKind::COUNT;
    assert_eq!(unknown.validate(), Err(ErrorReason::InvalidValue));

    let mut long = input;
    long.committed_text = Some("a".repeat(MAX_TEXT_BYTES + 1));
    assert_eq!(long.validate(), Err(ErrorReason::InvalidValue));
}

#[test]
fn conform_keeps_strings_and_revisions_the_kind_still_uses() {
    let mut row = GuiNodeTreeRow::authored(
        Some(GuiNodeId(1)),
        0,
        &GuiNodeData::TextInput {
            text: "abc".to_owned(),
            placeholder: "p".to_owned(),
        },
    );
    row.revision = 4;
    row.conform(GuiNodeKind::Button);
    assert_eq!(row.text.as_deref(), Some("abc"));
    assert_eq!(
        (row.placeholder.as_ref(), row.committed_text.as_ref()),
        (None, None)
    );
    assert_eq!(row.revision, 4);

    row.conform(GuiNodeKind::Stack);
    assert_eq!(row.text, None);
    assert_eq!(row.validate(), Ok(GuiNodeKind::Stack));

    let mut fresh = column(1, 0);
    fresh.conform(GuiNodeKind::Checkbox);
    assert_eq!(fresh.revision, 1);
    assert_eq!(fresh.validate(), Ok(GuiNodeKind::Checkbox));
}

#[test]
fn parent_writes_allocate_and_retire_rows() {
    let mut tree = tree(&[(1, column(0, 0))]);
    let parent = relative(2, GuiNodeTreeProperty::Parent);

    // An absent node reads its parent as unset; writing it allocates.
    assert_eq!(tree.row_field(parent), Ok(FieldValue::Unset));
    tree.set_row_field(parent, FieldValue::Dynamic(DynamicValue::U32(1)))
        .unwrap();
    assert_eq!(
        tree.get(GuiNodeId(2)),
        Some(&GuiNodeTreeRow {
            parent: 1,
            ..GuiNodeTreeRow::default()
        })
    );
    assert_eq!(tree.validate(), Ok(()));

    // Other properties of an absent node stay unaddressable.
    assert_eq!(
        tree.set_row_field(
            relative(3, GuiNodeTreeProperty::Order),
            FieldValue::Dynamic(DynamicValue::U32(1))
        ),
        Err(FieldError::UnknownField)
    );

    // Clearing the parent retires the row; the slot never returns.
    tree.set_row_field(parent, FieldValue::Unset).unwrap();
    assert!(!tree.is_live(GuiNodeId(2)));
    assert_eq!(
        tree.set_row_field(parent, FieldValue::Dynamic(DynamicValue::U32(1))),
        Err(FieldError::UnknownField)
    );
    assert_eq!(
        tree.set_row_field(parent, FieldValue::Unset),
        Err(FieldError::UnknownField)
    );

    // Node id zero is never allocated.
    assert!(
        tree.set_row_field(
            relative(0, GuiNodeTreeProperty::Parent),
            FieldValue::Dynamic(DynamicValue::U32(1))
        )
        .is_err()
    );
    assert_eq!(
        GuiNodeTree::validate_row_field(parent, FieldKind::Unset),
        Ok(())
    );
    assert!(
        GuiNodeTree::validate_row_field(relative(2, GuiNodeTreeProperty::Order), FieldKind::Unset)
            .is_err()
    );
}

#[test]
fn text_writes_respect_the_bound() {
    let mut tree = tree(&[(
        1,
        GuiNodeTreeRow::authored(None, 0, &GuiNodeData::Text(String::new())),
    )]);
    let text = relative(1, GuiNodeTreeProperty::Text);
    tree.set_row_field(text, FieldValue::String("a".repeat(MAX_TEXT_BYTES)))
        .unwrap();
    assert_eq!(
        tree.set_row_field(text, FieldValue::String("a".repeat(MAX_TEXT_BYTES + 1))),
        Err(FieldError::TextTooLong)
    );
}

#[test]
fn placement_walk_rejects_cycles_missing_parents_and_excess_depth() {
    let mut rows = vec![(1, column(0, 0))];
    for id in 2..=MAX_LAYOUT_DEPTH as u32 + 1 {
        rows.push((id, column(id - 1, 0)));
    }
    let chain = tree(&rows);
    let deepest = MAX_LAYOUT_DEPTH as u32 + 1;
    assert_eq!(chain.validate_parent(GuiNodeId(deepest)), Ok(()));
    // One level deeper than layout evaluates.
    assert!(
        chain
            .validate_placement(GuiNodeId(deepest + 1), deepest)
            .is_err()
    );
    assert_eq!(
        chain.validate_placement(GuiNodeId(deepest + 1), deepest - 1),
        Ok(())
    );
    // Under its own descendant, under itself, under a missing node.
    assert!(chain.validate_placement(GuiNodeId(2), 5).is_err());
    assert!(chain.validate_placement(GuiNodeId(2), 2).is_err());
    assert!(chain.validate_placement(GuiNodeId(2), 999).is_err());

    // The root may only stay the single root.
    assert_eq!(chain.validate_parent(GuiNodeId(1)), Ok(()));
    let two_roots = tree(&[(1, column(0, 0)), (2, column(0, 0))]);
    assert!(two_roots.validate_parent(GuiNodeId(2)).is_err());
    assert_eq!(
        tree(&[(4, column(0, 0))]).validate_parent(GuiNodeId(4)),
        Ok(())
    );
}

#[test]
fn descendants_follow_parent_links_in_any_identity_order() {
    let tree = tree(&[
        (1, column(0, 0)),
        (2, column(5, 0)),
        (3, column(1, 0)),
        (5, column(3, 0)),
        (6, column(1, 0)),
    ]);
    let mut below = tree.descendants(GuiNodeId(3));
    below.sort_unstable();
    assert_eq!(below, vec![2, 5]);
    assert!(tree.descendants(GuiNodeId(6)).is_empty());
}

#[test]
fn place_among_uses_gaps_before_renumbering_one_parent() {
    let id = GuiNodeId;
    assert_eq!(place_among(&[], 0), (ORDER_SPACING, Vec::new()));

    let siblings = [(id(2), 10), (id(3), 20)];
    assert_eq!(place_among(&siblings, 0), (5, Vec::new()));
    assert_eq!(place_among(&siblings, 1), (15, Vec::new()));
    assert_eq!(place_among(&siblings, 2), (20 + ORDER_SPACING, Vec::new()));

    // No gap between neighbours: every sibling is renumbered evenly.
    let tight = [(id(2), 10), (id(3), 11), (id(4), 12)];
    let (key, renumbered) = place_among(&tight, 1);
    assert_eq!(key, 2 * ORDER_SPACING);
    assert_eq!(
        renumbered,
        vec![
            (id(2), ORDER_SPACING),
            (id(3), 3 * ORDER_SPACING),
            (id(4), 4 * ORDER_SPACING)
        ]
    );

    // Keys at the edges of the space renumber too.
    assert_eq!(place_among(&[(id(2), 0)], 0).1.len(), 1);
    let (last, renumbered) = place_among(&[(id(2), u32::MAX)], 1);
    assert!(renumbered.iter().all(|&(_, order)| order < last));

    // Renumbering a full parent stays within the key space.
    let full: Vec<(GuiNodeId, u32)> = (0..MAX_NODES as u32 - 1).map(|n| (id(n + 2), n)).collect();
    let (key, renumbered) = place_among(&full, 7);
    let mut keys: Vec<u32> = renumbered.iter().map(|&(_, order)| order).collect();
    keys.insert(7, key);
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
}
