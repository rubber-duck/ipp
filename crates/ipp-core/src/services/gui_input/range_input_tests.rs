//! Range sliders through routed input: two thumbs that are focus parts of one
//! control, each a Tab stop with its own arrows, Home, End and wheel and its
//! own pointer feedback; drags and track presses that move one thumb and stop
//! at the other; the pointer's choice where the thumbs overlap; and client
//! writes and focus that name a thumb.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiButton, GuiSlider};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::slider::slider_rail;
use crate::systems::gui::local::{GuiInteractionFlags, GuiLocalAction};

use GuiPhysicalKey::{BackTab, Down, End, Home, Left, Right, Tab, Up};

/// A range over 0..=10 in whole steps with a fine step of a quarter.
fn range(lower: f32, upper: f32, axis: u32) -> GuiSlider {
    GuiSlider {
        min: 0.0,
        max: 10.0,
        step: 1.0,
        fine_step: 0.25,
        value: lower,
        upper,
        range: true,
        axis,
        ..Default::default()
    }
}

/// A range slider and a button after it, in a Canvas presented at ten pixels
/// per unit: a `10 x 2` horizontal slider in a `12 x 4` Canvas, or a `2 x 10`
/// vertical one in a `4 x 12` Canvas.
struct Scene {
    rig: Rig,
    world: WorldRef,
    slider: EntityId,
    button: EntityId,
    axis: usize,
}

impl Scene {
    fn new(slider: GuiSlider) -> Self {
        let axis = slider.axis as usize;
        let (mut host, _) = host();
        let world = world(&mut host);
        let (extent, size) = if axis == 0 {
            ([12.0, 4.0], [10.0, 2.0])
        } else {
            ([4.0, 12.0], [2.0, 10.0])
        };
        let (root, root_entity) = canvas_root(&mut host, world, extent[0], extent[1]);
        let entity = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiSlider(slider),
                sized(0, size[0], size[1]),
            ],
            Some(root_entity),
        );
        let button = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                sized(0, 1.0, 1.0),
            ],
            Some(root_entity),
        );
        let pixels = extent.map(|units| (units * 10.0) as u32);
        Self {
            rig: Rig::new(host, root, viewport(pixels[0], pixels[1])),
            world,
            slider: entity,
            button,
            axis,
        }
    }

    /// Normalized viewport point on the rail where a thumb centre would be at
    /// a value fraction.
    fn at(&mut self, fraction: f32) -> [f32; 2] {
        let rect = self.rig.bounds(self.world, self.slider);
        let thumb = slider_rail(rect, self.axis)
            .unwrap()
            .thumb_rect(fraction)
            .unwrap();
        self.rig
            .logical([thumb[0] + thumb[2] * 0.5, thumb[1] + thumb[3] * 0.5])
    }

    fn values(&mut self) -> [f32; 2] {
        match self.rig.value(self.world, self.slider) {
            GuiTestValue::Range(values) => values,
            other => panic!("expected a range, got {other:?}"),
        }
    }

    /// The thumb focus names, if focus is on the slider.
    fn focus_part(&mut self) -> Option<u32> {
        self.rig.snapshot(self.world, self.slider).focus_part
    }

    fn parts(&mut self) -> Vec<GuiInteractionFlags> {
        self.rig.snapshot(self.world, self.slider).parts
    }

    /// Send `input` and check that it reached the slider.
    fn routed(&mut self, input: GuiPhysicalInput) {
        let target = self.rig.snapshot(self.world, self.slider).target;
        assert_eq!(
            self.rig.send(input),
            GuiRoutingDisposition::Routed {
                target
            }
        );
    }

    fn finish(self) {
        assert!(self.rig.rejected().is_empty(), "{:?}", self.rig.rejected());
        self.rig.finish();
    }
}

#[test]
fn tab_reaches_the_lower_then_the_upper_thumb_and_each_takes_its_own_keys() {
    let mut scene = Scene::new(range(2.0, 8.0, 0));
    scene.routed(key(Tab));
    assert_eq!(scene.focus_part(), Some(0));

    // Arrows move only the focused thumb; End takes the lower thumb up to
    // the upper one, its legal bound, and no further.
    let mut values = Vec::new();
    for input in [key(Right), key(End), key(Right), key(Left)] {
        scene.routed(input);
        values.push(scene.values());
    }
    assert_eq!(values, [[3.0, 8.0], [8.0, 8.0], [8.0, 8.0], [7.0, 8.0]]);

    // The upper thumb is the next stop of the same control; Home takes it
    // down to the lower thumb and End to the maximum.
    scene.routed(key(Tab));
    assert_eq!(scene.focus_part(), Some(1));
    let mut values = Vec::new();
    for input in [
        key(Left),
        key(Down),
        key(Home),
        key(End),
        shifted(Down),
        key(Up),
    ] {
        scene.routed(input);
        values.push(scene.values());
    }
    assert_eq!(
        values,
        [
            [7.0, 7.0],
            [7.0, 7.0],
            [7.0, 7.0],
            [7.0, 10.0],
            [7.0, 9.75],
            [7.0, 10.0]
        ]
    );

    // Tab leaves the slider after its upper thumb, and BackTab returns
    // through the upper thumb to the lower one.
    scene.rig.send(key(Tab));
    assert!(scene.rig.snapshot(scene.world, scene.button).focused);
    scene.routed(key(BackTab));
    assert_eq!(scene.focus_part(), Some(1));
    scene.routed(key(BackTab));
    assert_eq!(scene.focus_part(), Some(0));
    scene.finish();
}

