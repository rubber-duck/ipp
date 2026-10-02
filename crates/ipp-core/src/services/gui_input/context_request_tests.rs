//! Context requests: a secondary press on a control, and the Menu key or
//! Shift+F10 on a focused one, focus the target and publish a momentary
//! context effect at a canvas point. Nothing opens in the runtime.

use super::router_test_support::*;
use super::*;
use crate::components::{GuiBehavior, GuiCheckbox};
use crate::services::gui_input::router::*;
use crate::systems::gui::local::{GuiLocalActionError, GuiLocalEffectKind};

use GuiPhysicalKey::{ContextMenu, Escape, F10, Tab};

/// A column of a button, a checkbox and a disabled button, each `4 x 2`, in a
/// `10 x 10` root canvas presented at ten pixels per unit.
struct Controls {
    rig: Rig,
    world: WorldRef,
    button: EntityId,
    checkbox: EntityId,
    disabled: EntityId,
}

impl Controls {
    fn new() -> Self {
        let (mut host, _) = host();
        let world = world(&mut host);
        let (root, root_entity) = canvas_root(&mut host, world, 10.0, 10.0);
        let mut control = |value: ComponentValue, enabled: bool| {
            create(
                &mut host,
                world,
                vec![
                    value,
                    sized(0, 4.0, 2.0),
                    ComponentValue::GuiBehavior(GuiBehavior {
                        enabled,
                        ..Default::default()
                    }),
                ],
                Some(root_entity),
            )
        };
        let button = control(ComponentValue::GuiButton(GuiButton::default()), true);
        let checkbox = control(ComponentValue::GuiCheckbox(GuiCheckbox::default()), true);
        let disabled = control(ComponentValue::GuiButton(GuiButton::default()), false);
        Self {
            rig: Rig::new(host, root, viewport(100, 100)),
            world,
            button,
            checkbox,
            disabled,
        }
    }

    fn target(&mut self, entity: EntityId) -> GuiEntityTarget {
        self.rig.snapshot(self.world, entity).target
    }

    /// Context effects routed so far: their control and canvas point.
    fn contexts(&self) -> Vec<(EntityId, [f32; 2])> {
        terminals(&self.rig.ledger)
            .into_iter()
            .filter_map(|terminal| match terminal {
                GuiDeliveryTerminal::Applied(GuiLocalEffect {
                    target,
                    source:
                        GuiLocalEffectSource::Routed {
                            ..
                        },
                    kind:
                        GuiLocalEffectKind::ContextRequested {
                            point,
                        },
                    ..
                }) => Some((target.entity, point)),
                _ => None,
            })
            .collect()
    }

    /// Whether a press effect was routed.
    fn pressed(&self) -> bool {
        terminals(&self.rig.ledger).iter().any(|terminal| {
            matches!(
                terminal,
                GuiDeliveryTerminal::Applied(GuiLocalEffect {
                    kind: GuiLocalEffectKind::Pressed,
                    ..
                })
            )
        })
    }

    /// The World's focus and whether its ring shows.
    fn focus(&mut self) -> Option<(EntityId, bool)> {
        self.rig
            .host
            .world_mut(self.world.id())
            .unwrap()
            .gui_focus_page(0, 0, 1)
            .first()
            .map(|record| (record.target.entity, record.visible))
    }

    fn finish(self) {
        self.rig.finish();
    }
}

fn secondary(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerDown {
        button: GuiPhysicalButton::Secondary,
        pointer,
        point,
    }
}

fn secondary_release(pointer: u64, point: [f32; 2]) -> GuiPhysicalInput {
    GuiPhysicalInput::PointerUp {
        button: GuiPhysicalButton::Secondary,
        pointer,
        point,
    }
}

fn near(left: [f32; 2], right: [f32; 2]) -> bool {
    (0..2).all(|axis| (left[axis] - right[axis]).abs() < 1e-4)
}

#[test]
fn secondary_press_focuses_the_control_and_requests_its_context_at_the_press_point() {
    let mut controls = Controls::new();
    let checkbox = controls.target(controls.checkbox);
    let bounds = controls.rig.bounds(controls.world, controls.checkbox);
    let point = controls.rig.point_in(controls.checkbox, [0.25, 0.5]);
    assert_eq!(
        controls.rig.send(secondary(1, point)),
        GuiRoutingDisposition::Routed {
            target: checkbox,
        }
    );
    let contexts = controls.contexts();
    assert_eq!(contexts.len(), 1, "{contexts:?}");
    assert_eq!(contexts[0].0, controls.checkbox);
    let expected = [bounds[0] + bounds[2] * 0.25, bounds[1] + bounds[3] * 0.5];
    assert!(
        near(contexts[0].1, expected),
        "{contexts:?} != {expected:?}"
    );

    // It focuses as a pointer press does, without the ring, and neither
    // toggles nor presses; nothing else is requested by the release.
    assert_eq!(controls.focus(), Some((controls.checkbox, false)));
    assert_eq!(
        controls.rig.value(controls.world, controls.checkbox),
        GuiTestValue::Bool(false)
    );
    assert_eq!(
        controls.rig.send(secondary_release(1, point)),
        GuiRoutingDisposition::Blocked
    );
    assert_eq!(controls.contexts().len(), 1);
    assert_eq!(
        controls
            .rig
            .snapshot(controls.world, controls.checkbox)
            .interaction,
        Default::default()
    );
    controls.finish();
}

