use super::*;
use crate::services::gui_input::query::{
    GuiQueryOptions, GuiQueryOutcome, project_composed_point, query_composed_input,
};
use crate::services::gui_input::{
    GUI_INPUT_MAX_POINTERS, GuiDeliveryTerminal, GuiInputContext, GuiInputService, GuiInputSession,
    GuiNativeCancellation, GuiPointerLease,
};
use crate::systems::canvas::{CanvasAxis, CanvasHitKind, CanvasSystem};
use crate::systems::gui::GuiSystem;
use crate::systems::gui::local::{
    GuiControlKind, GuiInteractionPart, GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand,
    GuiScrollChain,
};
use crate::systems::gui::presentation::{GuiCanvasPublication, GuiControlObservation};
use crate::{
    HostRuntime, ViewDescriptor, ViewQueryTarget, WorldAttachmentToken, WorldPublicationId,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Target {
    pub(super) control: Arc<GuiControlObservation>,
    pub(super) path: Vec<WorldAttachmentToken>,
    pub(super) source: WorldPublicationId,
    pub(super) part: CanvasHitKind,
}

#[derive(Clone)]
enum Drag {
    Text,
    Content {
        targets: Vec<Target>,
        origin: [f32; 2],
        last: [f32; 2],
        active: bool,
    },
    Slider(f32),
    Scroll {
        axis: usize,
        grab: f32,
    },
}

#[derive(Clone)]
struct Pointer {
    target: Target,
    lease: GuiPointerLease,
    pressed: bool,
    tap: bool,
    drag: Option<Drag>,
}

/// Sparse physical interaction state for one exact root binding/context lifetime.
/// No authored values or GUI tree are mirrored here.
pub struct GuiRoutingContext {
    session: GuiInputSession,
    context: GuiInputContext,
    pointers: BTreeMap<u64, Pointer>,
    focus: Option<Target>,
    scroll: Option<GuiScrollChain>,
    blockers: Vec<super::super::query::GuiPickingBlocker>,
    queue_owner: u64,
    queued_worlds: Rc<RefCell<BTreeMap<crate::WorldRef, usize>>>,
}

/// Host-owned router shared by physical adapters, not a World evaluator.
#[derive(Default)]
pub struct GuiInputRouter {
    service: GuiInputService,
    next_request: u64,
}

impl GuiInputRouter {
    /// Read only the exact physical focus owner's authoritative native state.
    pub fn with_native_text<Result>(
        &self,
        host: &mut HostRuntime,
        context: &GuiRoutingContext,
        inspect: impl FnOnce(Option<&crate::systems::gui::local::GuiNativeTextState>) -> Result,
    ) -> Result {
        if context.context.0.native.borrow().focus.is_none() {
            return inspect(None);
        }
        let Some(focus) = context.focus.as_ref() else {
            return inspect(None);
        };
        let Some(world) = host.world_mut(focus.control.record.target.world.id()) else {
            return inspect(None);
        };
        inspect(world.gui_native_text(focus.control.record.target, context.session.id()))
    }

    /// Cancel stale physical ownership after the Host frame, including idle input.
    /// Completed policy plus the current scheduling domain determine eligibility;
    /// no local control mutation or extra World evaluation occurs here.
    pub fn synchronize(
        &self,
        host: &HostRuntime,
        context: &mut GuiRoutingContext,
        view: Option<ViewDescriptor>,
    ) -> GuiRoutingCancellation {
        let mut cancelled = GuiRoutingCancellation::default();
        context.pointers.retain(|pointer, active| {
            if view.is_some_and(|view| current_target(host, view, &active.target).is_ok())
                && active.lease.usable()
            {
                return true;
            }
            let _ = self.service.cancel_pointer_lease(&active.lease);
            let _ = self
                .service
                .set_pointer_capture(&context.context, *pointer, None);
            cancelled.pointers[cancelled.count] = *pointer;
            cancelled.count += 1;
            false
        });
        if let Some(focus) = &context.focus {
            let result = view
                .ok_or(GuiInputError::Unavailable)
                .and_then(|view| current_target(host, view, focus));
            if result.is_err() {
                cancelled.focus = context.context.0.native.borrow().focus.is_some();
                let _ = self.service.set_native_focus(&context.context, None);
                context.focus = None;
            }
        }
        cancelled
    }

    /// Acquire a fresh context. The caller first validates its physical selection.
    pub fn bind(
        &self,
        host: &HostRuntime,
        root: crate::WorldRef,
        queue_owner: u64,
        blockers: Vec<super::super::query::GuiPickingBlocker>,
    ) -> Result<(GuiRoutingContext, Vec<GuiNativeCancellation>), GuiInputError> {
        let session = self.service.open_session()?;
        let binding = match self.service.bind_context(host, &session, root) {
            Ok(binding) => binding,
            Err(error) => {
                self.service.close_session(&session);
                return Err(error);
            }
        };
        Ok((
            GuiRoutingContext {
                session,
                context: binding.context,
                pointers: BTreeMap::new(),
                focus: None,
                scroll: None,
                blockers,
                queue_owner,
                queued_worlds: Rc::default(),
            },
            binding.cancelled,
        ))
    }

    /// Revoke before physical selection, context or viewport replacement.
    pub fn release(
        &self,
        host: &mut HostRuntime,
        context: GuiRoutingContext,
    ) -> Vec<GuiNativeCancellation> {
        let worlds: Vec<_> = context.queued_worlds.borrow().keys().copied().collect();
        let mut cancelled = self.service.release_context(&context.context);
        cancelled.extend(self.service.close_session(&context.session));
        for world in worlds {
            if host.world_ref(world.id()) == Some(world)
                && let Some(mut world) = host.world_mut(world.id())
            {
                world.release_system_session(context.queue_owner);
            }
        }
        cancelled
    }

    /// A receipt-local wheel remainder, read only after every child command settles.
    pub fn scroll_remainder(&self, context: &GuiRoutingContext) -> Option<GuiScrollChain> {
        context.scroll.clone()
    }

    /// Negative admission at receipt, before an older gate can close. This does
    /// not route positive work ahead of the Host-owned publication boundary.
    pub fn admission(
        &self,
        host: &HostRuntime,
        context: &GuiRoutingContext,
        view: ViewDescriptor,
        input: &GuiPhysicalInput,
    ) -> Result<(), GuiInputError> {
        self.service.context(&context.context)?;
        if context.context.root().output != view.output
            || context.context.root().viewport != view.viewport
        {
            return Err(GuiInputError::StaleContext);
        }
        let query = ViewQueryTarget::BoundView {
            binding: context.context.root(),
            publication: Some(view.publication),
        };
        host.resolve_view(query)
            .map_err(|_| GuiInputError::StaleContext)?;
        if matches!(
            input,
            GuiPhysicalInput::PointerCancel { .. } | GuiPhysicalInput::Blur
        ) {
            return Ok(());
        }
        live(host, view.output.world())?;
        if picking_blocked(host, query, input, context)? {
            return Ok(());
        }
        let target = match input {
            GuiPhysicalInput::PointerDown {
                point,
                ..
            }
            | GuiPhysicalInput::Wheel {
                point,
                ..
            } => hit(host, query, *point, &context.blockers)?,
            GuiPhysicalInput::PointerMove {
                pointer,
                point,
            }
            | GuiPhysicalInput::PointerUp {
                pointer,
                point,
                ..
            } => match context.pointers.get(pointer) {
                Some(pointer) if pointer.pressed => Some(refresh(host, view, &pointer.target)?),
                _ => hit(host, query, *point, &context.blockers)?,
            },
            GuiPhysicalInput::Text {
                ..
            }
            | GuiPhysicalInput::Key {
                ..
            } => context
                .focus
                .as_ref()
                .map(|focus| refresh(host, view, focus))
                .transpose()?,
            _ => None,
        };
        if let Some(target) = target {
            live(host, target.control.record.target.world)?;
            for edge in target.path {
                live(host, edge.parent())?;
            }
        }
        Ok(())
    }

    /// Route in ingress order against the supplied completed physical source.
    /// All local changes remain ordinary next-boundary System commands.
    pub fn route(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        view: ViewDescriptor,
        input: GuiPhysicalInput,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<GuiRoutingDisposition, GuiInputError> {
        // A native edit for a control that no longer holds focus is stale: it is
        // rejected without disturbing the current focus, native focus or pointers.
        if let GuiPhysicalInput::Text {
            fence,
            ..
        } = &input
            && context
                .focus
                .as_ref()
                .is_none_or(|focus| focus.control.record.target != fence.target)
        {
            return Err(GuiInputError::Unavailable);
        }
        let result = self.route_inner(host, context, view, input, delivery);
        if result.is_err() {
            for active in context.pointers.values() {
                self.service.cancel_pointer_lease(&active.lease)?;
            }
            context.focus = None;
            let _ = self.service.set_native_focus(&context.context, None);
        }
        result
    }

    fn route_inner(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        view: ViewDescriptor,
        input: GuiPhysicalInput,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<GuiRoutingDisposition, GuiInputError> {
        context.scroll = None;
        self.admission(host, context, view, &input)?;
        if context.context.root().output != view.output
            || context.context.root().viewport != view.viewport
        {
            return Err(GuiInputError::StaleContext);
        }
        let query = ViewQueryTarget::BoundView {
            binding: context.context.root(),
            publication: Some(view.publication),
        };
        host.resolve_view(query)
            .map_err(|_| GuiInputError::StaleContext)?;
        if picking_blocked(host, query, &input, context)? {
            return Ok(GuiRoutingDisposition::Blocked);
        }
        if let GuiPhysicalInput::PointerDown {
            point,
            button,
            ..
        }
        | GuiPhysicalInput::PointerUp {
            point,
            button,
            ..
        } = &input
            && *button != GuiPhysicalButton::Primary
        {
            let result = query_composed_input(
                host,
                query,
                *point,
                GuiQueryOptions {
                    blockers: &context.blockers,
                },
            )
            .map_err(|_| GuiInputError::StaleContext)?;
            return match result.outcome {
                GuiQueryOutcome::Miss => Ok(GuiRoutingDisposition::Miss),
                GuiQueryOutcome::Hit(_)
                | GuiQueryOutcome::Blocked {
                    ..
                } => Ok(GuiRoutingDisposition::Blocked),
                GuiQueryOutcome::Unavailable(_) => Err(GuiInputError::Unavailable),
            };
        }
        for active in context.pointers.values_mut() {
            active.target = refresh(host, view, &active.target)?;
        }
        if let Some(focus) = &mut context.focus {
            *focus = refresh(host, view, focus)?;
        }
        match input {
            GuiPhysicalInput::Text {
                fence,
                edit,
            } => {
                let target = context
                    .focus
                    .clone()
                    .filter(|target| target.control.record.target == fence.target)
                    .ok_or(GuiInputError::Unavailable)?;
                let input = self.input(host, context, &target, delivery)?;
                enqueue(
                    host,
                    context.queue_owner,
                    fence.target,
                    GuiLocalCommand::text(input, fence, edit)?,
                )?;
                Ok(GuiRoutingDisposition::Routed {
                    target: fence.target,
                })
            }
            GuiPhysicalInput::PointerDown {
                pointer,
                point,
                ..
            } => {
                self.cancel_pointer(host, context, pointer, delivery)?;
                // A panel is opaque to the scene: a press on its content that
                // selects no control is blocked, as for the other buttons.
                let target = match locate(host, query, point, &context.blockers)? {
                    Located::Target(target) => target,
                    Located::Panel => return Ok(GuiRoutingDisposition::Blocked),
                    Located::Miss => return Ok(GuiRoutingDisposition::Miss),
                };
                if context.pointers.len() >= GUI_INPUT_MAX_POINTERS {
                    return Err(GuiInputError::Capacity);
                }
                let identity = target.control.record.target;
                if context
                    .pointers
                    .values()
                    .any(|active| active.pressed && active.target.control.record.target == identity)
                {
                    return Ok(GuiRoutingDisposition::Blocked);
                }
                if !matches!(
                    target.control.record.kind,
                    GuiControlKind::ScrollView | GuiControlKind::VirtualList
                ) {
                    self.focus(host, context, target.clone(), false, delivery)?;
                }
                let (command, lease) = self.feedback(
                    host,
                    context,
                    &target,
                    pointer,
                    None,
                    GuiInteractionUpdate::Hover(true),
                    delivery,
                )?;
                context.pointers.insert(
                    pointer,
                    Pointer {
                        target: target.clone(),
                        lease: lease.clone(),
                        pressed: true,
                        tap: true,
                        drag: None,
                    },
                );
                enqueue(host, context.queue_owner, identity, command)?;
                let (command, _) = self.feedback(
                    host,
                    context,
                    &target,
                    pointer,
                    Some(lease.clone()),
                    GuiInteractionUpdate::Press,
                    delivery,
                )?;
                enqueue(host, context.queue_owner, identity, command)?;
                let (command, _) = self.feedback(
                    host,
                    context,
                    &target,
                    pointer,
                    Some(lease.clone()),
                    GuiInteractionUpdate::Capture,
                    delivery,
                )?;
                enqueue(host, context.queue_owner, identity, command)?;
                self.service
                    .set_pointer_capture(&context.context, pointer, Some(identity))?;
                let drag = self.start_drag(host, context, &target, query, point, delivery)?;
                context
                    .pointers
                    .get_mut(&pointer)
                    .expect("retained pointer candidate")
                    .drag = drag;
                Ok(GuiRoutingDisposition::Routed {
                    target: identity,
                })
            }
            GuiPhysicalInput::PointerMove {
                pointer,
                point,
            } => {
                if let Some(mut active) = context.pointers.get(&pointer).cloned() {
                    let identity = active.target.control.record.target;
                    if active.pressed {
                        match active.drag.clone() {
                            Some(Drag::Content {
                                targets,
                                origin,
                                last,
                                active: scrolling,
                            }) => {
                                let targets = targets
                                    .iter()
                                    .map(|target| refresh(host, view, target))
                                    .collect::<Result<Vec<_>, _>>()?;
                                let travel = [point[0] - origin[0], point[1] - origin[1]];
                                let axis = usize::from(travel[1].abs() >= travel[0].abs());
                                let winner = if scrolling {
                                    Some(0)
                                } else if travel[0].hypot(travel[1]) > 0.01 {
                                    targets.iter().position(|target| {
                                        target
                                            .control
                                            .record
                                            .scroll()
                                            .is_some_and(|(_, capacity)| capacity[axis] > 0.0)
                                    })
                                } else {
                                    None
                                };
                                if let Some(winner) = winner {
                                    if active.tap {
                                        self.disarm_tap(
                                            host,
                                            context,
                                            pointer,
                                            &mut active,
                                            delivery,
                                        )?;
                                    }
                                    let targets = targets[winner..].to_vec();
                                    let before = local_point(host, query, &targets[0], last)?;
                                    let after = local_point(host, query, &targets[0], point)?;
                                    self.scroll(
                                        host,
                                        context,
                                        &targets,
                                        std::array::from_fn(|axis| before[axis] - after[axis]),
                                        Some(pointer),
                                        delivery,
                                    )?;
                                    active.drag = Some(Drag::Content {
                                        targets,
                                        origin,
                                        last: point,
                                        active: true,
                                    });
                                }
                            }
                            Some(Drag::Text) => self.caret(
                                host,
                                context,
                                &active.target,
                                query,
                                point,
                                true,
                                delivery,
                            )?,
                            Some(Drag::Slider(offset)) => self.slide(
                                host,
                                context,
                                &active.target,
                                query,
                                point,
                                offset,
                                delivery,
                            )?,
                            Some(Drag::Scroll {
                                axis,
                                grab,
                            }) => {
                                let local = local_point(host, query, &active.target, point)?;
                                let bar = active.target.control.scroll_bars[axis]
                                    .ok_or(GuiInputError::Unavailable)?;
                                let offset = bar.offset_for_thumb(local[axis] - grab);
                                let input = self.input(host, context, &active.target, delivery)?;
                                enqueue(
                                    host,
                                    context.queue_owner,
                                    identity,
                                    GuiLocalCommand::scroll_axis(input, axis, offset)?,
                                )?;
                                let mut delta = [0.0; 2];
                                delta[axis] = offset - scroll_offset(&active.target)[axis];
                                self.disarm_scrolled_taps(
                                    host,
                                    context,
                                    std::slice::from_ref(&active.target),
                                    delta,
                                    Some(pointer),
                                    delivery,
                                )?;
                            }
                            None => {}
                        }
                        if active.tap
                            && matches!(
                                active.target.control.record.kind,
                                GuiControlKind::Button | GuiControlKind::Checkbox
                            )
                            && !hit(host, query, point, &context.blockers)?.is_some_and(|target| {
                                target.control.record.target == identity
                                    && target.path == active.target.path
                            })
                        {
                            self.disarm_tap(host, context, pointer, &mut active, delivery)?;
                        }
                        context.pointers.insert(pointer, active);
                        return Ok(GuiRoutingDisposition::Routed {
                            target: identity,
                        });
                    }
                    context.pointers.insert(pointer, active);
                }
                let target = hit(host, query, point, &context.blockers)?;
                if context
                    .pointers
                    .get(&pointer)
                    .map(|active| active.target.control.record.target)
                    != target.as_ref().map(|target| target.control.record.target)
                {
                    self.leave_hover(host, context, pointer, delivery)?;
                    if let Some(target) = target {
                        if context.pointers.len() >= GUI_INPUT_MAX_POINTERS {
                            return Err(GuiInputError::Capacity);
                        }
                        let identity = target.control.record.target;
                        let (command, lease) = self.feedback(
                            host,
                            context,
                            &target,
                            pointer,
                            None,
                            GuiInteractionUpdate::Hover(true),
                            delivery,
                        )?;
                        context.pointers.insert(
                            pointer,
                            Pointer {
                                target,
                                lease,
                                pressed: false,
                                tap: false,
                                drag: None,
                            },
                        );
                        enqueue(host, context.queue_owner, identity, command)?;
                        return Ok(GuiRoutingDisposition::Routed {
                            target: identity,
                        });
                    }
                } else if let Some(target) = target
                    && let Some(active) = context.pointers.get(&pointer).cloned()
                    && interaction_part(&active.target.part) != interaction_part(&target.part)
                {
                    // Moving between a control's body and its scroll bar parts keeps
                    // the hover and names the part now under the pointer.
                    let identity = target.control.record.target;
                    let (command, _) = self.feedback(
                        host,
                        context,
                        &target,
                        pointer,
                        Some(active.lease.clone()),
                        GuiInteractionUpdate::Hover(true),
                        delivery,
                    )?;
                    context
                        .pointers
                        .get_mut(&pointer)
                        .expect("hovering pointer")
                        .target = target;
                    enqueue(host, context.queue_owner, identity, command)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: identity,
                    });
                }
                Ok(GuiRoutingDisposition::Unhandled)
            }
            GuiPhysicalInput::PointerUp {
                pointer,
                point,
                ..
            } => {
                let Some(active) = context.pointers.get(&pointer).cloned() else {
                    return Ok(GuiRoutingDisposition::Unhandled);
                };
                let identity = active.target.control.record.target;
                let same = hit(host, query, point, &context.blockers)?.is_some_and(|target| {
                    target.control.record.target == identity && target.path == active.target.path
                });
                let (command, _) = self.feedback(
                    host,
                    context,
                    &active.target,
                    pointer,
                    Some(active.lease.clone()),
                    GuiInteractionUpdate::Release,
                    delivery,
                )?;
                enqueue(host, context.queue_owner, identity, command)?;
                if active.pressed && active.tap && same {
                    let action = match active.target.control.record.kind {
                        GuiControlKind::Button => Some(GuiLocalAction::Press),
                        GuiControlKind::Checkbox => Some(GuiLocalAction::Toggle),
                        _ => None,
                    };
                    if let Some(action) = action {
                        self.action(host, context, &active.target, action, delivery)?;
                    }
                }
                let (command, _) = self.feedback(
                    host,
                    context,
                    &active.target,
                    pointer,
                    Some(active.lease),
                    GuiInteractionUpdate::Cancel,
                    delivery,
                )?;
                enqueue(host, context.queue_owner, identity, command)?;
                self.service
                    .set_pointer_capture(&context.context, pointer, None)?;
                context.pointers.remove(&pointer);
                Ok(GuiRoutingDisposition::Routed {
                    target: identity,
                })
            }
            GuiPhysicalInput::PointerCancel {
                pointer,
            } => {
                self.cancel_pointer(host, context, pointer, delivery)?;
                Ok(GuiRoutingDisposition::Unhandled)
            }
            GuiPhysicalInput::Blur
            | GuiPhysicalInput::Key {
                key: GuiPhysicalKey::Escape,
            } => {
                while let Some(pointer) = context.pointers.keys().next().copied() {
                    self.cancel_pointer(host, context, pointer, delivery)?;
                }
                if let Some(focus) = context.focus.take() {
                    self.action(host, context, &focus, GuiLocalAction::Blur, delivery)?;
                }
                self.service.set_native_focus(&context.context, None)?;
                Ok(GuiRoutingDisposition::Unhandled)
            }
            GuiPhysicalInput::Wheel {
                point,
                delta,
            } => {
                let targets = scroll_targets(host, query, point, &context.blockers)?;
                let Some(first) = targets.first() else {
                    return Ok(GuiRoutingDisposition::Unhandled);
                };
                let identity = first.control.record.target;
                self.scroll(host, context, &targets, delta, None, delivery)?;
                Ok(GuiRoutingDisposition::Routed {
                    target: identity,
                })
            }
            GuiPhysicalInput::Key {
                key,
            } => {
                if matches!(key, GuiPhysicalKey::Tab | GuiPhysicalKey::BackTab) {
                    let candidates = super::keyboard::targets(host, view, context.focus.as_ref())?;
                    if candidates.is_empty() {
                        return Ok(GuiRoutingDisposition::Unhandled);
                    }
                    let current = context.focus.as_ref().and_then(|focus| {
                        candidates.iter().position(|candidate| {
                            candidate.control.record.target == focus.control.record.target
                                && candidate.path == focus.path
                        })
                    });
                    let index = match (current, key) {
                        (Some(index), GuiPhysicalKey::BackTab) => {
                            (index + candidates.len() - 1) % candidates.len()
                        }
                        (Some(index), _) => (index + 1) % candidates.len(),
                        (None, GuiPhysicalKey::BackTab) => {
                            candidates
                                .iter()
                                .take_while(|candidate| candidate.path == candidates[0].path)
                                .count()
                                - 1
                        }
                        (None, _) => 0,
                    };
                    let target = candidates[index].clone();
                    let identity = target.control.record.target;
                    self.focus(host, context, target, true, delivery)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: identity,
                    });
                }
                let Some(target) = context.focus.clone() else {
                    return Ok(GuiRoutingDisposition::Unhandled);
                };
                self.focus(host, context, target.clone(), true, delivery)?;
                if target.control.record.kind == GuiControlKind::Slider
                    && let Some(steps) = match key {
                        GuiPhysicalKey::Left | GuiPhysicalKey::Down => Some(-1.0),
                        GuiPhysicalKey::Right | GuiPhysicalKey::Up => Some(1.0),
                        _ => None,
                    }
                {
                    let input = self.input(host, context, &target, delivery)?;
                    enqueue(
                        host,
                        context.queue_owner,
                        target.control.record.target,
                        GuiLocalCommand::slider_step(input, steps)?,
                    )?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: target.control.record.target,
                    });
                }
                let action = match (key, target.control.record.kind) {
                    (GuiPhysicalKey::Enter | GuiPhysicalKey::Space, GuiControlKind::Button) => {
                        GuiLocalAction::Press
                    }
                    (GuiPhysicalKey::Space, GuiControlKind::Checkbox) => GuiLocalAction::Toggle,
                    (GuiPhysicalKey::Enter, GuiControlKind::TextInput) => GuiLocalAction::Submit,
                    (GuiPhysicalKey::Home, GuiControlKind::Slider) => GuiLocalAction::SetScalar(
                        target.control.slider.ok_or(GuiInputError::Unavailable)?.min,
                    ),
                    (GuiPhysicalKey::End, GuiControlKind::Slider) => GuiLocalAction::SetScalar(
                        target.control.slider.ok_or(GuiInputError::Unavailable)?.max,
                    ),
                    _ => return Ok(GuiRoutingDisposition::Unhandled),
                };
                self.action(host, context, &target, action, delivery)?;
                Ok(GuiRoutingDisposition::Routed {
                    target: target.control.record.target,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scroll(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        targets: &[Target],
        delta: [f32; 2],
        scrolling: Option<u64>,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let chain = GuiScrollChain::new(delta)?;
        let mut prepared = Vec::new();
        for target in targets {
            let input = self.input(host, context, target, delivery)?;
            let command = GuiLocalCommand::scroll(input, chain.clone())?;
            prepared
                .try_reserve_exact(1)
                .map_err(|_| GuiInputError::Capacity)?;
            prepared.push(command);
        }
        for (target, command) in targets.iter().zip(prepared) {
            enqueue(
                host,
                context.queue_owner,
                target.control.record.target,
                command,
            )?;
        }
        chain.seal();
        context.scroll = Some(chain);
        self.disarm_scrolled_taps(host, context, targets, delta, scrolling, delivery)
    }

    /// A scroll that moves content disarms the held button and checkbox taps of
    /// every panel in its chain, so their releases activate nothing. Slider and
    /// text presses own their drags and survive, as does the scrolling pointer.
    #[allow(clippy::too_many_arguments)]
    fn disarm_scrolled_taps(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        targets: &[Target],
        delta: [f32; 2],
        scrolling: Option<u64>,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if !targets.iter().any(|target| scroll_moves(target, delta)) {
            return Ok(());
        }
        let held: Vec<u64> = context
            .pointers
            .iter()
            .filter(|(pointer, active)| {
                Some(**pointer) != scrolling
                    && active.pressed
                    && active.tap
                    && matches!(
                        active.target.control.record.kind,
                        GuiControlKind::Button | GuiControlKind::Checkbox
                    )
                    && targets.iter().any(|target| {
                        target.control.record.target.world
                            == active.target.control.record.target.world
                            && target.path == active.target.path
                    })
            })
            .map(|(pointer, _)| *pointer)
            .collect();
        for pointer in held {
            let mut active = context.pointers[&pointer].clone();
            self.disarm_tap(host, context, pointer, &mut active, delivery)?;
            context.pointers.insert(pointer, active);
        }
        Ok(())
    }

    fn input(
        &mut self,
        host: &HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<super::super::GuiInputCommand, GuiInputError> {
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(GuiInputError::Capacity)?;
        let permit = delivery.command()?;
        let identity = target.control.record.target;
        *context
            .queued_worlds
            .borrow_mut()
            .entry(identity.world)
            .or_default() += 1;
        let permit: Box<dyn GuiDeliveryPermit> = Box::new(DispatchPermit {
            inner: Some(permit),
            worlds: context.queued_worlds.clone(),
            world: identity.world,
        });
        self.service
            .reserve_routed_source(
                host,
                &context.context,
                identity,
                self.next_request,
                &target.path,
                Some(target.source),
                permit,
            )
            .map_err(|error| {
                let (reason, permit) = error.into_parts();
                permit.settle(GuiDeliveryTerminal::Rejected(reason));
                reason
            })
    }

    fn action(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        action: GuiLocalAction,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::routed(input, action)?,
        )
    }

    fn focus(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: Target,
        visible: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if context
            .focus
            .as_ref()
            .is_some_and(|focus| focus.control.record.target != target.control.record.target)
        {
            let previous = context.focus.take().expect("previous focus");
            self.action(host, context, &previous, GuiLocalAction::Blur, delivery)?;
        }
        let input = self.input(host, context, &target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::focus(input, visible)?,
        )?;
        context.focus = Some(target);
        self.service.set_native_focus(
            &context.context,
            context
                .focus
                .as_ref()
                .map(|focus| focus.control.record.target),
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn feedback(
        &mut self,
        host: &HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        pointer: u64,
        lease: Option<GuiPointerLease>,
        update: GuiInteractionUpdate,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(GuiLocalCommand, GuiPointerLease), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        let lease = match lease {
            Some(lease) => lease,
            None => self.service.pointer_lease(&input, pointer)?,
        };
        Ok((
            GuiLocalCommand::part_interaction(
                input,
                lease.clone(),
                update,
                interaction_part(&target.part),
            )?,
            lease,
        ))
    }

    fn cancel_pointer(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        pointer: u64,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if let Some(active) = context.pointers.remove(&pointer) {
            let result = self.feedback(
                host,
                context,
                &active.target,
                pointer,
                Some(active.lease.clone()),
                GuiInteractionUpdate::Cancel,
                delivery,
            );
            match result {
                Ok((command, _)) => {
                    if let Err(error) = enqueue(
                        host,
                        context.queue_owner,
                        active.target.control.record.target,
                        command,
                    ) {
                        self.service.cancel_pointer_lease(&active.lease)?;
                        return Err(error);
                    }
                }
                Err(error) => {
                    self.service.cancel_pointer_lease(&active.lease)?;
                    return Err(error);
                }
            }
            self.service
                .set_pointer_capture(&context.context, pointer, None)?;
        }
        Ok(())
    }

    /// End an unpressed pointer's hover before it moves elsewhere. A hover
    /// whose feedback has not been applied yet only revokes its candidate
    /// lease: the queued Hover then fails validation instead of lighting the
    /// old control, so a burst of moves between mutation boundaries queues one
    /// command per move rather than a Hover and a Cancel.
    fn leave_hover(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        pointer: u64,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        match context.pointers.get(&pointer) {
            Some(active) if !active.lease.is_live() => {
                let active = context.pointers.remove(&pointer).expect("hovering pointer");
                self.service.cancel_pointer_lease(&active.lease)
            }
            _ => self.cancel_pointer(host, context, pointer, delivery),
        }
    }

    fn start_drag(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<Option<Drag>, GuiInputError> {
        let local = local_point(host, query, target, point)?;
        if target.control.record.kind == GuiControlKind::TextInput {
            self.caret(host, context, target, query, point, false, delivery)?;
            return Ok(Some(Drag::Text));
        }
        if target.control.record.kind == GuiControlKind::Slider {
            let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
            let rect = slider.thumb_rect;
            let grabbing = (0..2)
                .all(|axis| local[axis] >= rect[axis] && local[axis] < rect[axis] + rect[axis + 2]);
            let offset = if grabbing {
                local[0] - rect[0] - rect[2] * 0.5
            } else {
                0.0
            };
            if !grabbing {
                self.slide(host, context, target, query, point, offset, delivery)?;
            }
            return Ok(Some(Drag::Slider(offset)));
        }
        let (axis, thumb) = match target.part {
            CanvasHitKind::ScrollTrack {
                axis,
            } => (axis, false),
            CanvasHitKind::ScrollThumb {
                axis,
            } => (axis, true),
            _ => {
                let targets = scroll_targets(host, query, point, &context.blockers)?;
                return Ok((!targets.is_empty()).then_some(Drag::Content {
                    targets,
                    origin: point,
                    last: point,
                    active: false,
                }));
            }
        };
        let axis = match axis {
            CanvasAxis::Horizontal => 0,
            CanvasAxis::Vertical => 1,
        };
        let bar = target.control.scroll_bars[axis].ok_or(GuiInputError::Unavailable)?;
        if thumb {
            return Ok(Some(Drag::Scroll {
                axis,
                grab: local[axis] - bar.thumb_start(),
            }));
        }
        let mut delta = [0.0; 2];
        delta[axis] = if local[axis] < bar.thumb_start() {
            -bar.page
        } else {
            bar.page
        };
        self.scroll(
            host,
            context,
            std::slice::from_ref(target),
            delta,
            None,
            delivery,
        )?;
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn disarm_tap(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        pointer: u64,
        active: &mut Pointer,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        for update in [
            GuiInteractionUpdate::Release,
            GuiInteractionUpdate::Hover(false),
        ] {
            let (command, _) = self.feedback(
                host,
                context,
                &active.target,
                pointer,
                Some(active.lease.clone()),
                update,
                delivery,
            )?;
            enqueue(
                host,
                context.queue_owner,
                active.target.control.record.target,
                command,
            )?;
        }
        active.tap = false;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn caret(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        extend: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let (layout, origin) = target
            .control
            .text
            .as_ref()
            .ok_or(GuiInputError::Unavailable)?;
        let point = local_point(host, query, target, point)?;
        let offset = crate::systems::gui::local::text_edit::caret_offset_at_x(
            layout,
            (point[0] - origin[0]) / layout.font_size,
        );
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::text_caret(input, offset, extend)?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn slide(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        offset: f32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
        let local = local_point(host, query, target, point)?[0] - offset;
        let span = slider.thumb_centers[1] - slider.thumb_centers[0];
        let fraction = if span > 0.0 {
            ((local - slider.thumb_centers[0]) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let crate::systems::gui::presentation::GuiRoutingValue::Scalar(current) =
            target.control.record.value
        else {
            return Err(GuiInputError::Unavailable);
        };
        let value = crate::systems::gui::local::slider::value_at(
            slider.min,
            slider.max,
            slider.step,
            fraction,
            current,
        )
        .ok_or(GuiInputError::Unavailable)?;
        self.action(
            host,
            context,
            target,
            GuiLocalAction::SetScalar(value),
            delivery,
        )
    }
}

pub(super) fn live(host: &HostRuntime, world: crate::WorldRef) -> Result<(), GuiInputError> {
    if host.world_ref(world.id()) == Some(world) {
        Ok(())
    } else {
        Err(GuiInputError::Unavailable)
    }
}

fn enqueue(
    host: &mut HostRuntime,
    owner: u64,
    target: GuiEntityTarget,
    command: GuiLocalCommand,
) -> Result<(), GuiInputError> {
    host.world_mut(target.world.id())
        .ok_or(GuiInputError::Unavailable)?
        .enqueue_system_command(GuiSystem::ID, owner, command)
        .map_err(|_| GuiInputError::Unavailable)
}

struct DispatchPermit {
    inner: Option<Box<dyn GuiDeliveryPermit>>,
    worlds: Rc<RefCell<BTreeMap<crate::WorldRef, usize>>>,
    world: crate::WorldRef,
}

impl GuiDeliveryPermit for DispatchPermit {
    fn prepare_native(
        &mut self,
        state: &crate::systems::gui::local::GuiNativeTextState,
    ) -> Result<(), super::super::GuiDeliveryError> {
        self.inner
            .as_mut()
            .expect("live dispatch permit")
            .prepare_native(state)
    }

    fn prepare(
        &mut self,
        effect: Option<&crate::systems::gui::local::GuiLocalEffect>,
    ) -> Result<(), super::super::GuiDeliveryError> {
        self.inner
            .as_mut()
            .expect("live dispatch permit")
            .prepare(effect)
    }

    fn settle(mut self: Box<Self>, terminal: GuiDeliveryTerminal) {
        self.inner
            .take()
            .expect("unsettled dispatch permit")
            .settle(terminal);
    }
}

impl Drop for DispatchPermit {
    fn drop(&mut self) {
        let mut worlds = self.worlds.borrow_mut();
        let count = worlds.get_mut(&self.world).expect("pending dispatch World");
        *count -= 1;
        if *count == 0 {
            worlds.remove(&self.world);
        }
    }
}

fn refresh(
    host: &HostRuntime,
    view: ViewDescriptor,
    target: &Target,
) -> Result<Target, GuiInputError> {
    let mut publication = host
        .publication(view.publication)
        .ok_or(GuiInputError::StalePath)?;
    let mut output = view.output;
    for token in &target.path {
        let edge = publication
            .attachments
            .iter()
            .find(|edge| edge.token == *token)
            .ok_or(GuiInputError::StalePath)?;
        publication = host
            .attached_publication(edge)
            .ok_or(GuiInputError::StalePath)?;
        if edge.mode != crate::WorldAttachmentMode::Spatial {
            output = edge.output.ok_or(GuiInputError::StalePath)?;
        }
    }
    // The path is still live, so a missing Canvas scope or control means the
    // target itself was removed: its input is reported unavailable (gui.md).
    let control = publication
        .chunk(CanvasSystem::ID)
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .and_then(|gui| gui.views.get(&output))
        .and_then(|view| {
            view.controls
                .iter()
                .find(|control| control.record.target == target.control.record.target)
        })
        .ok_or(GuiInputError::Unavailable)?;
    Ok(Target {
        control: control.clone(),
        path: target.path.clone(),
        source: view.publication,
        part: target.part.clone(),
    })
}

fn current_target(
    host: &HostRuntime,
    view: ViewDescriptor,
    target: &Target,
) -> Result<(), GuiInputError> {
    live(host, view.output.world())?;
    live(host, target.control.record.target.world)?;
    for token in &target.path {
        live(host, token.parent())?;
    }
    let current = refresh(host, view, target)?;
    if !current.control.available
        || !current.control.hit.eligible
        || host
            .world_fault(current.control.record.target.world)
            .ok()
            .flatten()
            .is_some()
    {
        return Err(GuiInputError::Unavailable);
    }
    Ok(())
}

fn picking_blocked(
    host: &HostRuntime,
    query: ViewQueryTarget,
    input: &GuiPhysicalInput,
    context: &GuiRoutingContext,
) -> Result<bool, GuiInputError> {
    let blockers = &context.blockers;
    if blockers.is_empty() {
        return Ok(false);
    }
    let point = match input {
        GuiPhysicalInput::PointerDown {
            point,
            ..
        }
        | GuiPhysicalInput::Wheel {
            point,
            ..
        } => *point,
        GuiPhysicalInput::PointerUp {
            point,
            pointer,
            ..
        } if !context
            .pointers
            .get(pointer)
            .is_some_and(|active| active.pressed) =>
        {
            *point
        }
        _ => return Ok(false),
    };
    let result = query_composed_input(
        host,
        query,
        point,
        GuiQueryOptions {
            blockers,
        },
    )
    .map_err(|_| GuiInputError::StaleContext)?;
    Ok(matches!(
        result.outcome,
        GuiQueryOutcome::Blocked {
            reason: super::super::query::GuiQueryBlockReason::PickingGeometry,
            ..
        }
    ))
}

/// What a point selects: an eligible control, panel content without one, or
/// nothing that stops the input from reaching the scene.
enum Located {
    Target(Target),
    Panel,
    Miss,
}

fn hit(
    host: &HostRuntime,
    query: ViewQueryTarget,
    point: [f32; 2],
    blockers: &[super::super::query::GuiPickingBlocker],
) -> Result<Option<Target>, GuiInputError> {
    Ok(match locate(host, query, point, blockers)? {
        Located::Target(target) => Some(target),
        Located::Panel | Located::Miss => None,
    })
}

fn locate(
    host: &HostRuntime,
    query: ViewQueryTarget,
    point: [f32; 2],
    blockers: &[super::super::query::GuiPickingBlocker],
) -> Result<Located, GuiInputError> {
    if point.iter().any(|coordinate| !coordinate.is_finite()) {
        return Err(GuiInputError::Unavailable);
    }
    if point
        .iter()
        .any(|coordinate| !(0.0..=1.0).contains(coordinate))
    {
        return Ok(Located::Miss);
    }
    let result = query_composed_input(
        host,
        query,
        point,
        GuiQueryOptions {
            blockers,
        },
    )
    .map_err(|_| GuiInputError::StaleContext)?;
    match result.outcome {
        GuiQueryOutcome::Hit(hit) => {
            let publication = host
                .publication(hit.publication)
                .ok_or(GuiInputError::StalePath)?;
            let gui = publication
                .chunk(CanvasSystem::ID)
                .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
                .and_then(|gui| gui.views.get(&hit.output));
            let control = gui.and_then(|gui| {
                hit.hit.ancestry.iter().rev().find_map(|entity| {
                    gui.controls.iter().find(|control| {
                        control.record.target.entity == *entity
                            && control.available
                            && control.hit.eligible
                    })
                })
            });
            Ok(match control {
                Some(control) => Located::Target(Target {
                    control: control.clone(),
                    path: hit.path.iter().map(|step| step.token.clone()).collect(),
                    source: result.view.publication,
                    part: hit.hit.kind.clone(),
                }),
                // An ineligible control on a panel is still panel content.
                None if hit.path.iter().any(|step| step.distance.is_some()) => Located::Panel,
                None => Located::Miss,
            })
        }
        GuiQueryOutcome::Miss => Ok(Located::Miss),
        GuiQueryOutcome::Blocked {
            reason: super::super::query::GuiQueryBlockReason::Panel,
            ..
        } => Ok(Located::Panel),
        GuiQueryOutcome::Blocked {
            reason: super::super::query::GuiQueryBlockReason::PickingGeometry,
            ..
        } => Ok(Located::Miss),
        GuiQueryOutcome::Blocked {
            ..
        }
        | GuiQueryOutcome::Unavailable(_) => Err(GuiInputError::Unavailable),
    }
}

fn scroll_targets(
    host: &HostRuntime,
    query: ViewQueryTarget,
    point: [f32; 2],
    blockers: &[super::super::query::GuiPickingBlocker],
) -> Result<Vec<Target>, GuiInputError> {
    let result = query_composed_input(
        host,
        query,
        point,
        GuiQueryOptions {
            blockers,
        },
    )
    .map_err(|_| GuiInputError::StaleContext)?;
    let hit = match result.outcome {
        GuiQueryOutcome::Hit(hit) => hit,
        // Panel content without a scroll target leaves the wheel unconsumed.
        GuiQueryOutcome::Miss
        | GuiQueryOutcome::Blocked {
            reason: super::super::query::GuiQueryBlockReason::Panel,
            ..
        } => return Ok(Vec::new()),
        _ => return Err(GuiInputError::Unavailable),
    };
    let mut targets = Vec::new();
    let mut path: Vec<_> = hit.path.iter().map(|step| step.token.clone()).collect();
    append_scroll_targets(
        host,
        hit.publication,
        hit.output,
        &hit.hit.ancestry,
        &path,
        result.view.publication,
        &mut targets,
    );
    for step in hit.path.iter().rev() {
        path.pop();
        let publication = host
            .publication(step.publication)
            .ok_or(GuiInputError::StalePath)?;
        let edge = publication
            .attachments
            .iter()
            .find(|edge| edge.token == step.token)
            .ok_or(GuiInputError::StalePath)?;
        if let Some(output) = edge.placement_output
            && let Some(canvas) = publication.output(output).and_then(|chunk| chunk.data::<crate::systems::canvas::CanvasPublication>())
            && let Some(anchor) = canvas.hits.iter().find(|hit| matches!(&hit.kind, crate::systems::canvas::CanvasHitKind::Attachment { token, .. } if *token == edge.token)) {
            append_scroll_targets(host, publication.id, output, &anchor.ancestry, &path, result.view.publication, &mut targets);
        }
    }
    Ok(targets)
}

#[allow(clippy::too_many_arguments)]
fn append_scroll_targets(
    host: &HostRuntime,
    source: WorldPublicationId,
    output: crate::OutputRef,
    ancestry: &[crate::EntityId],
    path: &[WorldAttachmentToken],
    root: WorldPublicationId,
    targets: &mut Vec<Target>,
) {
    let Some(gui) = host
        .publication(source)
        .and_then(|publication| publication.chunk(CanvasSystem::ID))
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .and_then(|gui| gui.views.get(&output))
    else {
        return;
    };
    for entity in ancestry.iter().rev() {
        if let Some(control) = gui.controls.iter().find(|control| {
            control.record.target.entity == *entity
                && control.hit.eligible
                && control.available
                && matches!(
                    control.record.kind,
                    GuiControlKind::ScrollView | GuiControlKind::VirtualList
                )
        }) {
            targets.push(Target {
                control: control.clone(),
                path: path.to_vec(),
                source: root,
                part: CanvasHitKind::Entity,
            });
        }
    }
}

/// The control part a pointer names for skin feedback.
fn interaction_part(part: &CanvasHitKind) -> GuiInteractionPart {
    let axis = |axis: &CanvasAxis| usize::from(*axis == CanvasAxis::Vertical);
    match part {
        CanvasHitKind::ScrollTrack {
            axis: bar,
        } => GuiInteractionPart::ScrollTrack(axis(bar)),
        CanvasHitKind::ScrollThumb {
            axis: bar,
        } => GuiInteractionPart::ScrollThumb(axis(bar)),
        _ => GuiInteractionPart::Control,
    }
}

fn scroll_offset(target: &Target) -> [f32; 2] {
    target
        .control
        .record
        .scroll()
        .map_or([0.0; 2], |(offset, _)| offset)
}

/// Whether the completed scroll state leaves room to move along `delta`.
fn scroll_moves(target: &Target, delta: [f32; 2]) -> bool {
    let Some((offset, capacity)) = target.control.record.scroll() else {
        return false;
    };
    (0..2).any(|axis| {
        (delta[axis] > 0.0 && offset[axis] < capacity[axis])
            || (delta[axis] < 0.0 && offset[axis] > 0.0)
    })
}

fn local_point(
    host: &HostRuntime,
    query: ViewQueryTarget,
    target: &Target,
    point: [f32; 2],
) -> Result<[f32; 2], GuiInputError> {
    let projected = project_composed_point(host, query, &target.path, point, true)
        .map_err(|_| GuiInputError::StalePath)?
        .ok_or(GuiInputError::Unavailable)?;
    let hit = &target.control.hit;
    Ok(std::array::from_fn(|axis| {
        (projected.point[axis] - hit.position[axis]) / hit.scale[axis]
    }))
}
