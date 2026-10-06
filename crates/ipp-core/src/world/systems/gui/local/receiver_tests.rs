use super::test_support::{
    GuiTestHost, GuiTestValue, action, apply, attached, create, frame, outcomes, presented,
    snapshot,
};
use super::*;
use crate::services::gui_input::*;
use crate::systems::gui::GuiSystem;
use crate::{Command, ComponentValue, EntityRef, ErrorReason, WorldAttachmentToken, WorldId};

fn routed(
    host: &GuiTestHost,
    context: &GuiInputContext,
    target: GuiEntityTarget,
    request: u64,
    path: &[WorldAttachmentToken],
    action: GuiLocalAction,
) -> GuiLocalCommand {
    let input = host
        .service
        .reserve_routed(
            &host.host,
            context,
            target,
            request,
            path,
            host.permit(target.world.id(), request),
        )
        .unwrap();
    GuiLocalCommand::routed(input, action).unwrap()
}

fn queue(host: &mut GuiTestHost, world: WorldId, command: GuiLocalCommand) {
    host.world_mut(world)
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 999, command)
        .unwrap();
}

#[test]
fn physical_events_share_one_publication_and_apply_in_order() {
    for slider in [false, true] {
        let value = if slider {
            ComponentValue::GuiSlider(GuiSlider::default())
        } else {
            ComponentValue::GuiCheckbox(GuiCheckbox::default())
        };
        let (mut host, _, target, context) = presented(value);
        let source = host.latest_publication(target.world.id()).unwrap();
        let first = routed(
            &host,
            &context,
            target,
            100,
            &[],
            if slider {
                GuiLocalAction::SetScalar(0.25)
            } else {
                GuiLocalAction::Toggle
            },
        );
        let second = routed(
            &host,
            &context,
            target,
            101,
            &[],
            if slider {
                GuiLocalAction::SetScalar(0.75)
            } else {
                GuiLocalAction::Toggle
            },
        );
        queue(&mut host, target.world.id(), first);
        queue(&mut host, target.world.id(), second);
        frame(&mut host);
        let after = snapshot(&mut host, target.world.id(), target.entity);
        assert_eq!(
            after.value,
            if slider {
                GuiTestValue::Scalar(0.75)
            } else {
                GuiTestValue::Bool(false)
            }
        );
        let delivered = host.deliveries.borrow();
        assert_eq!(delivered.len(), 2);
        // Value operations settle without a momentary effect; the written
        // fields carry their result.
        for (index, (_, request, terminal)) in delivered.iter().enumerate() {
            assert_eq!(*request, 100 + index as u64);
            assert_eq!(
                *terminal,
                GuiDeliveryTerminal::Written {
                    target,
                    source: GuiLocalEffectSource::Routed {
                        publication: source
                    },
                    changed: true
                }
            );
        }
    }
}

#[test]
fn focus_and_momentary_press_apply_without_writing_a_value() {
    let (mut host, _, target, context) = presented(ComponentValue::GuiButton(GuiButton::default()));
    let focus = routed(&host, &context, target, 100, &[], GuiLocalAction::Focus(0));
    let press = routed(&host, &context, target, 101, &[], GuiLocalAction::Press);
    let last = routed(&host, &context, target, 102, &[], GuiLocalAction::Press);
    for command in [focus, press, last] {
        queue(&mut host, target.world.id(), command);
    }
    frame(&mut host);
    let after = snapshot(&mut host, target.world.id(), target.entity);
    assert_eq!(after.value, GuiTestValue::None);
    assert!(after.focused);
    let effects = outcomes(&mut host, target.world.id());
    assert_eq!(effects.len(), 3);
    assert_eq!(
        effects[0].effect().kind,
        GuiLocalEffectKind::FocusChanged {
            focused: true,
            changed: true,
            part: 0,
        }
    );
    assert!(
        effects[1..]
            .iter()
            .all(|effect| effect.effect().kind == GuiLocalEffectKind::Pressed)
    );
}

#[path = "focus_tests.rs"]
mod focus_tests;

#[test]
fn candidate_counter_overflow_rejects_before_any_commit() {
    let (mut host, _, target, _) = presented(ComponentValue::GuiTextInput(GuiTextInput::default()));
    host.world_mut(target.world.id())
        .unwrap()
        .with_system::<GuiSystem, _>(GuiSystem::ID, |system, _| {
            system.local.presentation_revision = u64::MAX;
        })
        .unwrap();
    let before = snapshot(&mut host, target.world.id(), target.entity);
    action(
        &mut host,
        target.world.id(),
        target,
        GuiLocalAction::Focus(0),
    );
    frame(&mut host);
    assert_eq!(
        before,
        snapshot(&mut host, target.world.id(), target.entity)
    );
    assert_eq!(
        outcomes(&mut host, target.world.id())[0].result,
        Err(ErrorReason::Capacity)
    );
}

#[test]
fn root_session_owns_child_focus_without_numeric_session_bridge_and_close_is_immediately_observable()
 {
    let (mut host, _, target, context, token) = attached();
    let focus = routed(
        &host,
        &context,
        target,
        300,
        &[token],
        GuiLocalAction::Focus(0),
    );
    queue(&mut host, target.world.id(), focus);
    frame(&mut host);
    assert!(snapshot(&mut host, target.world.id(), target.entity).focused);
    let (_, collision) = host.session.id().identity();
    host.world_mut(target.world.id())
        .unwrap()
        .release_system_session(collision);
    assert!(snapshot(&mut host, target.world.id(), target.entity).focused);

    host.service.close_session(&host.session);
    assert!(!snapshot(&mut host, target.world.id(), target.entity).focused);
    assert!(
        host.world_mut(target.world.id())
            .unwrap()
            .system::<GuiSystem>(GuiSystem::ID)
            .unwrap()
            .local
            .focus
            .is_some()
    );

    frame(&mut host);
    assert!(
        host.world_mut(target.world.id())
            .unwrap()
            .system::<GuiSystem>(GuiSystem::ID)
            .unwrap()
            .local
            .focus
            .is_none()
    );
    let session = host.service.open_session().unwrap();
    assert_ne!(session.id(), host.session.id());
    assert!(!snapshot(&mut host, target.world.id(), target.entity).focused);
}

#[test]
fn semantic_receiver_remains_available_when_parent_presentation_is_unavailable() {
    let (mut host, parent, target, _, _) = attached();
    // An invalid Canvas state update has no effect; clearing the root binding
    // withdraws the parent presentation.
    host.world_mut(parent.world().id())
        .unwrap()
        .enqueue_canvas_state_update(crate::CanvasStateUpdate {
            extent: Some([0.0, 1.0]),
            units_per_metre: None,
        })
        .unwrap();
    frame(&mut host);
    assert!(host.root_output(parent.world().id()).is_some());
    host.clear_root_output(parent.world().id());
    frame(&mut host);
    assert!(host.root_output(parent.world().id()).is_none());
    action(&mut host, target.world.id(), target, GuiLocalAction::Toggle);
    frame(&mut host);
    let after = snapshot(&mut host, target.world.id(), target.entity);
    assert_eq!(after.value, GuiTestValue::Bool(true));
    assert!(!after.focused);
    assert_eq!(
        outcomes(&mut host, target.world.id()).pop().unwrap().result,
        Ok(None)
    );
}
