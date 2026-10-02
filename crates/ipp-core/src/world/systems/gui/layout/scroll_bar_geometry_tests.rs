//! Ordinary ScrollView bar geometry: default and authored thickness and
//! insets, thumb ratio and travel between the pointed ends, the shared corner,
//! the minimum thumb, bars of scrolling axes whose content fits, and enclosing
//! tracks mapped into the bar's frame.

use super::*;

/// A control scrolling the axes its `content` overflows.
fn view(viewport: [f32; 2], content: [f32; 2]) -> GuiScrollExtent {
    scrolling(
        std::array::from_fn(|axis| content[axis] > viewport[axis]),
        viewport,
        content,
    )
}

fn scrolling(axes: [bool; 2], viewport: [f32; 2], content: [f32; 2]) -> GuiScrollExtent {
    GuiScrollExtent {
        axes,
        viewport,
        content,
        capacity: std::array::from_fn(|axis| (content[axis] - viewport[axis]).max(0.0)),
    }
}

/// Flush bars of `thickness`, with no inset from the control's sides or ends.
fn flush(thickness: f32) -> GuiScrollBarStyle {
    GuiScrollBarStyle {
        thickness,
        inset: 0.0,
        end_inset: 0.0,
    }
}

/// Bars of a control as large as its viewport.
fn bars(
    style: GuiScrollBarStyle,
    viewport: [f32; 2],
    content: [f32; 2],
    offset: [f32; 2],
    obstacles: &[GuiScrollBar],
) -> Vec<GuiScrollBar> {
    ordinary_scroll_bars(&view(viewport, content), viewport, style, offset, obstacles)
}

fn assert_rect(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "{actual:?} != {expected:?}"
        );
    }
}

/// An enclosing track already mapped into the inner ScrollView's local frame.
fn obstacle(axis: usize, track: [f32; 4]) -> GuiScrollBar {
    GuiScrollBar {
        axis,
        track,
        thumb: track,
        rail: [track[axis], track[axis + 2]],
        capacity: 1.0,
        page: 1.0,
    }
}

#[test]
fn vertical_thumb_follows_viewport_ratio_and_offset_between_the_pointed_ends() {
    // A 10-unit track whose ends point over 5 units leaves 90 of its 100 for
    // the thumb, which shows 60% of them.
    let vertical = bars(
        flush(10.0),
        [200.0, 100.0],
        [200.0, 1000.0 / 6.0],
        [0.0; 2],
        &[],
    );
    assert_eq!(vertical.len(), 1);
    let vertical = vertical[0];
    assert_eq!(vertical.axis, 1);
    assert_rect(vertical.track, [190.0, 0.0, 10.0, 100.0]);
    assert_eq!(vertical.rail, [5.0, 90.0]);
    assert_rect(vertical.thumb, [190.0, 5.0, 10.0, 54.0]);
    assert!(vertical.enabled());

    // The thumb travels the remaining 36 units in proportion to the offset.
    let capacity = vertical.capacity;
    let middle = bars(
        flush(10.0),
        [200.0, 100.0],
        [200.0, 1000.0 / 6.0],
        [0.0, capacity / 2.0],
        &[],
    )[0];
    assert_rect(middle.thumb, [190.0, 23.0, 10.0, 54.0]);
    let end = bars(
        flush(10.0),
        [200.0, 100.0],
        [200.0, 1000.0 / 6.0],
        [0.0, capacity],
        &[],
    )[0];
    assert_rect(end.thumb, [190.0, 41.0, 10.0, 54.0]);

    // Dragging the thumb start maps back to offsets, clamped to the rail.
    assert!((middle.travel() - 36.0).abs() < 1e-4);
    assert!((middle.offset_for_thumb(23.0) - capacity / 2.0).abs() < 1e-3);
    assert_eq!(middle.offset_for_thumb(-5.0), 0.0);
    assert_eq!(middle.offset_for_thumb(500.0), capacity);
}

#[test]
fn default_fields_follow_one_bar_thickness_of_the_font() {
    // Half a 16-unit font thick, one thickness in from the far side and half
    // of one from the ends: the design language's bar 8 at inset 8.
    assert_eq!(
        GuiScrollBarStyle::from_fields([-1.0; 3], 16.0),
        GuiScrollBarStyle {
            thickness: 8.0,
            inset: 8.0,
            end_inset: 4.0,
        }
    );

    // Default insets follow an authored thickness; authored insets stay.
    assert_eq!(
        GuiScrollBarStyle::from_fields([4.0, -1.0, -1.0], 16.0),
        GuiScrollBarStyle {
            thickness: 4.0,
            inset: 4.0,
            end_inset: 2.0,
        }
    );
    assert_eq!(
        GuiScrollBarStyle::from_fields([-1.0, 0.0, 1.0], 16.0),
        GuiScrollBarStyle {
            thickness: 8.0,
            inset: 0.0,
            end_inset: 1.0,
        }
    );
}

#[test]
fn authored_insets_place_the_bar_inside_the_control_box() {
    // The sheet's list frame: a 226 x 82 control whose bar is 10 wide, 9.5 in
    // from the right side and 5 in from both ends, over three of eight rows.
    let style = GuiScrollBarStyle {
        thickness: 10.0,
        inset: 9.5,
        end_inset: 5.0,
    };
    let rows = view([226.0, 78.0], [198.0, 208.0]);
    let bar = ordinary_scroll_bars(&rows, [226.0, 82.0], style, [0.0; 2], &[]);
    assert_eq!(bar.len(), 1);
    assert_rect(bar[0].track, [206.5, 5.0, 10.0, 72.0]);
    assert_eq!(bar[0].rail, [10.0, 62.0]);
    assert_rect(bar[0].thumb, [206.5, 10.0, 10.0, 62.0 * 78.0 / 208.0]);
    let end = ordinary_scroll_bars(&rows, [226.0, 82.0], style, [0.0, 130.0], &[]);
    assert_rect(
        end[0].thumb,
        [206.5, 72.0 - 62.0 * 78.0 / 208.0, 10.0, 62.0 * 78.0 / 208.0],
    );
}

