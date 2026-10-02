use super::input_test_support::GuiTestHost;
use super::local_tests::{GuiTestValue, action, apply, create, frame, snapshot};
use super::receiver_tests::{attached, presented};
use super::*;
use crate::services::gui_input::*;
use crate::systems::gui::GuiSystem;
use crate::{Command, ComponentValue, EntityPlacementRef, EntityRef, WorldAttachmentToken};
use std::cell::RefCell;
use std::rc::Rc;

struct Pointer {
    context: GuiInputContext,
    target: GuiEntityTarget,
    path: Vec<WorldAttachmentToken>,
    lease: Option<GuiPointerLease>,
}

impl Pointer {
    fn queue(&mut self, host: &mut GuiTestHost, update: GuiInteractionUpdate) {
        let command = self.command(host, update, None);
        host.world_mut(self.target.world.id())
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }

    fn command(
        &mut self,
        host: &GuiTestHost,
        update: GuiInteractionUpdate,
        permit: Option<Box<dyn GuiDeliveryPermit>>,
    ) -> GuiLocalCommand {
        let request = host.next_request();
        let input = host
            .service
            .reserve_routed(
                &host.host,
                &self.context,
                self.target,
                request,
                &self.path,
                permit.unwrap_or_else(|| host.permit(self.target.world.id(), request)),
            )
            .unwrap();
        let lease = self
            .lease
            .get_or_insert_with(|| host.service.pointer_lease(&input, 5).unwrap())
            .clone();
        GuiLocalCommand::interaction(input, lease, update).unwrap()
    }
}

