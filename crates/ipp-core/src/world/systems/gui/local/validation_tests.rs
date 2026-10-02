//! Control configuration and value domains: authored components, client
//! writes and semantic actions share one set of value rules, and a refused
//! value has no effect.

use super::local_tests::{
    GuiTestValue, action, apply, create, fixture, frame, outcomes, snapshot, submit,
};
use super::*;
use crate::{
    Command, ComponentValue, EntityRef, ErrorReason, FieldValue, FieldWrite, HostRuntime, WorldId,
};
use std::sync::Arc;

/// Apply one batch the core must refuse, returning its reason.
fn refused(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> ErrorReason {
    submit(host, world, operations)
        .result
        .expect_err("the batch must be refused")
        .reason
}

fn create_value(value: ComponentValue) -> Vec<Command> {
    vec![
        Command::Create {
            alias: 1,
            metadata: Default::default(),
            adopt: false,
        },
        Command::insert_value(EntityRef::Alias(1), value),
    ]
}

fn slider(min: f32, max: f32, step: f32) -> GuiSlider {
    GuiSlider {
        value: min,
        min,
        max,
        step,
        ..Default::default()
    }
}

#[test]
fn slider_actions_outside_the_range_or_non_finite_values_are_invalid_without_effect() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(slider(-1.0, 1.0, 0.0)),
    );
    let initial = snapshot(&mut host, world, entity);
    for value in [1.5, -1.5, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        action(
            &mut host,
            world,
            initial.target,
            GuiLocalAction::SetScalar(value),
        );
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::InvalidValue),
            "{value}"
        );
    }

    // A client write must still be a finite number of the field's type.
    let write = |value| {
        vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::GUI_SLIDER,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSlider, value) as u32,
                value,
            },
        }]
    };
    assert_eq!(
        refused(&mut host, world, write(FieldValue::F32(f32::NAN))),
        ErrorReason::InvalidValue
    );
    assert_eq!(
        refused(&mut host, world, write(FieldValue::Bool(true))),
        ErrorReason::InvalidField
    );
    assert_eq!(initial.value, snapshot(&mut host, world, entity).value);

    // The inclusive bounds are accepted.
    action(
        &mut host,
        world,
        initial.target,
        GuiLocalAction::SetScalar(1.0),
    );
    frame(&mut host);
    assert!(outcomes(&mut host, world)[0].result.is_ok());
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Scalar(1.0)
    );
}

#[test]
fn slider_configuration_rejects_negative_steps_non_finite_values_and_inverted_ranges() {
    let (mut host, world) = fixture();
    for invalid in [
        slider(0.0, 1.0, -0.1),
        slider(0.0, 1.0, f32::NAN),
        slider(f32::NEG_INFINITY, 1.0, 0.0),
        slider(0.0, f32::NAN, 0.0),
        slider(2.0, 1.0, 0.0),
        GuiSlider {
            value: f32::INFINITY,
            ..slider(0.0, 1.0, 0.0)
        },
        GuiSlider {
            fine_step: -0.01,
            ..slider(0.0, 1.0, 0.1)
        },
        GuiSlider {
            fine_step: f32::INFINITY,
            ..slider(0.0, 1.0, 0.1)
        },
        GuiSlider {
            origin: f32::NAN,
            ..slider(0.0, 1.0, 0.0)
        },
        GuiSlider {
            origin: f32::NEG_INFINITY,
            ..slider(0.0, 1.0, 0.0)
        },
        GuiSlider {
            axis: 3,
            ..slider(0.0, 1.0, 0.0)
        },
        GuiSlider {
            upper: f32::NAN,
            ..slider(0.0, 1.0, 0.0)
        },
        // A range's values never invert, and a dial holds one value.
        GuiSlider {
            value: 0.8,
            upper: 0.2,
            range: true,
            ..slider(0.0, 1.0, 0.0)
        },
        GuiSlider {
            value: 0.2,
            upper: 0.8,
            range: true,
            axis: 2,
            ..slider(0.0, 1.0, 0.0)
        },
    ] {
        assert_eq!(
            refused(
                &mut host,
                world,
                create_value(ComponentValue::GuiSlider(invalid))
            ),
            ErrorReason::InvalidValue,
            "{invalid:?}"
        );
    }

    // The value is a plain field: a finite value outside the range is stored
    // as written, and only semantic value changes are range checked.
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider {
            value: 1.5,
            ..slider(0.0, 1.0, 0.25)
        }),
    );
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Scalar(1.5)
    );
}

