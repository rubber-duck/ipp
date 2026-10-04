//! Skin transitions between interaction states through real Host mutation,
//! routed pointer feedback and retained Canvas output. Expected samples are
//! computed here from the endpoints each control paints once settled, the
//! elapsed Host time and the easing curves, independently of the runtime's
//! tables.

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiColor, GuiFont, GuiLayout, GuiSlider, GuiTextInput,
};
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputContext, GuiInputService,
    GuiInputSession, GuiPointerLease,
};
use ipp_core::systems::canvas::{
    CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
};
use ipp_core::systems::gui::local::{
    GuiInteractionPart, GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand, GuiLocalEffect,
    GuiNumberStep,
};
use ipp_core::systems::gui::motion::{GuiMotionPart, GuiThemeMotion};
use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};
use ipp_core::systems::gui::{
    GuiPartId, GuiPreferencesUpdate, GuiPrimitivePart, GuiSkinState, GuiSystem, gui_skin_looks,
};
use ipp_core::*;
use std::collections::BTreeMap;
use support::CanvasTestHost;
use support::gui_panel::{ControlValue, read_control};
use support::selection::{CAMERA, GUI_LAYOUT, RENDER, select};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

struct Applied;

impl GuiDeliveryPermit for Applied {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        assert!(
            matches!(
                terminal,
                GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
            ),
            "{terminal:?}"
        );
    }
}

/// What one painted box part shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Painted {
    position: [f32; 2],
    opacity: f32,
    fill: [f32; 4],
    border_color: [f32; 4],
    border_width: f32,
    glow: f32,
}

impl Painted {
    /// `t` of the way from `self` to `to`, lane by lane.
    fn lerp(&self, to: &Self, t: f64) -> Self {
        let t = t as f32;
        let mix = |from: f32, to: f32| from + (to - from) * t;
        let lanes = |from: [f32; 4], to: [f32; 4]| std::array::from_fn(|i| mix(from[i], to[i]));
        Self {
            position: [
                mix(self.position[0], to.position[0]),
                mix(self.position[1], to.position[1]),
            ],
            opacity: mix(self.opacity, to.opacity),
            fill: lanes(self.fill, to.fill),
            border_color: lanes(self.border_color, to.border_color),
            border_width: mix(self.border_width, to.border_width),
            glow: mix(self.glow, to.glow),
        }
    }

    fn assert_near(&self, expected: &Self, context: &str) {
        let lanes = [
            (self.position.as_slice(), expected.position.as_slice()),
            (
                std::slice::from_ref(&self.opacity),
                std::slice::from_ref(&expected.opacity),
            ),
            (self.fill.as_slice(), expected.fill.as_slice()),
            (
                self.border_color.as_slice(),
                expected.border_color.as_slice(),
            ),
            (
                std::slice::from_ref(&self.border_width),
                std::slice::from_ref(&expected.border_width),
            ),
            (
                std::slice::from_ref(&self.glow),
                std::slice::from_ref(&expected.glow),
            ),
        ];
        let close = lanes.iter().all(|(actual, expected)| {
            actual
                .iter()
                .zip(*expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1.0e-4)
        });
        assert!(close, "{context}: {self:?} != {expected:?}");
    }
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

struct Fixture {
    host: HostRuntime,
    world: WorldId,
    canvas: EntityId,
    control: EntityId,
    output: OutputRef,
    input: GuiInputService,
    session: GuiInputSession,
    context: Option<GuiInputContext>,
    lease: Option<GuiPointerLease>,
    request: u64,
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    components: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        components
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations).result.unwrap()[0].1
}

/// Apply one batch in a frame of zero Host time.
fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn layout() -> ComponentValue {
    ComponentValue::GuiLayout(GuiLayout {
        width: 96.0,
        height: 32.0,
        ..Default::default()
    })
}

impl Fixture {
    /// An unthemed control in a World that selects AnimationSystem.
    fn new(control: ComponentValue) -> Self {
        Self::with_systems(&select(&[CAMERA, GUI_LAYOUT, RENDER]), control)
    }

    fn with_systems(systems: &[ipp_core::systems::SystemId], control: ComponentValue) -> Self {
        let mut host = crate::support::task_scheduler::host();
        let world = host.create_world(Default::default(), systems).unwrap();
        let canvas = create(&mut host, world, vec![], None);
        let control = create(&mut host, world, vec![control, layout()], Some(canvas));
        let world_ref = host.world_ref(world).unwrap();
        let output = host.canvas_output(world_ref, [200.0, 100.0], 1.0);
        let input = GuiInputService::default();
        let session = input.open_session().unwrap();
        let mut fixture = Self {
            host,
            world,
            canvas,
            control,
            output,
            input,
            session,
            context: None,
            lease: None,
            request: 0,
        };
        fixture.frame(0.0);
        fixture
    }