#[test]
fn both_axes_share_the_corner_and_thumbs_keep_a_minimum_length() {
    let both = bars(
        flush(10.0),
        [200.0, 120.0],
        [400.0, 20_000.0],
        [0.0; 2],
        &[],
    );
    let [horizontal, vertical] = [both[0], both[1]];
    assert_eq!((horizontal.axis, vertical.axis), (0, 1));
    assert_rect(horizontal.track, [0.0, 110.0, 190.0, 10.0]);
    assert_rect(vertical.track, [190.0, 0.0, 10.0, 110.0]);
    // Half the width is visible; the tall content hits the two-thickness minimum.
    assert_rect(horizontal.thumb, [5.0, 110.0, 90.0, 10.0]);
    assert_rect(vertical.thumb, [190.0, 5.0, 10.0, 20.0]);

    // With insets, each track stops where the crossing one starts.
    let inset = GuiScrollBarStyle {
        thickness: 10.0,
        inset: 4.0,
        end_inset: 2.0,
    };
    let both = bars(inset, [200.0, 120.0], [400.0, 20_000.0], [0.0; 2], &[]);
    assert_rect(both[0].track, [2.0, 106.0, 184.0, 10.0]);
    assert_rect(both[1].track, [186.0, 2.0, 10.0, 104.0]);
}

#[test]
fn a_scrolling_axis_keeps_its_bar_while_content_fits_and_that_bar_takes_no_input() {
    let style = flush(10.0);

    // Content that fits along an axis the control does not scroll shows nothing.
    let fixed = scrolling([false; 2], [100.0, 60.0], [100.0, 50.0]);
    assert!(ordinary_scroll_bars(&fixed, [100.0, 60.0], style, [0.0; 2], &[]).is_empty());

    // A vertical control keeps its vertical bar; the thumb fills the travel.
    let fits = scrolling([false, true], [100.0, 60.0], [100.0, 50.0]);
    let kept = ordinary_scroll_bars(&fits, [100.0, 60.0], style, [0.0; 2], &[]);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].axis, 1);
    assert!(!kept[0].enabled());
    assert_rect(kept[0].thumb, [90.0, 5.0, 10.0, 50.0]);
    assert_eq!(kept[0].offset_for_thumb(30.0), 0.0);

    // Degenerate viewports and controls never produce bars.
    let flat = scrolling([true; 2], [0.0, 60.0], [100.0, 100.0]);
    assert!(ordinary_scroll_bars(&flat, [100.0, 60.0], style, [0.0; 2], &[]).is_empty());
    let open = scrolling([true; 2], [100.0, 60.0], [100.0, 100.0]);
    assert!(ordinary_scroll_bars(&open, [0.0, 60.0], style, [0.0; 2], &[]).is_empty());
}

#[test]
fn parallel_enclosing_tracks_move_a_bar_to_their_inner_edge() {
    // An outer 15-thick vertical track at 385..400 covers the inner 400 x 200
    // control's edge, wherever an outer vertical scroll moved it.
    for outer in [
        obstacle(1, [385.0, 0.0, 15.0, 300.0]),
        obstacle(1, [385.0, -100.0, 15.0, 300.0]),
    ] {
        let inner = bars(
            flush(10.0),
            [400.0, 200.0],
            [400.0, 300.0],
            [0.0; 2],
            &[outer],
        );
        assert_eq!(inner.len(), 1);
        assert_rect(inner[0].track, [375.0, 0.0, 10.0, 200.0]);
        assert_eq!(inner[0].thumb[0], 375.0);
    }

    // Without an enclosing track, or once an outer horizontal scroll moved the
    // outer track clear of this control, the bar keeps its own edge.
    assert_rect(
        bars(flush(10.0), [400.0, 200.0], [400.0, 300.0], [0.0; 2], &[])[0].track,
        [390.0, 0.0, 10.0, 200.0],
    );
    let clear = obstacle(1, [485.0, 0.0, 15.0, 300.0]);
    assert_rect(
        bars(
            flush(10.0),
            [400.0, 200.0],
            [400.0, 300.0],
            [0.0; 2],
            &[clear],
        )[0]
        .track,
        [390.0, 0.0, 10.0, 200.0],
    );
}

#[test]
fn enclosing_tracks_that_do_not_meet_leave_both_inner_bars_at_their_corner() {
    let outer = obstacle(1, [385.0, 0.0, 15.0, 300.0]);
    let inner = bars(
        flush(10.0),
        [300.0, 200.0],
        [400.0, 300.0],
        [0.0; 2],
        &[outer],
    );
    assert_rect(inner[0].track, [0.0, 190.0, 290.0, 10.0]);
    assert_rect(inner[1].track, [290.0, 0.0, 10.0, 190.0]);
}

#[test]
fn crossing_enclosing_tracks_end_a_bar_before_them() {
    // The outer horizontal track runs along y 285..300 of the outer view, which
    // is 185 in the frame of an inner control placed at y 100.
    let outer = obstacle(0, [0.0, 185.0, 400.0, 15.0]);
    let inner = bars(
        flush(10.0),
        [400.0, 200.0],
        [400.0, 300.0],
        [0.0; 2],
        &[outer],
    );
    assert_eq!(inner.len(), 1);
    assert_rect(inner[0].track, [390.0, 0.0, 10.0, 185.0]);
}
