//! Slider input beyond the horizontal rail: a vertical slider's drags, track
//! presses and keys with its minimum at the bottom, Shift's fine step, the
//! wheel, which steps a slider only while it holds focus and otherwise
//! scrolls the scroll view around it, and a dial's relative vertical drags.

use crate::components::{GuiScrollView, GuiSlider};
use crate::services::gui_input::router::*;
use crate::services::gui_input::routing_test_support::*;
use crate::services::gui_input::test_support::*;
use crate::systems::gui::local::controls::slider::slider_rail;
use crate::{ComponentValue, EntityId, WorldRef};

use GuiPhysicalKey::{Down, End, Home, Left, Right, Tab, Up};

/// A slider over 0..=10 in whole steps with a fine step of a quarter.
fn slider(axis: u32, value: f32) -> GuiSlider {
    GuiSlider {
        min: 0.0,
        max: 10.0,
        step: 1.0,
        fine_step: 0.25,
        value,
        axis,
        ..Default::default()
    }
}

/// A `4 x 12` Canvas presented at ten pixels per unit whose root column
/// holds one `2 x 10` vertical slider.
fn vertical_scene(slider: GuiSlider) -> (Rig, WorldRef, EntityId) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 4.0, 12.0);
    let entity = create(
        &mut host,
        world,
        vec![ComponentValue::GuiSlider(slider), sized(0, 2.0, 10.0)],
        Some(root_entity),
    );
    (Rig::new(host, root, viewport(40, 120)), world, entity)
}

/// Normalized viewport point of a vertical slider's thumb centre at a value
/// fraction.
fn thumb_centre(rig: &mut Rig, world: WorldRef, entity: EntityId, fraction: f32) -> [f32; 2] {
    let rect = rig.bounds(world, entity);
    let thumb = slider_rail(rect, 1).unwrap().thumb_rect(fraction).unwrap();
    rig.logical([thumb[0] + thumb[2] * 0.5, thumb[1] + thumb[3] * 0.5])
}

fn scalar(rig: &mut Rig, world: WorldRef, entity: EntityId) -> f32 {
    match rig.value(world, entity) {
        GuiTestValue::Scalar(value) => value,
        other => panic!("expected a slider value, got {other:?}"),
    }
}