#[test]
fn a_secondary_press_on_a_disabled_control_requests_nothing_and_keeps_focus() {
    let mut controls = Controls::new();
    controls.rig.send(key(Tab));
    assert_eq!(controls.focus(), Some((controls.button, true)));
    let point = controls.rig.point_in(controls.disabled, [0.5, 0.5]);
    let disposition = controls.rig.send(secondary(1, point));
    assert!(
        !matches!(disposition, GuiRoutingDisposition::Routed { .. }),
        "{disposition:?}"
    );
    assert!(controls.contexts().is_empty());
    assert_eq!(controls.focus(), Some((controls.button, true)));
    controls.finish();
}

#[test]
fn menu_key_and_shift_f10_request_the_focused_controls_context_at_its_corner() {
    let mut controls = Controls::new();
    let button = controls.target(controls.button);

    // Without focus there is no target.
    assert_eq!(
        controls.rig.route(key(ContextMenu)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    controls.rig.send(key(Tab));
    let bounds = controls.rig.bounds(controls.world, controls.button);
    let corner = [bounds[0], bounds[1] + bounds[3]];
    for input in [key(ContextMenu), shifted(F10)] {
        assert_eq!(
            controls.rig.send(input),
            GuiRoutingDisposition::Routed {
                target: button,
            }
        );
    }

    // F10 alone is not a context request: Shift reaches key handling.
    assert_eq!(
        controls.rig.route(key(F10)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    controls.rig.frame();
    let contexts = controls.contexts();
    assert_eq!(contexts.len(), 2, "{contexts:?}");
    for (entity, point) in contexts {
        assert_eq!(entity, controls.button);
        assert!(near(point, corner), "{point:?} != {corner:?}");
    }
    assert!(!controls.pressed());
    assert_eq!(controls.focus(), Some((controls.button, true)));

    controls.rig.send(key(Escape));
    assert_eq!(
        controls.rig.route(shifted(F10)),
        Ok(GuiRoutingDisposition::Unhandled)
    );
    controls.rig.frame();
    assert_eq!(controls.contexts().len(), 2);
    controls.finish();
}

#[test]
fn a_context_request_on_a_control_that_takes_no_focus_leaves_focus_where_it_is() {
    let mut controls = Controls::new();
    apply(
        &mut controls.rig.host,
        controls.world,
        vec![Command::insert_value(
            EntityRef::Handle(controls.checkbox),
            ComponentValue::GuiBehavior(GuiBehavior {
                focusable: false,
                ..Default::default()
            }),
        )],
    );
    controls.rig.frame();
    controls.rig.send(key(Tab));
    assert_eq!(controls.focus(), Some((controls.button, true)));
    let checkbox = controls.target(controls.checkbox);
    let point = controls.rig.point_in(controls.checkbox, [0.5, 0.5]);
    assert_eq!(
        controls.rig.send(secondary(1, point)),
        GuiRoutingDisposition::Routed {
            target: checkbox,
        }
    );
    let contexts = controls.contexts();
    assert_eq!(contexts.len(), 1, "{contexts:?}");
    assert_eq!(contexts[0].0, controls.checkbox);
    assert_eq!(controls.focus(), Some((controls.button, true)));
    controls.finish();
}

#[test]
fn shift_tab_traverses_backwards_like_back_tab() {
    let mut controls = Controls::new();
    controls.rig.send(key(Tab));
    controls.rig.send(key(Tab));
    assert_eq!(controls.focus(), Some((controls.checkbox, true)));
    controls.rig.send(shifted(Tab));
    assert_eq!(controls.focus(), Some((controls.button, true)));
    controls.finish();
}

#[test]
fn a_context_request_for_a_replaced_control_or_a_released_context_is_not_published() {
    let mut controls = Controls::new();
    let point = controls.rig.point_in(controls.checkbox, [0.5, 0.5]);

    // The control is replaced before the routed request reaches it.
    queue_edits(
        &mut controls.rig.host,
        controls.world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(controls.checkbox),
                component: ComponentValue::GUI_CHECKBOX,
            },
            Command::insert_value(
                EntityRef::Handle(controls.checkbox),
                ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ),
        ],
    );
    controls.rig.send(secondary(1, point));
    assert!(controls.contexts().is_empty());
    assert!(
        controls
            .rig
            .rejected()
            .contains(&GuiInputError::Local(GuiLocalActionError::StaleTarget)),
        "{:?}",
        controls.rig.rejected()
    );

    // Releasing the context cancels a request it queued.
    controls.rig.synchronize();
    let point = controls.rig.point_in(controls.button, [0.5, 0.5]);
    controls.rig.route(secondary(1, point)).unwrap();
    controls.rig.rebind();
    controls.rig.frame();
    assert!(controls.contexts().is_empty());
    assert!(
        terminals(&controls.rig.ledger).contains(&GuiDeliveryTerminal::Cancelled),
        "{:?}",
        terminals(&controls.rig.ledger)
    );
    controls.finish();
}
