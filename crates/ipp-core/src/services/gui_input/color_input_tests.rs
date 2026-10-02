//! Colour controls through routed input: the field, the hue rail and the alpha
//! rail are focus parts of one control, each a Tab stop with its own arrows,
//! Home and End and its own pointer feedback; a press or drag sets only the
//! channels of the surface it started on, the swatch takes no input, and the
//! wheel steps only the focused rail under the pointer.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiButton, GuiColor};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::GuiLocalAction;
use crate::systems::gui::local::color::GuiColorLayout;
use crate::systems::gui::presentation::GuiFont;

use GuiPhysicalKey::{BackTab, Down, End, Home, Left, Right, Tab, Up};

/// The control's font size: two units per em, so a `30 x 25` control has a
/// `18 x 18` field at `[1, 1]`, rails 3 wide at x 21 and 26 and a swatch 3
/// tall at y 21.
const EM: f32 = 2.0;
const SIZE: [f32; 2] = [30.0, 25.0];

/// A colour control and a button after it in a `32 x 28` Canvas presented at
/// ten pixels per unit.
struct Scene {
    rig: Rig,
    world: WorldRef,
    color: EntityId,
    button: EntityId,
    layout: GuiColorLayout,
}

impl Scene {
    fn new(color: GuiColor) -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 32.0, 28.0);
        let entity = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiColor(color),
                sized(0, SIZE[0], SIZE[1]),
                ComponentValue::GuiFont(GuiFont {
                    source: "".into(),
                    variant: 0,
                    font_size: EM,
                }),
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
        Self {
            rig: Rig::new(host, root, viewport(320, 280)),
            world,
            color: entity,
            button,
            layout: GuiColorLayout::new(SIZE, EM, color.alpha_rail),
        }
    }

    /// Normalized viewport point at a fraction of a surface, from its top-left.
    fn at(&mut self, part: u32, fraction: [f32; 2]) -> [f32; 2] {
        let [x, y, width, height] = self.layout.surface(part).unwrap();
        let bounds = self.rig.bounds(self.world, self.color);
        self.rig.logical([
            bounds[0] + x + width * fraction[0],
            bounds[1] + y + height * fraction[1],
        ])
    }

    /// Normalized viewport point at a fraction of the swatch.
    fn on_swatch(&mut self) -> [f32; 2] {
        let [x, y, width, height] = self.layout.swatch;
        let bounds = self.rig.bounds(self.world, self.color);
        self.rig
            .logical([bounds[0] + x + width * 0.5, bounds[1] + y + height * 0.5])
    }

    fn channels(&mut self) -> [f32; 4] {
        match self.rig.value(self.world, self.color) {
            GuiTestValue::Color(channels) => channels,
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    /// Check the channels within float rounding of the steps.
    fn expect(&mut self, expected: [f32; 4]) {
        let channels = self.channels();
        assert!(
            channels
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-5),
            "{channels:?} != {expected:?}"
        );
    }

    fn focus_part(&mut self) -> Option<u32> {
        self.rig.snapshot(self.world, self.color).focus_part
    }

    /// Send `input` and check that it reached the control.
    fn routed(&mut self, input: GuiPhysicalInput) {
        let target = self.rig.snapshot(self.world, self.color).target;
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

fn color(hue: f32, saturation: f32, value: f32, alpha: f32) -> GuiColor {
    GuiColor {
        hue,
        saturation,
        value,
        alpha,
        alpha_rail: true,
    }
}

#[test]
fn tab_visits_the_field_then_the_hue_and_alpha_rails_each_with_its_own_keys() {
    let mut scene = Scene::new(color(0.5, 0.5, 0.5, 1.0));
    assert_eq!(scene.layout.field, [1.0, 1.0, 18.0, 18.0]);

    // The field: Left and Right step the saturation, Down and Up the value,
    // by a hundredth or with Shift a thousandth; Home and End take the
    // saturation to its bounds.
    scene.routed(key(Tab));
    assert_eq!(scene.focus_part(), Some(0));
    let steps = [
        (key(Right), [0.5, 0.51, 0.5, 1.0]),
        (key(Up), [0.5, 0.51, 0.51, 1.0]),
        (shifted(Left), [0.5, 0.509, 0.51, 1.0]),
        (shifted(Down), [0.5, 0.509, 0.509, 1.0]),
        (key(Home), [0.5, 0.0, 0.509, 1.0]),
        (key(Left), [0.5, 0.0, 0.509, 1.0]),
        (key(End), [0.5, 1.0, 0.509, 1.0]),
    ];
    for (input, expected) in steps {
        scene.routed(input);
        scene.expect(expected);
    }

    // The hue rail: Right and Up raise the hue, Left and Down lower it, and
    // Home and End take it to red at either end without wrapping.
    scene.routed(key(Tab));
    assert_eq!(scene.focus_part(), Some(1));
    for (input, hue) in [
        (key(Up), 0.51),
        (key(Right), 0.52),
        (shifted(Down), 0.519),
        (key(End), 1.0),
        (key(Up), 1.0),
        (key(Home), 0.0),
        (key(Left), 0.0),
    ] {
        scene.routed(input);
        scene.expect([hue, 1.0, 0.509, 1.0]);
    }

    // The alpha rail.
    scene.routed(key(Tab));
    assert_eq!(scene.focus_part(), Some(2));
    scene.routed(key(Down));
    scene.expect([0.0, 1.0, 0.509, 0.99]);
    scene.routed(key(Home));
    scene.expect([0.0, 1.0, 0.509, 0.0]);

    // Tab leaves the control after its alpha rail, and BackTab returns
    // through the rails.
    scene.rig.send(key(Tab));
    assert!(scene.rig.snapshot(scene.world, scene.button).focused);
    scene.routed(key(BackTab));
    assert_eq!(scene.focus_part(), Some(2));
    scene.routed(key(BackTab));
    assert_eq!(scene.focus_part(), Some(1));
    scene.routed(key(BackTab));
    assert_eq!(scene.focus_part(), Some(0));
    scene.finish();
}

#[test]
fn a_drag_sets_only_the_channels_of_the_surface_it_started_on() {
    let mut scene = Scene::new(color(0.5, 0.5, 0.5, 0.5));

    // A press on the field takes and focuses it and sets the saturation and
    // value at the pointer; only the field shows the press.
    let field = scene.at(0, [0.25, 0.25]);
    scene.routed(press(1, field));
    scene.expect([0.5, 0.25, 0.75, 0.5]);
    assert_eq!(scene.focus_part(), Some(0));
    let parts = scene.rig.snapshot(scene.world, scene.color).parts;
    assert!(parts[0].pressed && !parts[1].pressed && !parts[2].pressed);

    // Dragged over the hue rail and below the field, it holds the field's
    // edges and still moves only the saturation and value.
    let past = scene.at(1, [0.5, 1.2]);
    scene.routed(movement(1, past));
    scene.expect([0.5, 1.0, 0.0, 0.5]);
    scene.routed(release(1, past));

    // A press on the hue rail a quarter from its top sets the hue alone.
    let hue = scene.at(1, [0.5, 0.25]);
    scene.routed(press(2, hue));
    scene.expect([0.75, 1.0, 0.0, 0.5]);
    assert_eq!(scene.focus_part(), Some(1));
    let alpha = scene.at(2, [0.5, 0.9]);
    scene.routed(movement(2, alpha));
    scene.expect([0.1, 1.0, 0.0, 0.5]);
    scene.routed(release(2, alpha));

    // The alpha rail.
    scene.routed(press(3, alpha));
    scene.expect([0.1, 1.0, 0.0, 0.1]);
    assert_eq!(scene.focus_part(), Some(2));
    scene.routed(release(3, alpha));
    scene.finish();
}

#[test]
fn the_swatch_takes_no_input_and_hover_lights_only_the_surface_under_the_pointer() {
    let mut scene = Scene::new(color(0.5, 0.5, 0.5, 0.5));
    let swatch = scene.on_swatch();
    scene.routed(press(1, swatch));
    scene.routed(release(1, swatch));
    scene.expect([0.5, 0.5, 0.5, 0.5]);
    assert_eq!(scene.focus_part(), None);

    // Focus stays on a rail a swatch press leaves.
    scene.routed(key(Tab));
    scene.routed(key(Tab));
    scene.routed(press(1, swatch));
    scene.routed(release(1, swatch));
    assert_eq!(scene.focus_part(), Some(1));

    let rail = scene.at(2, [0.5, 0.5]);
    scene.routed(movement(2, rail));
    let parts = scene.rig.snapshot(scene.world, scene.color).parts;
    assert!(parts[2].hovered && !parts[0].hovered && !parts[1].hovered);
    scene.routed(movement(2, swatch));
    let read = scene.rig.snapshot(scene.world, scene.color);
    assert!(read.interaction.hovered);
    assert!(read.parts.iter().all(|part| !part.hovered));
    scene.finish();
}

#[test]
fn the_wheel_steps_the_focused_rail_only_over_that_rail() {
    let mut scene = Scene::new(color(0.5, 0.5, 0.5, 0.5));
    let wheel = |point: [f32; 2], delta: f32, shift: bool| GuiPhysicalInput::Wheel {
        point,
        delta: [0.0, delta],
        shift,
    };
    let [field, hue, alpha] = [0, 1, 2].map(|part| scene.at(part, [0.5, 0.5]));

    // Over the focused field the wheel scrolls, here nothing.
    scene.routed(key(Tab));
    assert_eq!(
        scene.rig.send(wheel(field, -1.0, false)),
        GuiRoutingDisposition::Unhandled
    );
    scene.expect([0.5, 0.5, 0.5, 0.5]);

    // Over the focused hue rail it steps the hue, finely with Shift; over
    // another surface it leaves the colour.
    scene.routed(key(Tab));
    scene.routed(wheel(hue, -1.0, false));
    scene.expect([0.51, 0.5, 0.5, 0.5]);
    scene.routed(wheel(hue, 1.0, true));
    scene.expect([0.509, 0.5, 0.5, 0.5]);
    assert_eq!(
        scene.rig.send(wheel(alpha, -1.0, false)),
        GuiRoutingDisposition::Unhandled
    );
    scene.routed(key(Tab));
    scene.routed(wheel(alpha, 1.0, false));
    scene.expect([0.509, 0.5, 0.5, 0.49]);
    scene.finish();
}

#[test]
fn client_colours_stay_in_range_and_client_focus_names_a_surface() {
    let mut scene = Scene::new(GuiColor {
        alpha_rail: false,
        ..color(0.5, 0.5, 0.5, 1.0)
    });
    let target = scene.rig.snapshot(scene.world, scene.color).target;
    let world = scene.world;
    let action = |action| Command::GuiAction {
        target: crate::GuiActionTarget {
            entity: EntityRef::Handle(target.entity),
            component: target.component,
            incarnation: target.incarnation,
        },
        action,
    };

    // A colour outside the channels' range is refused and changes nothing.
    for refused in [
        [1.5, 0.5, 0.5, 1.0],
        [0.5, -0.1, 0.5, 1.0],
        [0.5, 0.5, f32::NAN, 1.0],
    ] {
        let outcome = batch(
            &mut scene.rig.host,
            world,
            vec![action(GuiLocalAction::SetColor(refused))],
        );
        assert_eq!(
            outcome.result.unwrap_err().reason,
            ErrorReason::InvalidValue
        );
    }
    scene.expect([0.5, 0.5, 0.5, 1.0]);
    batch(
        &mut scene.rig.host,
        world,
        vec![action(GuiLocalAction::SetColor([0.25, 1.0, 0.0, 0.5]))],
    )
    .result
    .unwrap();
    scene.expect([0.25, 1.0, 0.0, 0.5]);

    // Without the alpha rail the control has two parts.
    gui_action(&mut scene.rig.host, target, GuiLocalAction::Focus(1));
    scene.rig.frame();
    assert_eq!(scene.focus_part(), Some(1));
    let refused = batch(
        &mut scene.rig.host,
        world,
        vec![action(GuiLocalAction::Focus(2))],
    );
    assert_eq!(
        refused.result.unwrap_err().reason,
        ErrorReason::UnsupportedAction
    );
    scene.finish();
}
