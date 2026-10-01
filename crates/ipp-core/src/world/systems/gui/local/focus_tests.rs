use super::*;

#[derive(Clone, Debug, PartialEq)]
struct FocusObservation {
    /// Focused target and its owning input session; command focus has none.
    owner: Option<(GuiEntityTarget, Option<GuiInputSessionId>)>,
    paint: u64,
    text: u64,
}

impl FocusObservation {
    fn of(gui: &GuiSystem) -> Self {
        Self {
            owner: gui
                .local
                .focus
                .as_ref()
                .map(|(target, owner)| (*target, owner.as_ref().map(GuiInputSession::id))),
            paint: gui.local.presentation_revision,
            text: gui.local.text_revision,
        }
    }
}

fn observed(host: &mut GuiTestHost, world: WorldId) -> FocusObservation {
    FocusObservation::of(
        host.world_mut(world)
            .unwrap()
            .system::<GuiSystem>(GuiSystem::ID)
            .unwrap(),
    )
}

/// One applied focus action: a change publishes `FocusChanged`, a no-op
/// publishes nothing.
fn focus_effect(host: &mut GuiTestHost, world: WorldId, focused: bool, changed: bool) {
    let effects = outcomes(host, world);
    assert_eq!(effects.len(), 1);
    if !changed {
        assert_eq!(effects[0].result, Ok(None));
        return;
    }
    assert_eq!(
        effects[0].effect().kind,
        GuiLocalEffectKind::FocusChanged {
            focused,
            changed
        }
    );
}

#[test]
fn headless_focus_edit_blur_and_duplicates_preserve_the_value_and_exact_dirty_work() {
    let (mut host, world) = super::super::local_tests::fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    );
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::Focus);
    frame(&mut host);
    focus_effect(&mut host, world, true, true);
    let focused = observed(&mut host, world);
    action(&mut host, world, target, GuiLocalAction::Focus);
    frame(&mut host);
    focus_effect(&mut host, world, true, false);
    assert_eq!(observed(&mut host, world), focused);
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::SetText("edited".into()),
    );
    frame(&mut host);
    outcomes(&mut host, world);
    let edited = snapshot(&mut host, world, entity);
    assert_eq!(edited.value, GuiTestValue::Text("edited".into()));
    let before = observed(&mut host, world);
    action(&mut host, world, target, GuiLocalAction::Blur);
    frame(&mut host);
    focus_effect(&mut host, world, false, true);
    let after = snapshot(&mut host, world, entity);
    assert!(!after.focused);
    assert_eq!(edited.value, after.value);
    let blurred = observed(&mut host, world);
    assert_eq!(blurred.paint, before.paint + 1);
    assert_eq!(blurred.text, before.text);
    action(&mut host, world, target, GuiLocalAction::Blur);
    frame(&mut host);
    focus_effect(&mut host, world, false, false);
    assert_eq!(observed(&mut host, world), blurred);
    assert_eq!(edited.value, snapshot(&mut host, world, entity).value);
    assert!(host.root_output(world).is_none());
}

#[test]
fn delayed_blur_never_clears_another_targets_focus() {
    let (mut host, world) = super::super::local_tests::fixture();
    let first = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let second = create(
        &mut host,
        world,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let first = snapshot(&mut host, world, first).target;
    let second = snapshot(&mut host, world, second).target;
    action(&mut host, world, first, GuiLocalAction::Focus);
    frame(&mut host);
    outcomes(&mut host, world);
    action(&mut host, world, second, GuiLocalAction::Focus);
    action(&mut host, world, first, GuiLocalAction::Blur);
    frame(&mut host);
    let effects = outcomes(&mut host, world);
    assert_eq!(effects.len(), 2);
    assert_eq!(
        effects[0].effect().kind,
        GuiLocalEffectKind::FocusChanged {
            focused: true,
            changed: true
        }
    );
    assert_eq!(effects[1].result, Ok(None));
    assert!(snapshot(&mut host, world, second.entity).focused);
    assert!(!snapshot(&mut host, world, first.entity).focused);
    assert_eq!(observed(&mut host, world).owner, Some((second, None)));
}

#[test]
fn blur_requires_current_lifetime_and_local_eligibility() {
    let (mut host, world) = super::super::local_tests::fixture();
    let entity = create(
        &mut host,
        world,
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    );
    let target = snapshot(&mut host, world, entity).target;
    action(&mut host, world, target, GuiLocalAction::Focus);
    action(
        &mut host,
        world,
        target,
        GuiLocalAction::SetText("new".into()),
    );
    frame(&mut host);
    outcomes(&mut host, world);
    apply(
        &mut host,
        world,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(entity),
                component: ComponentValue::GUI_TEXT_INPUT,
            },
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiTextInput(GuiTextInput::default()),
            ),
        ],
    );
    let replacement = snapshot(&mut host, world, entity).target;
    assert_ne!(replacement, target);
    action(&mut host, world, replacement, GuiLocalAction::Focus);
    action(&mut host, world, target, GuiLocalAction::Blur);
    frame(&mut host);
    assert!(snapshot(&mut host, world, entity).focused);
    let settled = outcomes(&mut host, world);
    assert_eq!(settled.len(), 2);
    assert_eq!(settled[1].result, Err(ErrorReason::StaleTarget));
    for hidden in [false, true] {
        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiBehavior(GuiBehavior {
                    visible: !hidden,
                    enabled: hidden,
                    ..Default::default()
                }),
            )],
        );
        action(&mut host, world, replacement, GuiLocalAction::Blur);
        frame(&mut host);
        assert_eq!(
            outcomes(&mut host, world)[0].result,
            Err(ErrorReason::Unavailable)
        );
    }
}