fn pointer() -> (GuiTestHost, Pointer) {
    let (host, _, target, context) = presented(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    (
        host,
        Pointer {
            context,
            target,
            path: Vec::new(),
            lease: None,
        },
    )
}

fn flags(host: &mut GuiTestHost, pointer: &Pointer) -> GuiInteractionFlags {
    snapshot(host, pointer.target.world.id(), pointer.target.entity).interaction
}

#[test]
fn ordered_feedback_retains_values_focus_and_lease_across_publication_refresh() {
    let (mut host, mut pointer) = pointer();
    let prior = snapshot(&mut host, pointer.target.world.id(), pointer.target.entity);
    for update in [
        GuiInteractionUpdate::Hover(true),
        GuiInteractionUpdate::Press,
        GuiInteractionUpdate::Capture,
    ] {
        pointer.queue(&mut host, update);
    }
    assert!(!pointer.lease.as_ref().unwrap().is_live());
    assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
    frame(&mut host);
    let active = GuiInteractionFlags {
        hovered: true,
        pressed: true,
        captured: true,
    };
    assert_eq!(flags(&mut host, &pointer), active);
    let id = pointer.lease.as_ref().unwrap().id();
    let publication = host.latest_publication(pointer.target.world.id()).unwrap();
    frame(&mut host);
    assert_ne!(
        host.latest_publication(pointer.target.world.id()).unwrap(),
        publication
    );
    pointer.queue(&mut host, GuiInteractionUpdate::Release);
    frame(&mut host);
    assert_eq!(
        flags(&mut host, &pointer),
        GuiInteractionFlags {
            hovered: true,
            ..Default::default()
        }
    );
    pointer.queue(&mut host, GuiInteractionUpdate::Hover(false));
    frame(&mut host);
    assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
    pointer.queue(&mut host, GuiInteractionUpdate::Release);
    frame(&mut host);
    assert_eq!(pointer.lease.as_ref().unwrap().id(), id);
    let after = snapshot(&mut host, pointer.target.world.id(), pointer.target.entity);
    assert_eq!(prior.value, after.value);
    assert!(!after.focused);
    let delivered = host.deliveries.borrow();
    assert_eq!(delivered.len(), 6);
    let GuiDeliveryTerminal::Applied(GuiLocalEffect {
        kind: GuiLocalEffectKind::InteractionChanged(effect),
        ..
    }) = &delivered.last().unwrap().2
    else {
        panic!("missing duplicate result");
    };
    assert!(!effect.changed);
}

#[test]
fn same_publication_feedback_applies_in_order_without_writing_a_value() {
    let (mut host, mut pointer) = pointer();
    let input = host
        .service
        .reserve_routed(
            &host.host,
            &pointer.context,
            pointer.target,
            90,
            &[],
            host.permit(pointer.target.world.id(), 90),
        )
        .unwrap();
    let lease = host.service.pointer_lease(&input, 5).unwrap();
    let first =
        GuiLocalCommand::interaction(input, lease.clone(), GuiInteractionUpdate::Press).unwrap();
    let input = host
        .service
        .reserve_routed(
            &host.host,
            &pointer.context,
            pointer.target,
            91,
            &[],
            host.permit(pointer.target.world.id(), 91),
        )
        .unwrap();
    let second =
        GuiLocalCommand::interaction(input, lease.clone(), GuiInteractionUpdate::Capture).unwrap();
    for command in [first, second] {
        host.world_mut(pointer.target.world.id())
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }
    pointer.lease = Some(lease);
    frame(&mut host);
    assert!(flags(&mut host, &pointer).captured);
    assert!(
        host.deliveries
            .borrow()
            .iter()
            .all(|(_, _, terminal)| matches!(terminal, GuiDeliveryTerminal::Applied(_)))
    );
    assert_eq!(
        snapshot(&mut host, pointer.target.world.id(), pointer.target.entity).value,
        GuiTestValue::Bool(false)
    );
}

struct RejectPermit {
    callback: Option<Box<dyn FnOnce()>>,
    terminals: Rc<RefCell<Vec<GuiDeliveryTerminal>>>,
    capacity: bool,
}

impl GuiDeliveryPermit for RejectPermit {
    fn prepare(&mut self, effect: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        if effect.is_some() {
            if let Some(callback) = self.callback.take() {
                callback();
            }
            if self.capacity {
                return Err(GuiDeliveryError::Capacity);
            }
        }
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.terminals.borrow_mut().push(terminal);
    }
}

#[test]
fn rejected_candidate_and_reentrant_lease_revocation_never_steal_live_pointer() {
    for capacity in [true, false] {
        let (mut host, mut old) = pointer();
        old.queue(&mut host, GuiInteractionUpdate::Press);
        frame(&mut host);
        let mut candidate = Pointer {
            context: old.context.clone(),
            target: old.target,
            path: Vec::new(),
            lease: None,
        };
        let terminals = Rc::new(RefCell::new(Vec::new()));
        let callback_cell = Rc::new(RefCell::new(None::<GuiPointerLease>));
        let callback_lease = callback_cell.clone();
        let service = host.service.clone();
        let permit = RejectPermit {
            terminals: terminals.clone(),
            capacity,
            callback: (!capacity).then(|| {
                Box::new(move || {
                    service
                        .cancel_pointer_lease(callback_lease.borrow().as_ref().unwrap())
                        .unwrap();
                }) as Box<dyn FnOnce()>
            }),
        };
        let command = candidate.command(
            &host,
            GuiInteractionUpdate::Hover(true),
            Some(Box::new(permit)),
        );
        *callback_cell.borrow_mut() = candidate.lease.clone();
        assert!(!candidate.lease.as_ref().unwrap().is_live());
        assert!(old.lease.as_ref().unwrap().is_live());
        host.world_mut(old.target.world.id())
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
        frame(&mut host);
        assert!(old.lease.as_ref().unwrap().is_live());
        assert!(!candidate.lease.as_ref().unwrap().is_live());
        assert_eq!(
            flags(&mut host, &old),
            GuiInteractionFlags {
                pressed: true,
                ..Default::default()
            }
        );
        assert_eq!(
            *terminals.borrow(),
            [if capacity {
                GuiDeliveryTerminal::Rejected(GuiInputError::Delivery(GuiDeliveryError::Capacity))
            } else {
                GuiDeliveryTerminal::Cancelled
            }]
        );
    }
}

#[test]
fn successful_cross_world_reuse_revokes_only_old_activation_and_stale_release_cannot_steal_back() {
    let (mut host, root, child, context, token) = attached();
    let root_entity = host
        .world_mut(root.world().id())
        .unwrap()
        .entity_children(None)
        .next()
        .unwrap();
    let parent = create(
        &mut host,
        root.world().id(),
        ComponentValue::GuiButton(GuiButton::default()),
    );
    apply(
        &mut host,
        root.world().id(),
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(parent),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root_entity)),
                before: None,
            },
        }],
    );
    frame(&mut host);
    let parent = snapshot(&mut host, root.world().id(), parent).target;
    let mut old = Pointer {
        context: context.clone(),
        target: parent,
        path: Vec::new(),
        lease: None,
    };
    old.queue(&mut host, GuiInteractionUpdate::Press);
    frame(&mut host);
    let stale_release = old.command(&host, GuiInteractionUpdate::Release, None);
    let mut new = Pointer {
        context,
        target: child,
        path: vec![token],
        lease: None,
    };
    new.queue(&mut host, GuiInteractionUpdate::Hover(true));
    assert!(old.lease.as_ref().unwrap().is_live());
    frame(&mut host);
    assert!(!old.lease.as_ref().unwrap().is_live());
    assert_eq!(flags(&mut host, &old), GuiInteractionFlags::default());
    assert!(flags(&mut host, &new).hovered);
    host.world_mut(parent.world.id())
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 800, stale_release)
        .unwrap();
    frame(&mut host);
    assert!(flags(&mut host, &new).hovered);
    host.service
        .cancel_pointer_lease(old.lease.as_ref().unwrap())
        .unwrap();
    assert!(new.lease.as_ref().unwrap().is_live());
}

