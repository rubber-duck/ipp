use super::local_tests::{GuiTestValue, action, create, fixture, frame, outcomes, snapshot};
use super::*;
use crate::ComponentValue;
use crate::ErrorReason;
use std::sync::Arc;

#[test]
fn submit_retains_the_exact_stored_text_without_changing_it_or_focus() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::SetText("é🙂".into()),
    );
    frame(&mut host);
    outcomes(&mut host, world);
    let before = snapshot(&mut host, world, entity);
    action(&mut host, world, target, GuiLocalAction::Submit);
    frame(&mut host);
    let effect = outcomes(&mut host, world).pop().unwrap().into_effect();
    let GuiLocalEffectKind::Submitted(state) = effect.kind else {
        panic!("expected submitted effect")
    };
    assert!(effect.id.is_some());
    assert_eq!(effect.ancestry.as_ref(), &[entity]);
    let GuiTestValue::Text(stored) = &before.value else {
        panic!("text value");
    };
    assert!(Arc::ptr_eq(&state, stored), "submit shares the stored text");
    assert_eq!(&*state, "é🙂");
    let after = snapshot(&mut host, world, entity);
    assert_eq!(before, after);
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::SetText("later".into()),
    );
    frame(&mut host);
    assert_eq!(&*state, "é🙂");
    assert_eq!(
        snapshot(&mut host, world, entity).value,
        GuiTestValue::Text("later".into())
    );
}

#[test]
fn submit_does_not_activate_non_text_controls() {
    let (mut host, world) = fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    frame(&mut host);
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::Submit);
    frame(&mut host);
    assert!(matches!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::UnsupportedAction)
    ));
}