#[test]
fn a_vertical_slider_maps_drags_and_track_presses_from_the_bottom() {
    let (mut rig, world, entity) = vertical_scene(slider(1, 0.0));

    // A track press near the top jumps to the nearest step there.
    let track = thumb_centre(&mut rig, world, entity, 0.82);
    rig.send(press(1, track));
    assert_eq!(scalar(&mut rig, world, entity), 8.0);
    rig.send(release(1, track));

    // Grabbing the thumb keeps its offset; dragging it down decreases the
    // value and up increases it, and captured motion clamps past the ends.
    let grab = thumb_centre(&mut rig, world, entity, 0.8);
    let [low, high] = [0.3, 0.6].map(|fraction| thumb_centre(&mut rig, world, entity, fraction));
    rig.send(press(2, grab));
    rig.send(movement(2, low));
    assert_eq!(scalar(&mut rig, world, entity), 3.0);
    rig.send(movement(2, high));
    assert_eq!(scalar(&mut rig, world, entity), 6.0);
    rig.send(movement(2, [grab[0], 0.999]));
    assert_eq!(scalar(&mut rig, world, entity), 0.0);
    rig.send(movement(2, [grab[0], 0.001]));
    assert_eq!(scalar(&mut rig, world, entity), 10.0);
    rig.send(release(2, [grab[0], 0.001]));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn arrows_step_a_vertical_slider_and_shift_takes_the_fine_step() {
    let (mut rig, world, entity) = vertical_scene(slider(1, 5.0));
    rig.send(key(Tab));
    let mut values = Vec::new();
    for input in [
        key(Up),
        key(Up),
        key(Down),
        key(Right),
        key(Left),
        shifted(Up),
        shifted(Up),
        shifted(Down),
        key(Up),
        key(End),
        shifted(Up),
        key(Home),
        shifted(Down),
        shifted(Right),
    ] {
        assert_eq!(
            rig.send(input),
            GuiRoutingDisposition::Routed {
                target: rig.snapshot(world, entity).target
            }
        );
        values.push(scalar(&mut rig, world, entity));
    }

    // Up and Right increase, Down and Left decrease; Shift moves by the
    // quarter and the next whole step snaps back to the step's grid.
    assert_eq!(
        values,
        [
            6.0, 7.0, 6.0, 7.0, 6.0, 6.25, 6.5, 6.25, 7.0, 10.0, 10.0, 0.0, 0.0, 0.25
        ]
    );
    rig.finish();
}

#[test]
fn without_a_fine_step_shift_moves_by_the_step() {
    let (mut rig, world, entity) = vertical_scene(GuiSlider {
        fine_step: 0.0,
        ..slider(1, 5.0)
    });
    rig.send(key(Tab));
    rig.send(shifted(Up));
    assert_eq!(scalar(&mut rig, world, entity), 6.0);
    rig.send(shifted(Left));
    assert_eq!(scalar(&mut rig, world, entity), 5.0);
    rig.finish();
}

/// One `10 x 10` Canvas at ten pixels per unit:
///
/// ```text
/// root Column
/// └── ScrollView 10 x 6            capacity 6
///     └── Column 10 x 12
///         ├── horizontal slider 10 x 2, value 5
///         └── spacer 10 x 10
/// ```
struct Scrolled {
    rig: Rig,
    world: WorldRef,
    view: EntityId,
    slider: EntityId,
}

impl Scrolled {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
        let view = create(
            &mut host,
            world,
            vec![
                sized(0, 10.0, 6.0),
                ComponentValue::GuiScrollView(GuiScrollView::default()),
            ],
            Some(root_entity),
        );
        let content = create(&mut host, world, vec![sized(2, 10.0, 12.0)], Some(view));
        let slider = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiSlider(slider(0, 5.0)),
                sized(0, 10.0, 2.0),
            ],
            Some(content),
        );
        create(&mut host, world, vec![sized(0, 10.0, 10.0)], Some(content));
        Self {
            rig: Rig::new(host, root, viewport(100, 100)),
            world,
            view,
            slider,
        }
    }

    /// Wheel at a logical point, with or without Shift, and apply it.
    fn wheel(&mut self, point: [f32; 2], delta: [f32; 2], shift: bool) -> GuiRoutingDisposition {
        self.rig.send(GuiPhysicalInput::Wheel {
            point: self.rig.logical(point),
            delta,
            shift,
        })
    }

    /// The slider's value and the scroll view's vertical offset.
    fn state(&mut self) -> (f32, f32) {
        (
            scalar(&mut self.rig, self.world, self.slider),
            self.rig.scroll(self.world, self.view)[1],
        )
    }
}

/// Over the slider, clear of the scroll bar at the view's right edge.
const ON_SLIDER: [f32; 2] = [3.0, 1.0];

/// Over the spacer below the slider.
const BELOW: [f32; 2] = [3.0, 4.0];

#[test]
fn the_wheel_over_an_unfocused_slider_scrolls_its_scroll_view() {
    let mut scene = Scrolled::new();
    let view = scene.rig.snapshot(scene.world, scene.view).target;
    assert_eq!(
        scene.wheel(ON_SLIDER, [0.0, 1.0], false),
        GuiRoutingDisposition::Routed {
            target: view
        }
    );
    assert_eq!(scene.state(), (5.0, 1.0));
    scene.wheel(ON_SLIDER, [0.0, -1.0], true);
    assert_eq!(scene.state(), (5.0, 0.0));
    assert!(scene.rig.commits().is_empty());
    scene.rig.finish();
}

#[test]
fn the_wheel_steps_the_focused_slider_under_it_and_scrolls_elsewhere() {
    let mut scene = Scrolled::new();
    let target = scene.rig.snapshot(scene.world, scene.slider).target;
    scene.rig.send(key(Tab));
    assert!(scene.rig.snapshot(scene.world, scene.slider).focused);
    assert_eq!(scene.state(), (5.0, 0.0));

    // Down decreases and up increases by the step, Shift by the fine step;
    // the scroll view keeps its offset, so nothing leaves the slider.
    assert_eq!(
        scene.wheel(ON_SLIDER, [0.0, 1.0], false),
        GuiRoutingDisposition::Routed {
            target
        }
    );
    assert_eq!(scene.state(), (4.0, 0.0));
    scene.wheel(ON_SLIDER, [0.0, -0.25], false);
    assert_eq!(scene.state(), (5.0, 0.0));
    scene.wheel(ON_SLIDER, [0.0, -1.0], true);
    assert_eq!(scene.state(), (5.25, 0.0));

    // Browsers deliver Shift with the wheel as horizontal movement: to the
    // right it decreases, as the wheel turned down does.
    scene.wheel(ON_SLIDER, [1.0, 0.0], true);
    assert_eq!(scene.state(), (5.0, 0.0));

    // At a bound the focused slider still takes the wheel.
    scene.rig.send(key(End));
    scene.wheel(ON_SLIDER, [0.0, -1.0], false);
    assert_eq!(scene.state(), (10.0, 0.0));

    // Off the slider the wheel scrolls, focus or not.
    scene.wheel(BELOW, [0.0, 2.0], false);
    assert_eq!(scene.state(), (10.0, 2.0));
    assert!(scene.rig.snapshot(scene.world, scene.slider).focused);
    assert!(
        scene.rig.rejected().is_empty(),
        "{:?}",
        scene.rig.rejected()
    );
    scene.rig.finish();
}