#[test]
fn a_drag_moves_the_thumb_it_started_on_and_stops_at_the_other() {
    let mut scene = Scene::new(range(2.0, 6.0, 0));
    let [lower, past, back] = [0.2, 0.9, 0.4].map(|fraction| scene.at(fraction));

    // The press takes and focuses the lower thumb without moving it, and
    // only that thumb shows the press.
    scene.routed(press(1, lower));
    assert_eq!(scene.values(), [2.0, 6.0]);
    assert_eq!(scene.focus_part(), Some(0));
    let parts = scene.parts();
    assert!(parts[0].pressed && !parts[1].pressed && !parts[1].hovered);

    // Dragged past the upper thumb it stops there, and dragged back it is
    // still the lower thumb that moves: the thumbs never swap.
    scene.routed(movement(1, past));
    assert_eq!(scene.values(), [6.0, 6.0]);
    scene.routed(movement(1, back));
    assert_eq!(scene.values(), [4.0, 6.0]);
    scene.routed(release(1, back));
    assert!(!scene.parts()[0].pressed);
    scene.finish();
}

#[test]
fn a_track_press_moves_the_nearer_thumb_to_the_pointer() {
    let mut scene = Scene::new(range(2.0, 8.0, 0));
    for (fraction, expected, part) in [
        (0.9, [2.0, 9.0], 1),
        (0.4, [4.0, 9.0], 0),
        (0.7, [4.0, 7.0], 1),
    ] {
        let point = scene.at(fraction);
        scene.routed(press(1, point));
        assert_eq!(scene.values(), expected);
        assert_eq!(scene.focus_part(), Some(part));
        scene.routed(release(1, point));
    }
    scene.finish();
}

#[test]
fn overlapping_thumbs_at_an_end_give_the_pointer_the_thumb_that_can_move() {
    // Both at the maximum: only the lower thumb can move.
    let mut scene = Scene::new(range(10.0, 10.0, 0));
    let [end, middle] = [1.0, 0.5].map(|fraction| scene.at(fraction));
    scene.routed(press(1, end));
    scene.routed(movement(1, middle));
    assert_eq!(scene.values(), [5.0, 10.0]);
    scene.routed(release(1, middle));
    scene.finish();

    // Both at the minimum: only the upper thumb can move.
    let mut scene = Scene::new(range(0.0, 0.0, 0));
    let [end, middle] = [0.0, 0.5].map(|fraction| scene.at(fraction));
    scene.routed(press(1, end));
    scene.routed(movement(1, middle));
    assert_eq!(scene.values(), [0.0, 5.0]);
    scene.routed(release(1, middle));
    scene.finish();
}

#[test]
fn overlapping_thumbs_inside_the_range_give_the_pointer_the_last_active_thumb() {
    let mut scene = Scene::new(range(5.0, 5.0, 0));
    let [shared, above, below] = [0.5, 0.8, 0.2].map(|fraction| scene.at(fraction));

    // The keyboard made the upper thumb the active one: a press on the
    // shared position drags it, and it cannot pass below the lower thumb.
    scene.routed(key(Tab));
    scene.routed(key(Tab));
    scene.routed(press(1, shared));
    scene.routed(movement(1, below));
    assert_eq!(scene.values(), [5.0, 5.0]);
    scene.routed(movement(1, above));
    assert_eq!(scene.values(), [5.0, 8.0]);
    scene.routed(movement(1, shared));
    scene.routed(release(1, shared));
    assert_eq!(scene.focus_part(), Some(1));

    // After BackTab the lower thumb is the active one.
    scene.routed(key(BackTab));
    scene.routed(press(2, shared));
    scene.routed(movement(2, below));
    assert_eq!(scene.values(), [2.0, 5.0]);
    scene.routed(release(2, below));
    scene.finish();
}

#[test]
fn the_wheel_steps_the_focused_thumb() {
    let mut scene = Scene::new(range(2.0, 8.0, 0));
    let over = scene.at(0.5);
    let wheel = |delta: f32, shift: bool| GuiPhysicalInput::Wheel {
        point: over,
        delta: [0.0, delta],
        shift,
    };
    scene.routed(key(Tab));
    scene.routed(key(Tab));
    scene.routed(wheel(1.0, false));
    assert_eq!(scene.values(), [2.0, 7.0]);
    scene.routed(wheel(-1.0, true));
    assert_eq!(scene.values(), [2.0, 7.25]);
    scene.routed(key(BackTab));
    scene.routed(wheel(-1.0, false));
    assert_eq!(scene.values(), [3.0, 7.25]);
    scene.finish();
}

