//! Control lifetimes follow their entity and control component incarnation.

use super::local_tests::*;
use super::*;
use crate::{Command, ComponentValue, EntityRef, ErrorReason, HostRuntime};

#[test]
fn removing_a_container_subtree_retires_its_descendant_controls() {
    let (mut host, world) = fixture();
    let container = create(
        &mut host,
        world,
        ComponentValue::GuiBehavior(GuiBehavior::default()),
    );
    let child = |host: &mut HostRuntime, value| {
        let entity = create(host, world, value);
        apply(
            host,
            world,
            vec![Command::PlaceEntity {
                entity: EntityRef::Handle(entity),
                placement: crate::EntityPlacementRef {
                    parent: Some(EntityRef::Handle(container)),
                    before: None,
                },
            }],
        );
        entity
    };
    let checkbox = child(
        &mut host,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let text = child(
        &mut host,
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    );
    let checkbox_target = snapshot(&mut host, world, checkbox).target;
    let text_target = snapshot(&mut host, world, text).target;
    action(&mut host, world, text_target, GuiLocalAction::Focus(0));
    action(&mut host, world, checkbox_target, GuiLocalAction::Toggle);
    frame(&mut host);
    outcomes(&mut host, world);
    assert!(snapshot(&mut host, world, text).focused);

    // Deleting only the container keeps its children as roots with their
    // control lifetimes, values and focus.
    apply(
        &mut host,
        world,
        vec![Command::Delete {
            entity: EntityRef::Handle(container),
        }],
    );
    let orphan = snapshot(&mut host, world, checkbox);
    assert_eq!(orphan.target, checkbox_target);
    assert_eq!(orphan.value, GuiTestValue::Bool(true));
    assert_eq!(orphan.ancestry.as_ref(), &[checkbox]);
    assert!(snapshot(&mut host, world, text).focused);

    // Removing the subtree explicitly retires every descendant control: its
    // reads, later actions and logical focus.
    apply(
        &mut host,
        world,
        vec![
            Command::Delete {
                entity: EntityRef::Handle(checkbox),
            },
            Command::Delete {
                entity: EntityRef::Handle(text),
            },
        ],
    );
    for entity in [checkbox, text] {
        assert!(read(&mut host, world, entity).is_none());
    }
    action(&mut host, world, checkbox_target, GuiLocalAction::Toggle);
    action(&mut host, world, text_target, GuiLocalAction::Blur);
    frame(&mut host);
    let results: Vec<_> = outcomes(&mut host, world)
        .into_iter()
        .map(|outcome| outcome.result.err())
        .collect();
    assert_eq!(
        results,
        [
            Some(ErrorReason::StaleTarget),
            Some(ErrorReason::StaleTarget)
        ]
    );

    // A control created afterwards has a new lifetime and its default value.
    let fresh = create(
        &mut host,
        world,
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
    );
    let reused = snapshot(&mut host, world, fresh);
    assert_ne!(reused.target, checkbox_target);
    assert_eq!(reused.value, GuiTestValue::Bool(false));
    assert!(!reused.focused);
    action(&mut host, world, checkbox_target, GuiLocalAction::Toggle);
    frame(&mut host);
    assert_eq!(
        outcomes(&mut host, world)[0].result,
        Err(ErrorReason::StaleTarget)
    );
    assert_eq!(
        snapshot(&mut host, world, fresh).value,
        GuiTestValue::Bool(false)
    );
}