#[test]
fn a_colour_holds_finite_channels_in_the_unit_range_however_it_is_written() {
    let (mut host, world) = fixture();
    let colour = |hue, saturation, value, alpha| GuiColor {
        hue,
        saturation,
        value,
        alpha,
        alpha_rail: true,
    };
    for invalid in [
        colour(1.5, 0.5, 0.5, 1.0),
        colour(0.5, -0.25, 0.5, 1.0),
        colour(0.5, 0.5, f32::NAN, 1.0),
        colour(0.5, 0.5, 0.5, f32::INFINITY),
    ] {
        assert_eq!(
            refused(
                &mut host,
                world,
                create_value(ComponentValue::GuiColor(invalid))
            ),
            ErrorReason::InvalidValue,
            "{invalid:?}"
        );
    }

    // A client's field write outside the range is refused like a semantic
    // colour, and leaves the colour as it was; the bounds are accepted.
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiColor(colour(0.0, 1.0, 1.0, 1.0)),
    );
    let write = |offset: usize, value: f32| Command::SetField {
        entity: EntityRef::Handle(entity),
        component: ComponentValue::GUI_COLOR,
        field: FieldWrite {
            offset: offset as u32,
            value: FieldValue::F32(value),
        },
    };
    let hue = std::mem::offset_of!(GuiColor, hue);
    let alpha = std::mem::offset_of!(GuiColor, alpha);
    assert_eq!(
        refused(&mut host, world, vec![write(hue, 1.01)]),
        ErrorReason::InvalidValue
    );
    apply(&mut host, world, vec![write(hue, 1.0), write(alpha, 0.0)]);
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Color([1.0, 1.0, 1.0, 0.0])
    );
}

#[test]
fn a_range_refuses_values_that_invert_it_and_focus_on_thumbs_it_lacks() {
    let (mut host, world) = fixture();
    let range = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(GuiSlider {
            value: 0.2,
            upper: 0.6,
            range: true,
            ..slider(0.0, 1.0, 0.0)
        }),
    );
    let single = create(
        &mut host,
        world,
        ComponentValue::GuiSlider(slider(0.0, 1.0, 0.0)),
    );
    let [range, single] = [range, single].map(|entity| snapshot(&mut host, world, entity).target);

    // The semantic value is the lower one: above the upper value it is
    // refused, and up to it accepted.
    for (action_value, result) in [
        (
            GuiLocalAction::SetScalar(0.7),
            Err(ErrorReason::InvalidValue),
        ),
        (GuiLocalAction::SetScalar(0.6), Ok(())),
        (GuiLocalAction::Focus(1), Ok(())),
        (
            GuiLocalAction::Focus(2),
            Err(ErrorReason::UnsupportedAction),
        ),
    ] {
        action(&mut host, world, range, action_value.clone());
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result.clone().map(|_| ()),
            result,
            "{action_value:?}"
        );
    }
    assert_eq!(
        snapshot(&mut host, world, range.entity).value,
        GuiTestValue::Range([0.6, 0.6])
    );

    // A slider with one value has one focus part.
    action(&mut host, world, single, GuiLocalAction::Focus(1));
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::UnsupportedAction)
    );
}

#[test]
fn text_limit_is_shared_by_configuration_labels_values_and_actions() {
    let (mut host, world) = fixture();
    let maximum: Arc<str> = "a".repeat(MAX_GUI_TEXT_BYTES).into();
    let oversized: Arc<str> = "a".repeat(MAX_GUI_TEXT_BYTES + 1).into();

    // Configuration: one byte over the limit, or a line break in single-line
    // text, is refused wherever GUI text is authored.
    for invalid in [
        ComponentValue::GuiTextInput(GuiTextInput {
            text: oversized.clone(),
            placeholder: Arc::default(),
            ..Default::default()
        }),
        ComponentValue::GuiTextInput(GuiTextInput {
            text: "two\nlines".into(),
            placeholder: Arc::default(),
            ..Default::default()
        }),
        ComponentValue::GuiTextInput(GuiTextInput {
            text: Arc::default(),
            placeholder: oversized.clone(),
            ..Default::default()
        }),
        ComponentValue::GuiButton(GuiButton {
            label: oversized.clone(),
            ..Default::default()
        }),
        ComponentValue::GuiCheckbox(GuiCheckbox {
            checked: false,
            label: oversized.clone(),
        }),
        ComponentValue::GuiBehavior(GuiBehavior {
            semantic_label: oversized.clone(),
            ..Default::default()
        }),
    ] {
        assert_eq!(
            refused(&mut host, world, create_value(invalid)),
            ErrorReason::InvalidValue
        );
    }
    let button = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton {
            label: "Go".into(),
            ..Default::default()
        }),
    );
    let label = |value: &str| Command::SetField {
        entity: EntityRef::Handle(button),
        component: ComponentValue::GUI_BUTTON,
        field: FieldWrite {
            offset: std::mem::offset_of!(GuiButton, label) as u32,
            value: FieldValue::String(value.into()),
        },
    };
    refused(&mut host, world, vec![label(&oversized)]);
    apply(&mut host, world, vec![label(&maximum)]);
    assert!(
        host.world_mut(world)
            .unwrap()
            .inspect(button)
            .unwrap()
            .components
            .contains(&ComponentValue::GuiButton(GuiButton {
                label: maximum.clone(),
                ..Default::default()
            }))
    );

    // Text: the maximum is stored; a larger value never reaches the control,
    // whether requested semantically or written by a client.
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiTextInput(GuiTextInput {
            text: maximum.clone(),
            placeholder: Arc::default(),
            ..Default::default()
        }),
    );
    let seeded = snapshot(&mut host, world, entity);
    assert_eq!(seeded.value, GuiTestValue::Text(maximum.clone()));
    assert_eq!(
        refused(
            &mut host,
            world,
            vec![Command::SetField {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_TEXT_INPUT,
                field: FieldWrite {
                    offset: std::mem::offset_of!(GuiTextInput, text) as u32,
                    value: FieldValue::String(oversized.clone()),
                },
            }],
        ),
        ErrorReason::InvalidValue
    );
    action(
        &mut host,
        world,
        seeded.target,
        GuiLocalAction::SetText(oversized),
    );
    action(
        &mut host,
        world,
        seeded.target,
        GuiLocalAction::SetText("two\nlines".into()),
    );
    frame(&mut host);
    let terminals = outcomes(&mut host, world);
    assert_eq!(
        terminals
            .iter()
            .map(|outcome| outcome.result.clone().err())
            .collect::<Vec<_>>(),
        [
            Some(ErrorReason::InvalidValue),
            Some(ErrorReason::InvalidValue)
        ]
    );
    assert_eq!(seeded.value, snapshot(&mut host, world, entity).value);
}