#[test]
fn hover_lights_only_the_thumb_under_the_pointer() {
    let mut scene = Scene::new(range(2.0, 8.0, 0));
    let [upper, track] = [0.8, 0.5].map(|fraction| scene.at(fraction));
    scene.routed(movement(1, upper));
    let parts = scene.parts();
    assert!(parts[1].hovered && !parts[0].hovered);

    // Over the track between them the control is hovered and no thumb is.
    scene.routed(movement(1, track));
    assert!(
        scene
            .rig
            .snapshot(scene.world, scene.slider)
            .interaction
            .hovered
    );
    assert!(scene.parts().iter().all(|part| !part.hovered));
    scene.finish();
}

#[test]
fn a_vertical_range_keeps_its_lower_thumb_below_and_up_increases() {
    let mut scene = Scene::new(range(2.0, 8.0, 1));
    let [lower, upper] = [0.2, 0.8].map(|fraction| scene.at(fraction));
    assert!(lower[1] > upper[1], "the lower thumb lies below the upper");

    scene.routed(key(Tab));
    scene.routed(key(Up));
    assert_eq!(scene.values(), [3.0, 8.0]);

    // A press near the top moves the upper thumb, which is nearer.
    let top = scene.at(0.92);
    scene.routed(press(1, top));
    assert_eq!(scene.values(), [3.0, 9.0]);
    assert_eq!(scene.focus_part(), Some(1));
    scene.routed(release(1, top));
    scene.finish();
}

#[test]
fn client_writes_keep_the_order_and_client_focus_names_a_thumb() {
    let mut scene = Scene::new(range(2.0, 8.0, 0));
    let target = scene.rig.snapshot(scene.world, scene.slider).target;
    let world = scene.world;
    let write = |offset: usize, value: f32| Command::SetField {
        entity: EntityRef::Handle(target.entity),
        component: ComponentValue::GUI_SLIDER,
        field: crate::FieldWrite {
            offset: offset as u32,
            value: crate::FieldValue::F32(value),
        },
    };
    let lower = std::mem::offset_of!(GuiSlider, value);
    let upper = std::mem::offset_of!(GuiSlider, upper);

    // A write that would put the lower value above the upper one is refused
    // and changes nothing; both values change together in one write.
    let refused = batch(&mut scene.rig.host, world, vec![write(lower, 9.0)]);
    assert_eq!(
        refused.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    let refused = batch(&mut scene.rig.host, world, vec![write(upper, 1.0)]);
    assert_eq!(
        refused.result.unwrap_err().reason,
        ErrorReason::InvalidValue
    );
    assert_eq!(scene.values(), [2.0, 8.0]);
    let both = Command::InsertComponent {
        entity: EntityRef::Handle(target.entity),
        component: ComponentValue::GUI_SLIDER,
        fields: vec![
            crate::FieldWrite {
                offset: lower as u32,
                value: crate::FieldValue::F32(9.0),
            },
            crate::FieldWrite {
                offset: upper as u32,
                value: crate::FieldValue::F32(10.0),
            },
        ],
        adopt: true,
    };
    batch(&mut scene.rig.host, world, vec![both])
        .result
        .unwrap();
    assert_eq!(scene.values(), [9.0, 10.0]);

    // A semantic action sets the lower value within the same order.
    gui_action(&mut scene.rig.host, target, GuiLocalAction::SetScalar(10.0));
    scene.rig.frame();
    assert_eq!(scene.values(), [10.0, 10.0]);

    // Focus a client names the upper thumb of; the context adopts it, so
    // the next key moves that thumb. A thumb the slider lacks is refused.
    gui_action(&mut scene.rig.host, target, GuiLocalAction::Focus(1));
    scene.rig.frame();
    assert_eq!(scene.focus_part(), Some(1));
    scene.rig.synchronize();
    scene.rig.frame();
    scene.routed(key(Left));
    assert_eq!(scene.values(), [10.0, 10.0]);
    scene.routed(key(Home));
    scene.routed(key(BackTab));
    scene.routed(key(Left));
    assert_eq!(scene.values(), [9.0, 10.0]);
    let refused = batch(
        &mut scene.rig.host,
        world,
        vec![Command::GuiAction {
            target: crate::GuiActionTarget {
                entity: EntityRef::Handle(target.entity),
                component: target.component,
                incarnation: target.incarnation,
            },
            action: GuiLocalAction::Focus(2),
        }],
    );
    assert_eq!(
        refused.result.unwrap_err().reason,
        ErrorReason::UnsupportedAction
    );
    scene.finish();
}