    fn frame(&mut self, dt: f64) {
        let result = self.host.frame(dt).unwrap();
        assert!(
            result.worlds.values().all(Result::is_ok),
            "{:?}",
            result.worlds
        );
        assert!(
            result.publication_errors.is_empty(),
            "{:?}",
            result.publication_errors
        );
    }

    fn apply(&mut self, operations: Vec<Command>) {
        apply(&mut self.host, self.world, operations)
            .result
            .unwrap();
    }

    /// Skin the control with a new theme entity.
    fn theme(&mut self, theme: GuiTheme, motion: Option<GuiThemeMotion>) -> EntityId {
        let mut components = vec![ComponentValue::GuiTheme(theme)];
        components.extend(motion.map(ComponentValue::GuiThemeMotion));
        let entity = create(&mut self.host, self.world, components, None);
        self.apply(vec![Command::insert_value(
            EntityRef::Handle(self.control),
            ComponentValue::GuiSkin(GuiSkin {
                theme: entity,
                ..Default::default()
            }),
        )]);
        entity
    }

    /// Route one pointer feedback update to the control; the next frame applies it.
    fn pointer(&mut self, update: GuiInteractionUpdate) {
        self.part_pointer(update, GuiInteractionPart::Control);
    }

    /// Route one pointer feedback update naming a part of the control.
    fn part_pointer(&mut self, update: GuiInteractionUpdate, part: GuiInteractionPart) {
        if self.context.is_none() {
            self.host
                .set_root_output(
                    self.output,
                    WorldViewport {
                        width: 200,
                        height: 100,
                        device_pixel_ratio: 1.0,
                    },
                )
                .unwrap();
            self.frame(0.0);
            self.context = Some(
                self.input
                    .bind_context(&self.host, &self.session, self.output.world())
                    .unwrap()
                    .context,
            );
        }
        self.request += 1;
        let target = read_control(&mut self.host, self.world, self.control)
            .unwrap()
            .target;
        let input = self
            .input
            .reserve_routed(
                &self.host,
                self.context.as_ref().unwrap(),
                target,
                self.request,
                &[],
                Box::new(Applied),
            )
            .unwrap();
        let lease = self
            .lease
            .get_or_insert_with(|| self.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::part_interaction(input, lease, update, part).unwrap();
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }

    /// Queue one `GuiAction` command; the next frame applies it.
    fn action(&mut self, action: GuiLocalAction) {
        self.request += 1;
        let target = read_control(&mut self.host, self.world, self.control)
            .unwrap()
            .target;
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 1 << 32 | self.request,
                operations: vec![Command::GuiAction {
                    target: GuiActionTarget {
                        entity: EntityRef::Handle(target.entity),
                        component: target.component,
                        incarnation: target.incarnation,
                    },
                    action,
                }],
            })
            .unwrap();
    }

    fn enabled(&mut self, enabled: bool) {
        self.apply(vec![Command::SetField {
            entity: EntityRef::Handle(self.control),
            component: ComponentValue::GUI_BEHAVIOR,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiBehavior, enabled) as u32,
                value: FieldValue::Bool(enabled),
            },
        }]);
    }

    fn reduced_motion(&mut self, reduced: bool) {
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue_gui_preferences_update(GuiPreferencesUpdate {
                reduced_motion: Some(reduced),
            })
            .unwrap();
    }

    fn publication(&self) -> CanvasPublication {
        self.host
            .publication(self.host.latest_publication(self.world).unwrap())
            .unwrap()
            .output(self.output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone()
    }

    /// The box parts `entity` paints.
    fn boxes_of(&self, entity: EntityId) -> BTreeMap<CanvasPart, Painted> {
        self.publication()
            .entries
            .iter()
            .filter_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive:
                        CanvasPrimitive::Box {
                            style,
                            border_width,
                            border_color,
                            fill,
                            glow,
                            ..
                        },
                    ..
                } if style.identity.target.entity == entity => Some((
                    style.identity.part,
                    Painted {
                        position: style.position,
                        opacity: style.opacity,
                        fill: match fill {
                            CanvasShapeFill::Solid(color) => *color,
                            // A colour field's colours come from the value,
                            // never from a sampled colour.
                            CanvasShapeFill::Hue {
                                ..
                            }
                            | CanvasShapeFill::SaturationValue {
                                ..
                            } => [0.0; 4],
                            _ => [f32::NAN; 4],
                        },
                        border_color: *border_color,
                        border_width: *border_width,
                        glow: glow.map_or(0.0, |glow| glow.intensity),
                    },
                )),
                _ => None,
            })
            .collect()
    }

    fn part(&self, part: CanvasPart) -> Painted {
        *self
            .boxes_of(self.control)
            .get(&part)
            .unwrap_or_else(|| panic!("{part:?} not painted"))
    }

    fn background(&self) -> Painted {
        self.part(CanvasPart::Background)
    }

    /// Parts of the control with live transition channels.
    fn transitions(&mut self) -> usize {
        self.host
            .world_mut(self.world)
            .unwrap()
            .inspect(self.control)
            .unwrap()
            .components
            .iter()
            .find_map(|component| match component {
                ComponentValue::GuiBehavior(behavior) => Some(behavior.motion.transitions()),
                _ => None,
            })
            .unwrap()
    }

    fn work(&mut self) -> (usize, usize, usize) {
        let (preparation, sampling) = self
            .host
            .world_mut(self.world)
            .unwrap()
            .gui_motion_work()
            .unwrap();
        assert_eq!(sampling.owners, sampling.samples);
        (preparation.snapshots, sampling.bindings, sampling.samples)
    }
}