#[test]
fn session_context_and_exact_lease_cancellation_are_immediate() {
    for cancellation in 0..3 {
        let (mut host, mut pointer) = pointer();
        pointer.queue(&mut host, GuiInteractionUpdate::Press);
        frame(&mut host);
        match cancellation {
            0 => {
                host.service.close_session(&host.session);
            }
            1 => {
                host.service.release_context(&pointer.context);
            }
            _ => {
                host.service
                    .cancel_pointer_lease(pointer.lease.as_ref().unwrap())
                    .unwrap();
            }
        }
        assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());

        frame(&mut host);
        assert!(
            host.world_mut(pointer.target.world.id())
                .unwrap()
                .system::<GuiSystem>(GuiSystem::ID)
                .unwrap()
                .local
                .pointers
                .is_empty()
        );
    }
}

#[test]
fn local_policy_cancellation_is_permanent_after_correction_without_touching_value_or_focus() {
    for hidden in [false, true] {
        let (mut host, mut pointer) = pointer();
        pointer.queue(&mut host, GuiInteractionUpdate::Press);
        frame(&mut host);
        let before = snapshot(&mut host, pointer.target.world.id(), pointer.target.entity);
        apply(
            &mut host,
            pointer.target.world.id(),
            vec![Command::insert_value(
                EntityRef::Handle(pointer.target.entity),
                ComponentValue::GuiBehavior(GuiBehavior {
                    enabled: hidden,
                    visible: !hidden,
                    ..Default::default()
                }),
            )],
        );
        assert!(!pointer.lease.as_ref().unwrap().is_live());
        assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
        apply(
            &mut host,
            pointer.target.world.id(),
            vec![Command::RemoveComponent {
                entity: EntityRef::Handle(pointer.target.entity),
                component: ComponentValue::GUI_BEHAVIOR,
            }],
        );
        frame(&mut host);
        assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
        assert_eq!(
            before.value,
            snapshot(&mut host, pointer.target.world.id(), pointer.target.entity).value
        );
    }
}

