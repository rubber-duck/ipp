//! Ordinary ScrollView bar geometry: thumb ratio and travel, the shared corner, the
//! minimum thumb, theme-kept bars and enclosing tracks mapped into the bar's frame.

use super::*;

fn view(viewport: [f32; 2], content: [f32; 2]) -> GuiScrollExtent {
    GuiScrollExtent {
        viewport,
        content,
        capacity: std::array::from_fn(|axis| (content[axis] - viewport[axis]).max(0.0)),
    }
}

fn bars(
    viewport: [f32; 2],
    content: [f32; 2],
    offset: [f32; 2],
    obstacles: &[GuiScrollBar],
) -> Vec<GuiScrollBar> {
    ordinary_scroll_bars(&view(viewport, content), offset, obstacles, [false; 2])
}

fn assert_rect(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-5,
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
        capacity: 1.0,
        page: 1.0,
    }
}

#[test]
fn vertical_thumb_follows_viewport_ratio_and_offset() {
    // Thickness is 5% of the 6-unit side; the thumb shows 60% of the track.
    let vertical = bars([10.0, 6.0], [10.0, 10.0], [0.0, 0.0], &[]);
    assert_eq!(vertical.len(), 1);
    let vertical = vertical[0];
    assert_eq!(vertical.axis, 1);
    assert_rect(vertical.track, [9.7, 0.0, 0.3, 6.0]);
    assert_rect(vertical.thumb, [9.7, 0.0, 0.3, 3.6]);
    assert_eq!((vertical.capacity, vertical.page), (4.0, 6.0));
    assert!(vertical.enabled());

    // The thumb travels the remaining 2.4 units in proportion to the offset.
    let middle = bars([10.0, 6.0], [10.0, 10.0], [0.0, 2.0], &[])[0];
    assert_rect(middle.thumb, [9.7, 1.2, 0.3, 3.6]);
    let end = bars([10.0, 6.0], [10.0, 10.0], [0.0, 4.0], &[])[0];
    assert_rect(end.thumb, [9.7, 2.4, 0.3, 3.6]);

    // Dragging the thumb start maps back to offsets, clamped to the track.
    assert!((middle.travel() - 2.4).abs() < 1e-5);
    assert!((middle.offset_for_thumb(1.2) - 2.0).abs() < 1e-5);
    assert_eq!(middle.offset_for_thumb(-5.0), 0.0);
    assert_eq!(middle.offset_for_thumb(50.0), 4.0);
}

#[test]
fn both_axes_share_the_corner_and_thumbs_keep_a_minimum_length() {
    let both = bars([10.0, 6.0], [20.0, 1000.0], [0.0, 0.0], &[]);
    let [horizontal, vertical] = [both[0], both[1]];
    assert_eq!((horizontal.axis, vertical.axis), (0, 1));
    assert_rect(horizontal.track, [0.0, 5.7, 9.7, 0.3]);
    assert_rect(vertical.track, [9.7, 0.0, 0.3, 5.7]);
    // Half the width is visible; the tall content hits the two-thickness minimum.
    assert_rect(horizontal.thumb, [0.0, 5.7, 4.85, 0.3]);
    assert_rect(vertical.thumb, [9.7, 0.0, 0.3, 0.6]);
}

#[test]
fn fitting_content_shows_only_theme_kept_bars_and_they_take_no_input() {
    let fits = view([10.0, 6.0], [10.0, 5.0]);
    assert!(ordinary_scroll_bars(&fits, [0.0; 2], &[], [false; 2]).is_empty());
    let kept = ordinary_scroll_bars(&fits, [0.0; 2], &[], [false, true]);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].axis, 1);
    assert!(!kept[0].enabled());
    assert_rect(kept[0].thumb, kept[0].track);
    assert_eq!(kept[0].offset_for_thumb(3.0), 0.0);

    // Degenerate viewports never produce bars.
    let flat = view([0.0, 6.0], [10.0, 10.0]);
    assert!(ordinary_scroll_bars(&flat, [0.0; 2], &[], [true; 2]).is_empty());
}

#[test]
fn parallel_enclosing_tracks_move_a_bar_to_their_inner_edge() {
    // An outer 0.15-thick vertical track at 3.85..4 covers the inner 4 x 2
    // viewport's edge, wherever an outer vertical scroll moved it.
    for outer in [
        obstacle(1, [3.85, 0.0, 0.15, 3.0]),
        obstacle(1, [3.85, -1.0, 0.15, 3.0]),
    ] {
        let inner = bars([4.0, 2.0], [4.0, 3.0], [0.0; 2], &[outer]);
        assert_eq!(inner.len(), 1);
        assert_rect(inner[0].track, [3.75, 0.0, 0.1, 2.0]);
        assert_eq!(inner[0].thumb[0], 3.75);
    }

    // Without an enclosing track, or once an outer horizontal scroll moved the
    // outer track clear of this viewport, the bar keeps its own edge.
    assert_rect(
        bars([4.0, 2.0], [4.0, 3.0], [0.0; 2], &[])[0].track,
        [3.9, 0.0, 0.1, 2.0],
    );
    let clear = obstacle(1, [4.85, 0.0, 0.15, 3.0]);
    assert_rect(
        bars([4.0, 2.0], [4.0, 3.0], [0.0; 2], &[clear])[0].track,
        [3.9, 0.0, 0.1, 2.0],
    );
}

#[test]
fn enclosing_tracks_that_do_not_meet_leave_both_inner_bars_at_their_corner() {
    let outer = obstacle(1, [3.85, 0.0, 0.15, 3.0]);
    let inner = bars([3.0, 2.0], [4.0, 3.0], [0.0; 2], &[outer]);
    assert_rect(inner[0].track, [0.0, 1.9, 2.9, 0.1]);
    assert_rect(inner[1].track, [2.9, 0.0, 0.1, 1.9]);
}

#[test]
fn crossing_enclosing_tracks_end_a_bar_before_them() {
    // The outer horizontal track runs along y 2.85..3 of the outer view, which
    // is 1.85 in the frame of an inner viewport placed at y 1.
    let outer = obstacle(0, [0.0, 1.85, 4.0, 0.15]);
    let inner = bars([4.0, 2.0], [4.0, 3.0], [0.0; 2], &[outer]);
    assert_eq!(inner.len(), 1);
    assert_rect(inner[0].track, [3.9, 0.0, 0.1, 1.85]);
}