/// A transition from `from` toward `to` over `seconds` with `easing`, sampled
/// at each elapsed time against the eased interpolation of the settled ends.
fn assert_samples(
    fixture: &mut Fixture,
    part: CanvasPart,
    from: Painted,
    steps: &[f64],
    seconds: f64,
    easing: fn(f64) -> f64,
    settle: impl FnOnce(&mut Fixture),
) -> Painted {
    let mut samples = Vec::new();
    let mut elapsed = 0.0;
    for &step in steps {
        fixture.frame(step);
        elapsed += step;
        samples.push((elapsed, fixture.part(part)));
    }
    settle(fixture);
    fixture.frame(1.0);
    let to = fixture.part(part);
    assert_ne!(from, to, "the transition must change {part:?}");
    for (elapsed, sample) in samples {
        let expected = from.lerp(&to, easing((elapsed / seconds).min(1.0)));
        sample.assert_near(
            &expected,
            &format!("{part:?} at {elapsed} s of {seconds} s"),
        );
    }
    to
}

fn linear(t: f64) -> f64 {
    t
}

#[test]
fn default_hover_fades_in_over_80_ms_and_out_over_120_ms() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let idle = fixture.background();

    fixture.pointer(GuiInteractionUpdate::Hover(true));
    let hovered = assert_samples(
        &mut fixture,
        CanvasPart::Background,
        idle,
        &[0.02, 0.02, 0.02],
        0.08,
        linear,
        |fixture| fixture.frame(0.04),
    );
    // The hovered look lights the line and its half-strength glow.
    assert!(hovered.border_width > idle.border_width && hovered.glow > idle.glow);
    assert_eq!(fixture.transitions(), 0);

    fixture.pointer(GuiInteractionUpdate::Hover(false));
    let back = assert_samples(
        &mut fixture,
        CanvasPart::Background,
        hovered,
        &[0.03, 0.03, 0.03],
        0.12,
        linear,
        |fixture| fixture.frame(0.03),
    );
    back.assert_near(&idle, "hover out returns to idle");
}

#[test]
fn a_press_is_immediate_and_its_release_fades_over_100_ms() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    let hovered = fixture.background();

    // The press lands in the frame that applies it, whatever its Host time.
    fixture.pointer(GuiInteractionUpdate::Press);
    fixture.frame(0.001);
    let pressed = fixture.background();
    assert_ne!(pressed.fill, hovered.fill);
    assert_eq!(fixture.transitions(), 0);
    fixture.frame(0.5);
    fixture
        .background()
        .assert_near(&pressed, "press settles at once");

    fixture.pointer(GuiInteractionUpdate::Release);
    let released = assert_samples(
        &mut fixture,
        CanvasPart::Background,
        pressed,
        &[0.025, 0.025, 0.025],
        0.1,
        linear,
        |fixture| fixture.frame(0.025),
    );
    released.assert_near(&hovered, "release returns to hovered");

    // Cancelling a press away from the control releases toward idle over the
    // same duration.
    fixture.pointer(GuiInteractionUpdate::Press);
    fixture.frame(0.0);
    fixture.pointer(GuiInteractionUpdate::Cancel);
    assert_samples(
        &mut fixture,
        CanvasPart::Background,
        pressed,
        &[0.05],
        0.1,
        linear,
        |fixture| fixture.frame(0.05),
    );
}

#[test]
fn a_checkbox_fills_over_100_ms_and_its_mark_appears_and_disappears_at_once() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let unchecked = fixture.background();
    assert!(
        !fixture
            .boxes_of(fixture.control)
            .contains_key(&CanvasPart::Icon)
    );

    // A toggle that lands without a press, as Space on a focused box does:
    // the mark is whole in the first frame while the fill fades in.
    fixture.action(GuiLocalAction::Toggle);
    let checked = assert_samples(
        &mut fixture,
        CanvasPart::Background,
        unchecked,
        &[0.0, 0.05, 0.025],
        0.1,
        linear,
        |fixture| {
            let mark = fixture.part(CanvasPart::Icon);
            assert_eq!(mark.opacity, 1.0);
            fixture.frame(0.025);
            fixture
                .part(CanvasPart::Icon)
                .assert_near(&mark, "the mark never fades");
        },
    );

    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.05);
    assert!(
        !fixture
            .boxes_of(fixture.control)
            .contains_key(&CanvasPart::Icon)
    );
    fixture
        .background()
        .assert_near(&checked.lerp(&unchecked, 0.5), "unchecking fades the fill");
}

