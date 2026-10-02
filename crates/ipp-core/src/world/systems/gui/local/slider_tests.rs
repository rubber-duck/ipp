//! Slider rail geometry in both orientations, the fill between an origin and
//! the value, the dial's square, rings, angles and relative drag, and key
//! stepping by the step or the fine step.

use super::slider::{GuiDialDrag, GuiSliderDial, nudge, slider_dial, slider_rail};

/// A 100 x 20 horizontal rail: a 15-unit thumb whose centre travels from 7.5
/// to 92.5, under a rail 8 thick.
const WIDE: [f32; 4] = [0.0, 0.0, 100.0, 20.0];

/// The same control turned upright: 20 x 100, minimum at the bottom.
const TALL: [f32; 4] = [0.0, 0.0, 20.0, 100.0];

#[test]
fn a_vertical_rail_runs_from_the_bottom_with_the_horizontal_rail_turned() {
    let wide = slider_rail(WIDE, 0).unwrap();
    let tall = slider_rail(TALL, 1).unwrap();
    assert_eq!(wide.thumb_centers(), [7.5, 92.5]);
    assert_eq!(tall.thumb_centers(), [92.5, 7.5]);

    // The rail is the whole control length, centred across it.
    assert_eq!(wide.rail_rect(8.0), [0.0, 6.0, 100.0, 8.0]);
    assert_eq!(tall.rail_rect(8.0), [6.0, 0.0, 8.0, 100.0]);

    // The thumb sits at the minimum's end at the minimum: the left, or the
    // bottom of a vertical rail.
    assert_eq!(wide.thumb_rect(0.0), Some([0.0, 2.5, 15.0, 15.0]));
    assert_eq!(tall.thumb_rect(0.0), Some([2.5, 85.0, 15.0, 15.0]));
    assert_eq!(wide.thumb_rect(1.0), Some([85.0, 2.5, 15.0, 15.0]));
    assert_eq!(tall.thumb_rect(1.0), Some([2.5, 0.0, 15.0, 15.0]));
    assert_eq!(tall.thumb_rect(0.5), Some([2.5, 42.5, 15.0, 15.0]));

    // A rail thicker than the control is clamped to it.
    assert_eq!(tall.rail_rect(30.0), [0.0, 0.0, 20.0, 100.0]);
    assert!(slider_rail(TALL, 2).is_none());
}

#[test]
fn a_fill_from_the_minimum_starts_at_the_rail_end_under_half_the_thumb() {
    let wide = slider_rail(WIDE, 0).unwrap();
    let tall = slider_rail(TALL, 1).unwrap();
    assert_eq!(wide.fill_rect(0.0, 0.0, 8.0), Some([0.0, 6.0, 7.5, 8.0]));
    assert_eq!(wide.fill_rect(0.0, 0.5, 8.0), Some([0.0, 6.0, 50.0, 8.0]));
    assert_eq!(tall.fill_rect(0.0, 0.0, 8.0), Some([6.0, 92.5, 8.0, 7.5]));
    assert_eq!(tall.fill_rect(0.0, 0.5, 8.0), Some([6.0, 50.0, 8.0, 50.0]));
    assert_eq!(tall.fill_rect(0.0, 1.0, 8.0), Some([6.0, 7.5, 8.0, 92.5]));
}

#[test]
fn a_fill_from_an_inner_origin_lies_between_the_origin_and_the_value_on_either_side() {
    let wide = slider_rail(WIDE, 0).unwrap();
    let tall = slider_rail(TALL, 1).unwrap();

    // Origin at the middle, the value a quarter of the range below or above
    // it: the fill runs from the middle to the thumb centre on that side.
    let (below, above) = (0.25, 0.75);
    assert_eq!(
        wide.fill_rect(0.5, below, 8.0),
        Some([28.75, 6.0, 21.25, 8.0])
    );
    assert_eq!(
        wide.fill_rect(0.5, above, 8.0),
        Some([50.0, 6.0, 21.25, 8.0])
    );
    assert_eq!(
        tall.fill_rect(0.5, below, 8.0),
        Some([6.0, 50.0, 8.0, 21.25])
    );
    assert_eq!(
        tall.fill_rect(0.5, above, 8.0),
        Some([6.0, 28.75, 8.0, 21.25])
    );

    // At the origin there is nothing to fill.
    assert_eq!(wide.fill_rect(0.5, 0.5, 8.0), None);
    assert_eq!(tall.fill_rect(0.5, 0.5, 8.0), None);

    // An origin off centre: from 25% of the range in either direction.
    assert_eq!(wide.fill_rect(0.25, 0.0, 8.0), Some([7.5, 6.0, 21.25, 8.0]));
    assert_eq!(tall.fill_rect(0.25, 1.0, 8.0), Some([6.0, 7.5, 8.0, 63.75]));
}

#[test]
fn a_fill_from_the_maximum_starts_at_the_far_rail_end() {
    let wide = slider_rail(WIDE, 0).unwrap();
    let tall = slider_rail(TALL, 1).unwrap();
    assert_eq!(wide.fill_rect(1.0, 1.0, 8.0), Some([92.5, 6.0, 7.5, 8.0]));
    assert_eq!(wide.fill_rect(1.0, 0.5, 8.0), Some([50.0, 6.0, 50.0, 8.0]));
    assert_eq!(tall.fill_rect(1.0, 1.0, 8.0), Some([6.0, 0.0, 8.0, 7.5]));
    assert_eq!(tall.fill_rect(1.0, 0.0, 8.0), Some([6.0, 0.0, 8.0, 92.5]));
    assert_eq!(wide.fill_rect(f32::NAN, 0.5, 8.0), None);
}