#[test]
fn same_publication_focus_edit_blur_apply_in_order_and_clear_canvas_priority() {
    let (mut host, output, target, context) =
        presented(ComponentValue::GuiTextInput(GuiTextInput::default()));
    let source = host.latest_publication(target.world.id()).unwrap();
    let focus = routed(&host, &context, target, 100, &[], GuiLocalAction::Focus);
    let edit = routed(
        &host,
        &context,
        target,
        101,
        &[],
        GuiLocalAction::SetText("edited".into()),
    );
    let blur = routed(&host, &context, target, 102, &[], GuiLocalAction::Blur);
    for command in [focus, edit, blur] {
        queue(&mut host, target.world.id(), command);
    }
    frame(&mut host);
    let after = snapshot(&mut host, target.world.id(), target.entity);
    assert!(!after.focused);
    assert_eq!(after.value, GuiTestValue::Text("edited".into()));
    let effects = outcomes(&mut host, target.world.id());
    assert_eq!(effects.len(), 3);

    // The edit writes the field and settles without a momentary effect.
    assert_eq!(effects[1].result, Ok(None));
    for effect in [&effects[0], &effects[2]] {
        assert_eq!(
            effect.effect().source,
            GuiLocalEffectSource::Routed {
                publication: source
            }
        );
        assert_eq!(effect.effect().tick, effects[0].effect().tick);
    }
    assert_eq!(
        effects[2].effect().kind,
        GuiLocalEffectKind::FocusChanged {
            focused: false,
            changed: true
        }
    );
    let publication = host
        .output(host.latest_publication(target.world.id()).unwrap(), output)
        .unwrap()
        .data::<crate::systems::canvas::CanvasPublication>()
        .unwrap();
    assert!(!publication.interaction.requires_direct());
}

#[test]
fn revoked_routed_blur_cannot_clear_logical_focus_but_headless_semantic_blur_can() {
    let (mut host, _, target, context) = presented(ComponentValue::GuiButton(GuiButton::default()));
    action(&mut host, target.world.id(), target, GuiLocalAction::Focus);
    frame(&mut host);
    outcomes(&mut host, target.world.id());
    let blur = routed(&host, &context, target, 100, &[], GuiLocalAction::Blur);
    host.service.release_context(&context);
    let before = observed(&mut host, target.world.id());
    queue(&mut host, target.world.id(), blur);
    frame(&mut host);
    assert_eq!(observed(&mut host, target.world.id()), before);
    assert_eq!(
        host.deliveries.borrow().last().unwrap().2,
        GuiDeliveryTerminal::Cancelled
    );
    host.deliveries.borrow_mut().clear();
    assert!(snapshot(&mut host, target.world.id(), target.entity).focused);

    action(&mut host, target.world.id(), target, GuiLocalAction::Blur);
    frame(&mut host);
    focus_effect(&mut host, target.world.id(), false, true);
}

#[test]
fn blur_overflow_preflight_and_no_focus_noop_do_not_wrap_dirty_counters() {
    for focused in [false, true] {
        let (mut host, world) = super::super::local_tests::fixture();
        let entity = create(
            &mut host,
            world,
            ComponentValue::GuiButton(GuiButton::default()),
        );
        let target = snapshot(&mut host, world, entity).target;
        if focused {
            action(&mut host, world, target, GuiLocalAction::Focus);
            frame(&mut host);
            outcomes(&mut host, world);
        }
        host.world_mut(world)
            .unwrap()
            .with_system::<GuiSystem, _>(GuiSystem::ID, |system, _| {
                system.local.presentation_revision = u64::MAX;
            })
            .unwrap();
        let before = observed(&mut host, world);
        action(&mut host, world, target, GuiLocalAction::Blur);
        frame(&mut host);
        assert_eq!(observed(&mut host, world), before);
        assert_eq!(snapshot(&mut host, world, entity).value, GuiTestValue::None);
        if focused {
            assert_eq!(
                outcomes(&mut host, world)[0].result,
                Err(ErrorReason::Capacity)
            );
            assert!(snapshot(&mut host, world, entity).focused);
        } else {
            focus_effect(&mut host, world, false, false);
        }
    }
}