/// The built-in switch look as a client theme, from its exported rows.
fn switch_theme() -> (GuiTheme, GuiThemeMotion) {
    let look = gui_skin_looks()
        .iter()
        .find(|look| look.name == "switch")
        .unwrap();
    let mut parts = Rows::default();
    for row in &look.parts {
        parts.push(row.clone()).unwrap();
    }
    let mut motion = Rows::default();
    for row in &look.motion {
        motion.push(row.clone()).unwrap();
    }
    (
        GuiTheme {
            parts,
            em: look.em,
        },
        GuiThemeMotion {
            parts: motion,
        },
    )
}

#[test]
fn the_switch_travels_in_160_ms_with_an_ease_out_cubic_and_reverses_from_where_it_is() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let (theme, motion) = switch_theme();
    fixture.theme(theme, Some(motion));
    let off = fixture.part(CanvasPart::Icon);

    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.04);
    let at_40 = fixture.part(CanvasPart::Icon);
    fixture.frame(0.04);
    let at_80 = fixture.part(CanvasPart::Icon);
    fixture.frame(1.0);
    let on = fixture.part(CanvasPart::Icon);
    assert!(on.position[0] > off.position[0] + 1.0, "{off:?} {on:?}");
    assert_ne!(on.fill, off.fill);

    // The sheet: 58% of the travel at 40 ms and 88% at 80 ms, the colour with it.
    at_40.assert_near(&off.lerp(&on, ease_out_cubic(0.25)), "40 ms");
    at_80.assert_near(&off.lerp(&on, ease_out_cubic(0.5)), "80 ms");
    let travel = |painted: &Painted| {
        (painted.position[0] - off.position[0]) / (on.position[0] - off.position[0])
    };
    assert!((travel(&at_40) - 0.578).abs() < 0.001 && (travel(&at_80) - 0.875).abs() < 0.001);

    // Back off, and reversed 80 ms in: the value commits at once, the block
    // turns from where it is over the full duration.
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.08);
    let turning = fixture.part(CanvasPart::Icon);
    turning.assert_near(&on.lerp(&off, ease_out_cubic(0.5)), "on to off at 80 ms");
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.0);
    assert_eq!(
        read_control(&mut fixture.host, fixture.world, fixture.control)
            .unwrap()
            .value,
        ControlValue::Bool(true)
    );
    fixture.part(CanvasPart::Icon).assert_near(
        &turning,
        "reversal starts where the block is, without a snap",
    );
    fixture.frame(0.04);
    fixture.part(CanvasPart::Icon).assert_near(
        &turning.lerp(&on, ease_out_cubic(0.25)),
        "reversal at 40 ms",
    );
    fixture.frame(0.12);
    fixture
        .part(CanvasPart::Icon)
        .assert_near(&on, "reversal ends on");
}

#[test]
fn focus_disable_and_slider_drag_are_immediate() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let idle = fixture.background();
    fixture.action(GuiLocalAction::Focus(0));
    fixture.frame(0.001);
    let ring = fixture.part(CanvasPart::FocusRing);
    assert_eq!(ring.opacity, 1.0);
    assert!(ring.glow > 0.0);
    fixture.action(GuiLocalAction::Blur);
    fixture.frame(0.001);
    assert!(
        !fixture
            .boxes_of(fixture.control)
            .contains_key(&CanvasPart::FocusRing)
    );

    // A hovered button lights its line; disabling it draws the line in the
    // disabled colour at once, and enabling returns it to idle at once.
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    let hovered = fixture.background();
    fixture.enabled(false);
    let disabled = fixture.background();
    assert_ne!(disabled, hovered);
    fixture.frame(0.5);
    fixture
        .background()
        .assert_near(&disabled, "disable lands at once");
    fixture.enabled(true);
    fixture
        .background()
        .assert_near(&idle, "enable lands at once");
    assert_eq!(fixture.transitions(), 0);

    // A dragged thumb follows the value in the frame that applies it.
    let mut slider = Fixture::new(ComponentValue::GuiSlider(GuiSlider::default()));
    let before = slider.part(CanvasPart::Icon);
    slider.action(GuiLocalAction::SetScalar(0.75));
    slider.frame(0.001);
    let after = slider.part(CanvasPart::Icon);
    assert!(after.position[0] > before.position[0]);
    slider.frame(0.5);
    slider
        .part(CanvasPart::Icon)
        .assert_near(&after, "no tween");
}