fn assert_local_clip_lifetime(style: crate::components::CanvasStyle, eligible: bool) {
    for ancestor in [false, true] {
        let (mut host, mut pointer) = pointer();
        let world = pointer.target.world.id();
        let entity = pointer.target.entity;
        let owner = if ancestor {
            let root = snapshot(&mut host, world, entity).ancestry[0];
            let parent = create(
                &mut host,
                world,
                ComponentValue::GuiBehavior(GuiBehavior::default()),
            );
            apply(
                &mut host,
                world,
                vec![
                    Command::PlaceEntity {
                        entity: EntityRef::Handle(parent),
                        placement: EntityPlacementRef {
                            parent: Some(EntityRef::Handle(root)),
                            before: None,
                        },
                    },
                    Command::PlaceEntity {
                        entity: EntityRef::Handle(entity),
                        placement: EntityPlacementRef {
                            parent: Some(EntityRef::Handle(parent)),
                            before: None,
                        },
                    },
                ],
            );
            parent
        } else {
            entity
        };
        action(&mut host, world, pointer.target, GuiLocalAction::Focus(0));
        frame(&mut host);
        for update in [
            GuiInteractionUpdate::Hover(true),
            GuiInteractionUpdate::Press,
            GuiInteractionUpdate::Capture,
        ] {
            pointer.queue(&mut host, update);
        }
        frame(&mut host);
        let before = snapshot(&mut host, world, entity);
        let delivered = host.deliveries.borrow().len();
        assert!(before.focused && before.interaction.captured);

        apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(owner),
                ComponentValue::CanvasStyle(style),
            )],
        );
        // The written style is stored as is.
        let stored = host
            .world_mut(world)
            .unwrap()
            .world
            .components
            .canvas_style(owner.index() as usize)
            .cloned();
        assert_eq!(stored, Some(style));
        let expected = if eligible {
            before.interaction
        } else {
            GuiInteractionFlags::default()
        };
        assert_eq!(
            pointer.lease.as_ref().unwrap().is_live(),
            eligible,
            "ancestor={ancestor}, style={style:?}"
        );
        let after = snapshot(&mut host, world, entity);
        assert_eq!(after.interaction, expected);
        assert_eq!(after.focused, before.focused);
        assert_eq!(after.value, before.value);

        let correction = vec![Command::insert_value(
            EntityRef::Handle(owner),
            ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                clipped: true,
                clip_max_x: 100.0,
                clip_max_y: 100.0,
                ..Default::default()
            }),
        )];
        apply(&mut host, world, correction);
        frame(&mut host);
        let after = snapshot(&mut host, world, entity);
        assert_eq!(after.interaction, expected);
        assert_eq!(after.focused, before.focused);
        assert_eq!(after.value, before.value);
        assert_eq!(pointer.lease.as_ref().unwrap().is_live(), eligible);
        assert_eq!(host.deliveries.borrow().len(), delivered);
        if !eligible {
            let request = host.next_request();
            let input = host
                .service
                .reserve_routed(
                    &host.host,
                    &pointer.context,
                    pointer.target,
                    request,
                    &[],
                    host.permit(world, request),
                )
                .unwrap();
            assert!(matches!(
                GuiLocalCommand::interaction(
                    input,
                    pointer.lease.as_ref().unwrap().clone(),
                    GuiInteractionUpdate::Press,
                ),
                Err(GuiInputError::Cancelled)
            ));
            pointer.lease = None;
        }
        pointer.queue(&mut host, GuiInteractionUpdate::Hover(true));
        frame(&mut host);
        assert!(flags(&mut host, &pointer).hovered);
        assert!(pointer.lease.as_ref().unwrap().is_live());
    }
}

