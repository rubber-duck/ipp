//! VirtualList position, anchor and range math.

use super::*;

fn list(count: u32, extent: f32, overscan: u32, viewport: f32) -> GuiVirtualListLayout {
    GuiVirtualListLayout::new(
        &GuiNodeDataRow::virtual_list(count, extent, overscan, 1),
        [4.0, viewport],
    )
}

/// Declare children at `indices` with main extents `extents`, as layout
/// does in index order.
fn declare(list: &mut GuiVirtualListLayout, children: &[(u32, f32)]) {
    for (slot, &(index, extent)) in children.iter().enumerate() {
        let index = list.accepts(index).expect("index accepted");
        list.push(index, GuiNodeId(slot as u32 + 10), [4.0, extent], true);
    }
}

#[test]
fn undeclared_items_take_the_estimate() {
    let list = list(100, 2.0, 1, 5.0);

    assert_eq!(list.content_extent(), 200.0);
    assert_eq!(list.content_size([4.0, 5.0]), [4.0, 200.0]);
    assert_eq!(list.capacity(), 195.0);
    assert_eq!(list.position(7), 14.0);
    assert_eq!(list.item_at(0.0), 0);
    assert_eq!(list.item_at(13.9), 6);
    assert_eq!(list.item_at(14.0), 7);
    assert_eq!(list.item_at(1.0e9), 99);
    assert_eq!(list.anchor(15.5), (7, 1.5));
    assert_eq!(list.anchored_offset(7, 1.5), 15.5);

    // Offset 3 shows items 1..=3 (3.0..8.0); one item of overscan each side.
    assert_eq!(list.wanted_range(3.0), (0, 5));
    assert_eq!(list.wanted_range(4.0), (1, 6));
    assert_eq!(list.wanted_range(195.0), (96, 100));
    assert_eq!(list.loaded_range(), (0, 0));
}

#[test]
fn declared_children_replace_the_estimate_and_move_later_items() {
    let mut list = list(100, 2.0, 0, 5.0);
    declare(&mut list, &[(3, 5.0), (4, 1.0), (10, 2.5)]);

    // Item 3 starts at its estimate position; later items shift by the
    // measured deltas before them: +3.0 after item 3, -1.0 after item 4.
    assert_eq!(list.position(3), 6.0);
    assert_eq!(list.position(4), 11.0);
    assert_eq!(list.position(5), 12.0);
    assert_eq!(list.position(10), 22.0);
    assert_eq!(list.position(11), 24.5);
    assert_eq!(list.content_extent(), 200.0 + 3.0 - 1.0 + 0.5);
    assert_eq!(list.loaded_range(), (3, 11));

    assert_eq!(list.item_at(6.0), 3);
    assert_eq!(list.item_at(10.9), 3);
    assert_eq!(list.item_at(11.5), 4);
    assert_eq!(list.item_at(12.0), 5);
    assert_eq!(list.item_at(21.9), 9);
    assert_eq!(list.item_at(24.0), 10);
    assert_eq!(list.item_at(24.5), 11);

    for offset in [0.0, 5.9, 6.0, 8.25, 11.0, 11.75, 12.0, 23.0, 30.0, 150.0] {
        let (index, within) = list.anchor(offset);
        assert!(within >= 0.0);
        assert!((list.anchored_offset(index, within) - offset).abs() < 1.0e-4);
    }

    // Visible 7.0..12.0 covers items 3 and 4 and ends exactly where 5
    // starts, which is therefore not wanted.
    assert_eq!(list.wanted_range(7.0), (3, 5));
}

#[test]
fn children_past_the_count_or_repeating_an_index_are_not_placed() {
    let mut list = list(5, 1.0, 0, 2.0);
    assert_eq!(list.accepts(2), Some(2));
    list.push(2, GuiNodeId(1), [4.0, 3.0], true);
    assert_eq!(list.accepts(2), None);
    assert_eq!(list.accepts(1), None);
    assert_eq!(list.accepts(5), None);
    assert_eq!(list.accepts(4), Some(4));

    // A child that could not measure keeps the estimate.
    list.push(4, GuiNodeId(2), [0.0, 0.0], false);
    assert_eq!(list.items[1].extent, 1.0);
    assert_eq!(list.content_extent(), 7.0);
}

#[test]
fn an_empty_list_wants_nothing() {
    let list = list(0, 2.0, 3, 5.0);
    assert_eq!(list.content_extent(), 0.0);
    assert_eq!(list.capacity(), 0.0);
    assert_eq!(list.item_at(10.0), 0);
    assert_eq!(list.wanted_range(0.0), (0, 0));
    assert_eq!(list.anchored_offset(4, 1.0), 1.0);
}

#[test]
fn a_large_list_stays_bounded_by_its_declared_window() {
    let mut list = list(100_000, 1.5, 2, 12.0);
    let window: Vec<(u32, f32)> = (50_000..50_012)
        .map(|index| {
            (
                index,
                if index % 2 == 0 {
                    1.0
                } else {
                    3.0
                },
            )
        })
        .collect();
    declare(&mut list, &window);

    assert_eq!(list.items.len(), 12);
    let start = list.position(50_000);
    assert_eq!(start, 75_000.0);

    // Twelve items of alternating 1.0 and 3.0 add 6 * (1.0 - 1.5) + 6 *
    // (3.0 - 1.5) = +6.0 to everything after them.
    assert_eq!(list.content_extent(), 150_000.0 + 6.0);
    assert_eq!(list.position(50_012), 75_000.0 + 12.0 * 1.5 + 6.0);
    assert_eq!(list.position(99_999), 99_999.0 * 1.5 + 6.0);

    // The viewport shows items 50000..=50005 (0.0..12.0 of the window).
    assert_eq!(list.wanted_range(start), (49_998, 50_008));
    assert_eq!(list.item_at(list.content_extent()), 99_999);
}

#[test]
fn horizontal_lists_place_along_x() {
    let list = GuiVirtualListLayout::new(&GuiNodeDataRow::virtual_list(10, 3.0, 0, 0), [9.0, 4.0]);
    assert_eq!(list.axis, 0);
    assert_eq!(list.item_extent, 3.0);
    assert_eq!(list.viewport, 9.0);
    assert_eq!(list.local_point(12.0), [12.0, 0.0]);
    assert_eq!(list.content_size([9.0, 4.0]), [30.0, 4.0]);
}