#[test]
fn a_range_thumb_fades_its_own_hover_while_the_other_thumb_stays_idle() {
    let mut fixture = Fixture::new(ComponentValue::GuiSlider(GuiSlider {
        value: 0.25,
        upper: 0.75,
        range: true,
        ..Default::default()
    }));
    let lower = fixture.part(CanvasPart::Icon);
    let upper = fixture.part(CanvasPart::PartIcon(1));

    // Hovering the upper thumb fades its lit edge in over 80 ms; the lower
    // thumb keeps its idle look throughout.
    fixture.part_pointer(
        GuiInteractionUpdate::Hover(true),
        GuiInteractionPart::FocusPart(1),
    );
    let hovered = assert_samples(
        &mut fixture,
        CanvasPart::PartIcon(1),
        upper,
        &[0.02, 0.02, 0.02],
        0.08,
        linear,
        |fixture| fixture.frame(0.04),
    );
    assert!(hovered.glow > upper.glow);
    fixture
        .part(CanvasPart::Icon)
        .assert_near(&lower, "the other thumb stays idle");

    // Moving to the lower thumb fades the upper one out over 120 ms and the
    // lower one in.
    fixture.part_pointer(
        GuiInteractionUpdate::Hover(true),
        GuiInteractionPart::FocusPart(0),
    );
    fixture.frame(0.06);
    let halfway = upper.lerp(&hovered, 0.5);
    fixture
        .part(CanvasPart::PartIcon(1))
        .assert_near(&halfway, "the upper thumb halfway out");
    fixture.frame(0.1);
    fixture
        .part(CanvasPart::PartIcon(1))
        .assert_near(&upper, "the upper thumb back at idle");
    let lit = fixture.part(CanvasPart::Icon);
    assert_eq!(
        (lit.border_color, lit.border_width, lit.glow),
        (hovered.border_color, hovered.border_width, hovered.glow),
        "the lower thumb lit"
    );

    // The next preparation releases the settled channels.
    fixture.frame(0.0);
    assert_eq!(fixture.transitions(), 0);
}

#[test]
fn a_step_part_fades_its_own_hover_while_the_field_and_the_other_part_stay_idle() {
    let mut fixture = Fixture::new(ComponentValue::GuiTextInput(GuiTextInput {
        numeric: true,
        value: 1.0,
        min: 0.0,
        max: 2.0,
        step_parts: true,
        ..Default::default()
    }));
    let increment = fixture.part(CanvasPart::Increment);
    let decrement = fixture.part(CanvasPart::Decrement);
    let field = fixture.background();

    // Hovering the increment part fades its lit edge in over 80 ms; the
    // decrement part and the field keep their idle look throughout.
    fixture.part_pointer(
        GuiInteractionUpdate::Hover(true),
        GuiInteractionPart::Step(GuiNumberStep::Increment),
    );
    let hovered = assert_samples(
        &mut fixture,
        CanvasPart::Increment,
        increment,
        &[0.02, 0.02, 0.02],
        0.08,
        linear,
        |fixture| fixture.frame(0.04),
    );
    assert!(hovered.glow > increment.glow);
    fixture
        .part(CanvasPart::Decrement)
        .assert_near(&decrement, "the other part stays idle");
    fixture
        .background()
        .assert_near(&field, "the field stays idle");

    // Reaching the bound disables the increment part at once.
    fixture.action(GuiLocalAction::SetScalar(2.0));
    fixture.frame(0.001);
    let disabled = fixture.part(CanvasPart::IncrementMark);
    fixture.frame(0.5);
    fixture
        .part(CanvasPart::IncrementMark)
        .assert_near(&disabled, "disable lands at once");
    fixture.frame(0.0);
    assert_eq!(fixture.transitions(), 0);
}

#[test]
fn each_colour_surface_moves_on_its_own_part_channel() {
    let mut fixture = Fixture::new(ComponentValue::GuiColor(GuiColor {
        alpha_rail: true,
        ..Default::default()
    }));
    let field = fixture.part(CanvasPart::Track);
    let rail = fixture.part(CanvasPart::PartTrack(1));

    // Hovering the hue rail fades its lit edge in over 80 ms; the field and
    // the other rail keep their idle look throughout.
    fixture.part_pointer(
        GuiInteractionUpdate::Hover(true),
        GuiInteractionPart::FocusPart(1),
    );
    let hovered = assert_samples(
        &mut fixture,
        CanvasPart::PartTrack(1),
        rail,
        &[0.02, 0.02, 0.02],
        0.08,
        linear,
        |fixture| fixture.frame(0.04),
    );
    assert!(hovered.glow > rail.glow);
    fixture
        .part(CanvasPart::Track)
        .assert_near(&field, "the field stays idle");

    // Moving to the field fades the rail out over 120 ms and the field in;
    // the marker and thumbs keep their outline throughout.
    let marker = fixture.part(CanvasPart::Marker);
    let thumb = fixture.part(CanvasPart::PartIcon(1));
    fixture.part_pointer(
        GuiInteractionUpdate::Hover(true),
        GuiInteractionPart::FocusPart(0),
    );
    fixture.frame(0.06);
    fixture
        .part(CanvasPart::PartTrack(1))
        .assert_near(&rail.lerp(&hovered, 0.5), "the rail halfway out");
    fixture.frame(0.1);
    fixture
        .part(CanvasPart::PartTrack(1))
        .assert_near(&rail, "the rail back at idle");
    let lit = fixture.part(CanvasPart::Track);
    assert_eq!(
        (lit.border_color, lit.glow),
        (hovered.border_color, hovered.glow),
        "the field lit"
    );
    fixture
        .part(CanvasPart::Marker)
        .assert_near(&marker, "the marker unchanged");
    fixture
        .part(CanvasPart::PartIcon(1))
        .assert_near(&thumb, "the thumb unchanged");
    fixture.frame(0.0);
    assert_eq!(fixture.transitions(), 0);
}