#[test]
fn controls_require_the_canvas_system_that_admits_their_canvas_bounds() {
    let controls = || {
        [
            ComponentValue::GuiButton(GuiButton::default()),
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiSlider(GuiSlider::default()),
            ComponentValue::GuiTextInput(GuiTextInput::default()),
            ComponentValue::GuiScrollView(GuiScrollView::default()),
            ComponentValue::GuiVirtualList(GuiVirtualList::default()),
        ]
    };
    let (mut host, _) = fixture();
    let gui_only = host
        .create_world(Default::default(), &[crate::systems::gui::GuiSystem::ID])
        .expect("a GUI-only selection is a valid World without controls");
    for control in controls() {
        let component = ComponentValue::type_id(&control);
        assert!(
            !host
                .world_mut(gui_only)
                .unwrap()
                .manifest()
                .supports_component(component),
            "{control:?}"
        );
        assert_eq!(
            refused(&mut host, gui_only, create_value(control)),
            ErrorReason::UnsupportedDependency
        );
    }
    let manifest = host.world_mut(gui_only).unwrap().manifest().clone();
    assert!(manifest.supports_component(ComponentValue::GUI_BEHAVIOR));
    assert!(!manifest.supports_component(ComponentValue::CANVAS_BOUNDS));

    // Selecting Canvas admits every control together with its CanvasBounds.
    let (mut host, world) = fixture();
    for control in controls() {
        let entity = create(&mut host, world, control);
        let components: Vec<u16> = host
            .world_mut(world)
            .unwrap()
            .inspect(entity)
            .unwrap()
            .components
            .iter()
            .map(ComponentValue::type_id)
            .collect();
        assert!(components.contains(&ComponentValue::CANVAS_BOUNDS));
        assert!(!components.contains(&ComponentValue::CANVAS_STYLE));
        assert!(components.contains(&ComponentValue::GUI_BEHAVIOR));
    }
}

#[test]
fn scroll_configuration_bounds_the_item_count_extent_and_axis() {
    let (mut host, world) = fixture();
    let list = |item_count: u32, item_extent: f32, axis: u32| GuiVirtualList {
        item_count,
        item_extent,
        overscan: 40,
        axis,
        ..Default::default()
    };
    for invalid in [
        list((1 << 24) + 1, 1.0, 1),
        list(1, 0.0, 1),
        list(1, -1.0, 1),
        list(1, f32::INFINITY, 1),
        list(1, f32::NAN, 1),
        list(1, 1.0, 2),
        list(1 << 24, f32::MAX, 0),
    ] {
        assert_eq!(
            refused(
                &mut host,
                world,
                create_value(ComponentValue::GuiVirtualList(invalid))
            ),
            ErrorReason::InvalidValue,
            "{invalid:?}"
        );
    }
    assert_eq!(
        refused(
            &mut host,
            world,
            create_value(ComponentValue::GuiScrollView(GuiScrollView {
                axis: 3,
                ..Default::default()
            })),
        ),
        ErrorReason::InvalidValue
    );

    // The largest count, any positive finite estimate and both axes seed an
    // unscrolled list.
    for (count, extent, axis) in [(1 << 24, 0.25, 0), (7, 1.0, 1)] {
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiVirtualList(list(count, extent, axis)),
        );
        let seeded = snapshot(&mut host, world, entity);
        assert_eq!(seeded.kind, GuiControlKind::VirtualList);
        assert_eq!(seeded.value, GuiTestValue::Scroll([0.0, 0.0]));
    }
}