/// A Canvas whose root column holds one `8 x 8` dial: its whole range takes
/// 20 units of upward travel.
fn dial_scene(slider: GuiSlider) -> (Rig, WorldRef, EntityId) {
    let (mut host, _) = host();
    let world = world(&mut host);
    let (root, root_entity) = canvas_root(&mut host, world, 10.0, 12.0);
    let entity = create(
        &mut host,
        world,
        vec![ComponentValue::GuiSlider(slider), sized(0, 8.0, 8.0)],
        Some(root_entity),
    );
    (Rig::new(host, root, viewport(100, 120)), world, entity)
}

/// Normalized viewport point at a fraction of a control's box.
fn within(rig: &mut Rig, world: WorldRef, entity: EntityId, fraction: [f32; 2]) -> [f32; 2] {
    let [x, y, width, height] = rig.bounds(world, entity);
    rig.logical([x + width * fraction[0], y + height * fraction[1]])
}

#[test]
fn a_dial_press_keeps_the_value_and_vertical_drags_turn_it_relatively() {
    let (mut rig, world, entity) = dial_scene(slider(2, 5.0));
    let target = rig.snapshot(world, entity).target;

    // A press anywhere on the dial, even near the minimum's end of its
    // sweep, leaves the value and focuses it without the ring.
    let corner = within(&mut rig, world, entity, [0.2, 0.8]);
    assert_eq!(
        rig.send(press(1, corner)),
        GuiRoutingDisposition::Routed {
            target
        }
    );
    assert_eq!(scalar(&mut rig, world, entity), 5.0);
    assert!(rig.snapshot(world, entity).focused);

    // Upward travel of 4 units is a fifth of the range, two steps; downward
    // travel decreases, relative to the press, and sideways travel does
    // nothing.
    let unit = rig.logical([1.0, 1.0]);
    let at = |offset: [f32; 2]| {
        [
            corner[0] + offset[0] * unit[0],
            corner[1] + offset[1] * unit[1],
        ]
    };
    rig.send(movement(1, at([0.0, -4.0])));
    assert_eq!(scalar(&mut rig, world, entity), 7.0);
    rig.send(movement(1, at([0.0, 6.0])));
    assert_eq!(scalar(&mut rig, world, entity), 2.0);
    rig.send(movement(1, at([3.0, 6.0])));
    assert_eq!(scalar(&mut rig, world, entity), 2.0);

    // Captured travel past the maximum holds it there, and coming back
    // responds from the bound at once.
    rig.send(movement(1, at([0.0, -40.0])));
    assert_eq!(scalar(&mut rig, world, entity), 10.0);
    rig.send(movement(1, at([0.0, -36.0])));
    assert_eq!(scalar(&mut rig, world, entity), 8.0);
    rig.send(release(1, at([0.0, -36.0])));

    // A new press starts from the committed value.
    rig.send(press(2, corner));
    rig.send(movement(2, at([0.0, 2.0])));
    assert_eq!(scalar(&mut rig, world, entity), 7.0);
    rig.send(release(2, at([0.0, 2.0])));
    assert!(rig.rejected().is_empty(), "{:?}", rig.rejected());
    rig.finish();
}

#[test]
fn a_dial_steps_by_keys_as_every_slider_does() {
    let (mut rig, world, entity) = dial_scene(slider(2, 5.0));
    rig.send(key(Tab));
    let mut values = Vec::new();
    for input in [
        key(Up),
        key(Left),
        shifted(Up),
        shifted(Down),
        key(Right),
        key(End),
        key(Home),
    ] {
        rig.send(input);
        values.push(scalar(&mut rig, world, entity));
    }
    assert_eq!(values, [6.0, 5.0, 5.25, 5.0, 6.0, 10.0, 0.0]);
    rig.finish();
}