#[test]
fn reversing_a_hover_continues_from_the_current_composite() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let idle = fixture.background();
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    let hovered = fixture.background();
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    fixture.frame(1.0);

    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(0.06);
    let partway = fixture.background();
    partway.assert_near(&idle.lerp(&hovered, 0.75), "hover in at 60 ms");

    // Hover out from three quarters of the way: no jump, then the hover-out
    // duration from there.
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    fixture.frame(0.0);
    fixture.background().assert_near(&partway, "no jump");
    fixture.frame(0.06);
    fixture
        .background()
        .assert_near(&partway.lerp(&idle, 0.5), "hover out at 60 ms");
    fixture.frame(0.06);
    fixture.background().assert_near(&idle, "settled");
}

#[test]
fn reduced_motion_snaps_every_transition_including_one_under_way() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let idle = fixture.background();
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    let hovered = fixture.background();
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    fixture.frame(0.03);
    assert_ne!(fixture.background(), idle);

    fixture.reduced_motion(true);
    fixture.frame(0.0);
    fixture
        .background()
        .assert_near(&idle, "the transition under way snaps");
    assert_eq!(fixture.transitions(), 0);
    assert!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .gui_preferences()
            .unwrap()
            .reduced_motion
    );

    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(0.0);
    fixture
        .background()
        .assert_near(&hovered, "a new transition snaps");
    assert_eq!(fixture.work().1, 0);

    fixture.reduced_motion(false);
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    fixture.frame(0.06);
    fixture
        .background()
        .assert_near(&hovered.lerp(&idle, 0.5), "motion resumes");
}

#[test]
fn a_font_change_retargets_a_transition_under_way_and_reaches_settled_controls_without_work() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let font = |size: f32| {
        ComponentValue::GuiFont(GuiFont {
            font_size: size,
            ..Default::default()
        })
    };
    fixture.apply(vec![Command::insert_value(
        EntityRef::Handle(fixture.canvas),
        font(16.0),
    )]);
    let settled = create(
        &mut fixture.host,
        fixture.world,
        vec![ComponentValue::GuiButton(GuiButton::default()), layout()],
        Some(fixture.canvas),
    );
    fixture.frame(0.0);
    let idle = fixture.background();
    let settled_idle = fixture.boxes_of(settled)[&CanvasPart::Background];

    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(0.04);
    fixture.apply(vec![Command::insert_value(
        EntityRef::Handle(fixture.canvas),
        font(32.0),
    )]);
    // Only the transitioning control prepared and sampled for the edit.
    assert_eq!(fixture.work(), (1, 0, 1));
    let doubled = fixture.boxes_of(settled)[&CanvasPart::Background];
    assert!((doubled.border_width - 2.0 * settled_idle.border_width).abs() < 1.0e-4);

    // The transition keeps its clock and reaches the hovered look at the new size.
    fixture.frame(0.04);
    let hovered = fixture.background();
    fixture.frame(1.0);
    fixture.background().assert_near(&hovered, "ended on time");
    assert!(hovered.border_width > 2.0 * idle.border_width);
    assert_eq!(fixture.work(), (0, 0, 0));
}

#[test]
fn a_world_without_animation_paints_the_static_looks_and_snaps() {
    let systems = select(&[GUI_LAYOUT]);
    let mut fixture =
        Fixture::with_systems(&systems, ComponentValue::GuiButton(GuiButton::default()));
    let mut animated = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture
        .background()
        .assert_near(&animated.background(), "idle");

    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(0.0);
    animated.pointer(GuiInteractionUpdate::Hover(true));
    animated.frame(1.0);
    fixture
        .background()
        .assert_near(&animated.background(), "hover lands at once");
    assert_eq!(fixture.transitions(), 0);
    assert!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .gui_motion_work()
            .is_none()
    );

    // Theme motion rows need the sampler.
    let theme = create(&mut fixture.host, fixture.world, vec![], None);
    let result = apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(theme),
            ComponentValue::GuiThemeMotion(GuiThemeMotion::default()),
        )],
    );
    assert!(result.result.is_err());
}