#[test]
fn keys_step_by_the_step_and_snap_to_its_grid_and_the_fine_step_snaps_to_its_own() {
    // Step 10 on 0..100: a key from 20 reaches 30 and from 21 snaps to 30.
    assert_eq!(nudge(0.0, 100.0, 10.0, 20.0, 1.0), Some(30.0));
    assert_eq!(nudge(0.0, 100.0, 10.0, 21.0, 1.0), Some(30.0));
    assert_eq!(nudge(0.0, 100.0, 10.0, 21.0, -1.0), Some(10.0));

    // The fine step of 1 keeps the off-grid values the step would snap.
    assert_eq!(nudge(0.0, 100.0, 1.0, 20.0, 1.0), Some(21.0));
    assert_eq!(nudge(0.0, 100.0, 1.0, 21.0, -1.0), Some(20.0));

    // Bipolar ranges step from their minimum's grid through zero.
    assert_eq!(nudge(-100.0, 100.0, 10.0, -5.0, 1.0), Some(10.0));
    assert_eq!(nudge(-100.0, 100.0, 1.0, -1.0, 1.0), Some(0.0));

    // Clamped at both ends; continuous ranges move a hundredth of the range.
    assert_eq!(nudge(0.0, 100.0, 10.0, 95.0, 1.0), Some(100.0));
    assert_eq!(nudge(0.0, 100.0, 10.0, 5.0, -1.0), Some(0.0));
    assert_eq!(nudge(0.0, 100.0, 0.0, 50.0, 1.0), Some(51.0));
}

#[test]
fn a_dial_draws_in_the_top_left_square_of_its_control() {
    // An 80-unit square dial at 16-unit type: a tick ring 32 from the centre,
    // ticks 4 long, a quarter-em gap and a 4-unit value arc centred 22 out.
    let square = slider_dial([10.0, 20.0, 80.0, 80.0]).unwrap();
    assert_eq!(square.ticks_radius(16.0), 32.0);
    assert_eq!(square.ring_radius(16.0, 4.0, 4.0), 22.0);
    assert_eq!(square.square(32.0), [18.0, 28.0, 64.0, 64.0]);
    assert_eq!(square.travel(), 200.0);

    // A taller control keeps the dial in its top square, leaving the room
    // beneath; a wider one keeps it at its left.
    let tall = slider_dial([0.0, 0.0, 80.0, 112.0]).unwrap();
    assert_eq!(tall.square(40.0), [0.0, 0.0, 80.0, 80.0]);
    let wide = slider_dial([0.0, 0.0, 120.0, 80.0]).unwrap();
    assert_eq!(wide.square(40.0), [0.0, 0.0, 80.0, 80.0]);

    // Rings never turn inside out on a dial too small for them.
    let tiny = slider_dial([0.0, 0.0, 12.0, 12.0]).unwrap();
    assert_eq!(tiny.ring_radius(16.0, 4.0, 4.0), 0.0);
    assert!(slider_dial([0.0, 0.0, 0.0, 10.0]).is_none());
    assert!(slider_dial([0.0, 0.0, f32::NAN, 10.0]).is_none());
}

#[test]
fn a_dial_sweeps_270_degrees_clockwise_with_the_gap_at_the_bottom() {
    // The minimum at half past seven, the middle at twelve and the maximum
    // at half past four, in turns clockwise from twelve.
    assert_eq!(GuiSliderDial::angle(0.0), 0.625);
    assert_eq!(GuiSliderDial::angle(0.5), 1.0);
    assert_eq!(GuiSliderDial::angle(1.0), 1.375);
    assert_eq!(GuiSliderDial::angle(2.0), 1.375);
    assert_eq!(GuiSliderDial::sweep(), [0.625, 0.75]);

    // The value arc runs from the origin to the value either way round, and
    // vanishes where they meet: a bipolar dial fills from its middle.
    assert_eq!(GuiSliderDial::value_arc(0.0, 0.5), Some([0.625, 0.375]));
    assert_eq!(GuiSliderDial::value_arc(0.5, 0.25), Some([0.8125, 0.1875]));
    assert_eq!(GuiSliderDial::value_arc(0.5, 0.75), Some([1.0, 0.1875]));
    assert_eq!(GuiSliderDial::value_arc(0.5, 0.5), None);
    assert_eq!(GuiSliderDial::value_arc(0.0, f32::NAN), None);
}

#[test]
fn a_dial_drag_is_relative_and_restarts_at_a_bound() {
    // From a press at y 100 on the value at 40% of a dial whose whole range
    // takes 200 units: up increases, down decreases, in proportion.
    let drag = GuiDialDrag {
        origin: 100.0,
        start: 0.4,
    };
    assert_eq!(drag.turned(100.0, 200.0), Some((0.4, drag)));
    assert_eq!(drag.turned(60.0, 200.0).unwrap().0, 0.6);
    assert_eq!(drag.turned(140.0, 200.0).unwrap().0, 0.2);

    // Past the maximum the value holds there and the drag restarts from the
    // pointer, so coming back down responds at once.
    let (top, past) = drag.turned(-60.0, 200.0).unwrap();
    assert_eq!(
        (top, past),
        (
            1.0,
            GuiDialDrag {
                origin: -60.0,
                start: 1.0,
            }
        )
    );
    assert_eq!(past.turned(-20.0, 200.0).unwrap().0, 0.8);
    let (bottom, _) = drag.turned(400.0, 200.0).unwrap();
    assert_eq!(bottom, 0.0);
    assert_eq!(drag.turned(f32::NAN, 200.0), None);
    assert_eq!(drag.turned(100.0, 0.0), None);
}