#[test]
fn empty_local_clips_cancel_capture_permanently_after_correction() {
    for (clip_max_x, clip_max_y) in [(0.0, 0.0), (0.0, 100.0), (100.0, 0.0)] {
        assert_local_clip_lifetime(
            crate::components::CanvasStyle {
                clipped: true,
                clip_max_x,
                clip_max_y,
                ..Default::default()
            },
            false,
        );
    }
}

#[test]
fn reversed_local_clips_cancel_capture_permanently_after_correction() {
    for (clip_min_x, clip_min_y) in [(200.0, 0.0), (0.0, 200.0)] {
        assert_local_clip_lifetime(
            crate::components::CanvasStyle {
                clipped: true,
                clip_min_x,
                clip_min_y,
                clip_max_x: 100.0,
                clip_max_y: 100.0,
                ..Default::default()
            },
            false,
        );
    }
}

#[test]
fn positive_and_disabled_local_clips_preserve_capture_without_value_or_focus_changes() {
    for clipped in [false, true] {
        assert_local_clip_lifetime(
            crate::components::CanvasStyle {
                clipped,
                clip_min_x: if clipped {
                    0.0
                } else {
                    200.0
                },
                clip_min_y: if clipped {
                    0.0
                } else {
                    200.0
                },
                clip_max_x: 100.0,
                clip_max_y: 100.0,
                ..Default::default()
            },
            true,
        );
    }
}

#[test]
fn cancel_and_world_removal_revoke_handles_without_another_input_or_native_capture() {
    let (mut host, mut pointer) = pointer();
    pointer.queue(&mut host, GuiInteractionUpdate::Press);
    pointer.queue(&mut host, GuiInteractionUpdate::Capture);
    frame(&mut host);
    pointer.queue(&mut host, GuiInteractionUpdate::Cancel);
    frame(&mut host);
    assert!(!pointer.lease.as_ref().unwrap().is_live());
    assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
    pointer.lease = None;
    pointer.queue(&mut host, GuiInteractionUpdate::Hover(true));
    frame(&mut host);
    assert!(pointer.lease.as_ref().unwrap().is_live());
    assert!(host.destroy_world(pointer.target.world.id()));
    assert!(!pointer.lease.as_ref().unwrap().is_live());
}

#[test]
fn control_incarnation_removal_and_invisible_style_cancel_without_revival() {
    for remove in [false, true] {
        let (mut host, mut pointer) = pointer();
        pointer.queue(&mut host, GuiInteractionUpdate::Press);
        frame(&mut host);
        let entity = EntityRef::Handle(pointer.target.entity);
        let operations = if remove {
            vec![Command::RemoveComponent {
                entity: entity.clone(),
                component: ComponentValue::GUI_CHECKBOX,
            }]
        } else {
            vec![Command::insert_value(
                entity.clone(),
                ComponentValue::CanvasStyle(crate::components::CanvasStyle {
                    opacity: 0.0,
                    ..Default::default()
                }),
            )]
        };
        apply(&mut host, pointer.target.world.id(), operations);
        assert!(!pointer.lease.as_ref().unwrap().is_live());
        let correction = if remove {
            Command::insert_value(entity, ComponentValue::GuiCheckbox(GuiCheckbox::default()))
        } else {
            Command::RemoveComponent {
                entity,
                component: ComponentValue::CANVAS_STYLE,
            }
        };
        apply(&mut host, pointer.target.world.id(), vec![correction]);
        frame(&mut host);
        let after = snapshot(&mut host, pointer.target.world.id(), pointer.target.entity);
        assert_eq!(after.interaction, GuiInteractionFlags::default());
        assert!(!pointer.lease.as_ref().unwrap().is_live());
        if remove {
            assert_ne!(after.target.incarnation, pointer.target.incarnation);
        }
    }
}