#[test]
fn static_and_settled_controls_do_no_per_frame_work() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let mut operations = Vec::new();
    for alias in 1..=1024 {
        operations.push(Command::Create {
            alias,
            metadata: Default::default(),
            adopt: false,
        });
        operations.push(Command::insert_value(
            EntityRef::Alias(alias),
            ComponentValue::GuiButton(GuiButton::default()),
        ));
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(alias),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(fixture.canvas)),
                before: None,
            },
        });
    }
    let created = apply(&mut fixture.host, fixture.world, operations)
        .result
        .unwrap();
    // The applying frame inspected each new control once.
    assert_eq!(fixture.work(), (1024, 0, 0));
    fixture.frame(0.1);
    assert_eq!(fixture.work(), (0, 0, 0));

    // A camera, style or unrelated control edit prepares and samples nothing.
    fixture.apply(vec![Command::insert_value(
        EntityRef::Handle(created[0].1),
        ComponentValue::CanvasStyle(ipp_core::systems::canvas::CanvasStyle {
            opacity: 0.5,
            ..Default::default()
        }),
    )]);
    assert_eq!(fixture.work(), (0, 0, 0));

    // One toggle: one control inspected, its box's fill started and sampled.
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.05);
    assert_eq!(fixture.work(), (1, 1, 1));
    fixture.frame(0.025);
    assert_eq!(fixture.work(), (0, 0, 1));
    fixture.frame(0.025);
    assert_eq!(fixture.work(), (0, 0, 1));
    assert_eq!(fixture.transitions(), 1);
    fixture.frame(0.1);
    assert_eq!(fixture.work(), (0, 0, 0));
    assert_eq!(fixture.transitions(), 0);
}

#[test]
fn a_theme_times_its_own_transitions_over_the_default_look() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPrimitivePart::Background;
    let mut parts = Rows::default();
    for (identity, color) in [
        (GuiPartId::base(background), RED),
        (GuiPartId::state(background, GuiSkinState::Hovered), BLUE),
    ] {
        parts
            .push(GuiPaintPart {
                color: Some(color),
                ..GuiPaintPart::keyed(identity).unwrap()
            })
            .unwrap();
    }
    let mut motion = Rows::default();
    motion
        .push(GuiMotionPart {
            duration: Some(1.0),
            easing: Some(1),
            exit: Some(0.5),
            ..GuiMotionPart::keyed(GuiPartId::state(background, GuiSkinState::Hovered)).unwrap()
        })
        .unwrap();
    fixture.theme(
        GuiTheme {
            parts,
            em: 0.0,
        },
        Some(GuiThemeMotion {
            parts: motion,
        }),
    );
    let idle = fixture.background();
    assert_eq!(idle.fill, RED);

    // Into hovered: the theme's second and smoothstep.
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    let hovered = assert_samples(
        &mut fixture,
        CanvasPart::Background,
        idle,
        &[0.25, 0.25],
        1.0,
        smoothstep,
        |fixture| fixture.frame(0.5),
    );
    assert_eq!(hovered.fill, BLUE);

    // Out of hovered: the theme's exit, with the hovered row's easing.
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    assert_samples(
        &mut fixture,
        CanvasPart::Background,
        hovered,
        &[0.125, 0.125],
        0.5,
        smoothstep,
        |fixture| fixture.frame(0.25),
    );
}

#[test]
fn sampled_colour_reaches_the_solid_fill_and_the_focus_stroke_but_not_implicit_gradient_stops() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let background = GuiPrimitivePart::Background;
    let ring = GuiPrimitivePart::FocusRing;
    let mut parts = Rows::default();
    for row in [
        GuiPaintPart {
            color: Some(RED),
            fill_mode: Some(1.0),
            gradient_color1: Some(BLUE),
            ..GuiPaintPart::keyed(GuiPartId::base(background)).unwrap()
        },
        GuiPaintPart {
            color: Some(BLUE),
            ..GuiPaintPart::keyed(GuiPartId::state(background, GuiSkinState::Hovered)).unwrap()
        },
        GuiPaintPart {
            color: Some(RED),
            ..GuiPaintPart::keyed(GuiPartId::base(ring)).unwrap()
        },
    ] {
        parts.push(row).unwrap();
    }
    let mut motion = Rows::default();
    for identity in [GuiPartId::base(background), GuiPartId::base(ring)] {
        motion
            .push(GuiMotionPart {
                duration: Some(1.0),
                ..GuiMotionPart::keyed(identity).unwrap()
            })
            .unwrap();
    }
    fixture.theme(
        GuiTheme {
            parts,
            em: 0.0,
        },
        Some(GuiThemeMotion {
            parts: motion,
        }),
    );
    fixture.pointer(GuiInteractionUpdate::Hover(true));
    fixture.frame(0.0);
    fixture.pointer(GuiInteractionUpdate::Hover(false));
    fixture.action(GuiLocalAction::Focus(0));
    fixture.frame(0.5);
    let entries = fixture.publication().entries;
    let gradient = entries.iter().find_map(|entry| match entry.as_ref() {
        CanvasPaintEntry::Primitive {
            primitive:
                CanvasPrimitive::Box {
                    style,
                    fill:
                        CanvasShapeFill::LinearGradient {
                            start_color,
                            ..
                        },
                    ..
                },
            ..
        } if style.identity.part == CanvasPart::Background => Some(*start_color),
        _ => None,
    });
    // The gradient's implicit first stop keeps the destination colour.
    assert_eq!(gradient, Some(RED));
    let ring = fixture.part(CanvasPart::FocusRing);
    assert_eq!(ring.border_color, RED);
    assert!(ring.opacity > 0.0);
}

#[test]
fn replacing_the_control_or_its_behavior_mid_transition_drops_the_transition() {
    for replace_behavior in [false, true] {
        let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
        let unchecked = fixture.background();
        fixture.action(GuiLocalAction::Toggle);
        fixture.frame(0.05);
        assert_eq!(fixture.transitions(), 1);
        let replacement = if replace_behavior {
            ComponentValue::GuiBehavior(GuiBehavior::default())
        } else {
            ComponentValue::GuiCheckbox(GuiCheckbox::default())
        };
        fixture.apply(vec![Command::insert_value(
            EntityRef::Handle(fixture.control),
            replacement,
        )]);
        assert_eq!(fixture.transitions(), 0);
        fixture.frame(0.025);
        assert_eq!(fixture.work().2, 0);
        if replace_behavior {
            assert_ne!(fixture.background(), unchecked);
        } else {
            fixture
                .background()
                .assert_near(&unchecked, "the replacement starts unchecked");
        }
    }
}

#[test]
fn deleting_a_transitioning_control_frees_its_slot_for_a_static_control() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.05);
    let old = fixture.control;
    fixture.apply(vec![Command::Delete {
        entity: EntityRef::Handle(old),
    }]);
    let reused = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            layout(),
        ],
        Some(fixture.canvas),
    );
    assert_eq!(reused.index(), old.index());
    fixture.control = reused;
    fixture.frame(0.025);
    assert_eq!(fixture.transitions(), 0);
    assert_eq!(fixture.work().2, 0);
}

#[test]
fn graph_restore_saves_no_transition_and_paints_the_destination() {
    use ipp_core::services::world_serialization::WorldLoadOptions;

    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.05);
    assert_eq!(fixture.transitions(), 1);
    let saved = fixture
        .host
        .save_world(fixture.world, 72, Default::default())
        .unwrap();
    fixture.frame(1.0);
    let checked = fixture.background();
    let restored = fixture
        .host
        .load_world(
            &saved,
            72,
            WorldLoadOptions {
                symbolic_id: Some("motion-copy".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let world = restored.root.id();
    let mut controls = Vec::new();
    for entity in fixture.host.world_mut(world).unwrap().entities() {
        for value in &entity.components {
            if let ComponentValue::GuiBehavior(behavior) = value {
                assert_eq!(behavior.motion.transitions(), 0);
                controls.push(entity.id);
            }
        }
    }
    assert_eq!(controls.len(), 1);
    let control = read_control(&mut fixture.host, world, controls[0]).unwrap();
    assert_eq!(control.value, ControlValue::Bool(true));

    let world_ref = fixture.host.world_ref(world).unwrap();
    let output = fixture.host.canvas_output(world_ref, [200.0, 100.0], 1.0);
    fixture.frame(0.0);
    let restored = Fixture {
        world,
        control: controls[0],
        output,
        ..fixture
    };
    restored
        .background()
        .assert_near(&checked, "the restored control paints its destination");
}

#[test]
fn reduced_motion_is_saved_with_the_world_like_other_system_state() {
    use ipp_core::services::world_serialization::WorldLoadOptions;

    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let restore = |fixture: &mut Fixture, name: &str| {
        let saved = fixture
            .host
            .save_world(fixture.world, 72, Default::default())
            .unwrap();
        let restored = fixture
            .host
            .load_world(
                &saved,
                72,
                WorldLoadOptions {
                    symbolic_id: Some(name.into()),
                    ..Default::default()
                },
                Default::default(),
                Default::default(),
            )
            .unwrap();
        fixture
            .host
            .world_mut(restored.root.id())
            .unwrap()
            .gui_preferences()
            .unwrap()
    };
    assert!(!restore(&mut fixture, "default-copy").reduced_motion);
    fixture.reduced_motion(true);
    fixture.frame(0.0);
    assert!(restore(&mut fixture, "reduced-copy").reduced_motion);
}