#[test]
fn graph_restore_excludes_live_and_queued_interaction_without_revoking_original_world() {
    let (mut host, mut pointer) = pointer();
    pointer.queue(&mut host, GuiInteractionUpdate::Hover(true));
    frame(&mut host);
    pointer.queue(&mut host, GuiInteractionUpdate::Press);
    let saved = host
        .save_world(pointer.target.world.id(), 71, Default::default())
        .unwrap();
    let loaded = host
        .load_world(
            &saved,
            71,
            crate::services::world_serialization::WorldLoadOptions {
                symbolic_id: Some("interaction-copy".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored = {
        let world = host.world_mut(loaded.root.id()).unwrap();
        world
            .entities()
            .iter()
            .find_map(|entity| crate::systems::gui::test_support::read_control(&world, entity.id))
            .unwrap()
    };
    assert_ne!(restored.target.world, pointer.target.world);
    assert_eq!(restored.value, GuiTestValue::Bool(false));
    assert_eq!(restored.interaction, GuiInteractionFlags::default());
    assert!(!restored.focused);
    assert!(pointer.lease.as_ref().unwrap().is_live());
    frame(&mut host);
    assert!(flags(&mut host, &pointer).pressed);
    assert_eq!(
        snapshot(&mut host, loaded.root.id(), restored.target.entity).interaction,
        GuiInteractionFlags::default()
    );
}

#[test]
fn older_inert_candidate_cannot_activate_after_a_newer_generation_has_committed() {
    let (mut host, mut old) = pointer();
    let delayed = old.command(&host, GuiInteractionUpdate::Press, None);
    let mut new = Pointer {
        context: old.context.clone(),
        target: old.target,
        path: Vec::new(),
        lease: None,
    };
    new.queue(&mut host, GuiInteractionUpdate::Hover(true));
    let current = new.lease.as_ref().unwrap();
    assert!(!current.is_live());
    host.world_mut(old.target.world.id())
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 800, delayed)
        .unwrap();
    frame(&mut host);
    assert!(current.is_live());
    assert!(!old.lease.as_ref().unwrap().is_live());
    assert_eq!(
        flags(&mut host, &new),
        GuiInteractionFlags {
            hovered: true,
            ..Default::default()
        }
    );
}

#[test]
fn reparent_within_the_canvas_keeps_feedback() {
    let (mut host, mut pointer) = pointer();
    let original = snapshot(&mut host, pointer.target.world.id(), pointer.target.entity);
    let root = original.ancestry[0];
    let parent = create(
        &mut host,
        pointer.target.world.id(),
        ComponentValue::GuiBehavior(GuiBehavior::default()),
    );
    apply(
        &mut host,
        pointer.target.world.id(),
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(parent),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(root)),
                before: None,
            },
        }],
    );
    frame(&mut host);
    pointer.queue(&mut host, GuiInteractionUpdate::Hover(true));
    frame(&mut host);
    apply(
        &mut host,
        pointer.target.world.id(),
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(pointer.target.entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    );
    assert!(pointer.lease.as_ref().unwrap().is_live());
}

#[test]
fn unsupported_capture_and_counter_overflow_leave_candidates_inert_and_existing_authority_intact() {
    for overflow in [false, true] {
        let (mut host, mut pointer) = pointer();
        if overflow {
            host.world_mut(pointer.target.world.id())
                .unwrap()
                .with_system::<GuiSystem, _>(GuiSystem::ID, |system, _| {
                    system.local.presentation_revision = u64::MAX
                })
                .unwrap();
        }
        pointer.queue(
            &mut host,
            if overflow {
                GuiInteractionUpdate::Press
            } else {
                GuiInteractionUpdate::Capture
            },
        );
        frame(&mut host);
        assert!(!pointer.lease.as_ref().unwrap().is_live());
        assert_eq!(flags(&mut host, &pointer), GuiInteractionFlags::default());
        assert_eq!(
            host.deliveries.borrow().last().unwrap().2,
            GuiDeliveryTerminal::Rejected(if overflow {
                GuiInputError::Capacity
            } else {
                GuiInputError::Local(GuiLocalActionError::UnsupportedAction)
            })
        );
    }
}
