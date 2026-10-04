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
use crate::systems::gui::local::slider::GuiDialDrag;
use crate::systems::gui::local::{
    GuiControlKind, GuiInteractionPart, GuiInteractionUpdate, GuiLocalAction, GuiLocalCommand,
    GuiNumberStep, GuiScrollChain, GuiTextEdit,
};
use crate::systems::gui::presentation::{
    GuiCanvasPublication, GuiCanvasSemanticView, GuiControlObservation,
};
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
    /// Completed provider lifetimes, parallel to path; value edits preserve these fences.
    pub(super) surface_lifetimes: Vec<Option<(u16, u64)>>,
    pub(super) source: WorldPublicationId,
    pub(super) part: CanvasHitKind,
    /// The focus part of a control with several, such as a range's thumb,
    /// that this target names: the focused one, or the one a pointer is
    /// over, presses or drags. None for a control with one part, whose focus
    /// names part 0, and for a pointer over none of a control's parts.
    pub(super) focus_part: Option<u32>,
    /// The step part of a numeric text input a pointer is over or holds;
    /// step parts are no focus parts.
    pub(super) step: Option<GuiNumberStep>,
}

impl Target {
    /// The focus part this target names; 0 for a control with one.
    pub(super) fn focus_part(&self) -> u32 {
        self.focus_part.unwrap_or(0)
    }

    /// Whether `other` names the same control through the same path and the
    /// same focus part.
    pub(super) fn same_stop(&self, other: &Target) -> bool {
        self.control.record.target == other.control.record.target
            && self.path == other.path
            && self.focus_part() == other.focus_part()
    }
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
    /// A dial's relative drag.
    Dial(GuiDialDrag),
    /// A drag on a colour control's surface, the focus part it set.
    Color(u32),
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
    pub(super) context: GuiInputContext,
    pointers: BTreeMap<u64, Pointer>,
    /// The keyboard target: the logical focus of the Worlds this context
    /// presents, however it was set.
    pub(super) focus: Option<Target>,
    /// The item this context last made its group's active item, until its
    /// World settles this context's routed work; the GUI System holds the
    /// active item.
    pub(super) active: Option<Target>,
    /// Whether this context routed an edit of the focused numeric text
    /// input's text since it last committed, discarded or left it: Escape's
    /// guide while its World has not applied this context's work, after
    /// which the GUI System's own record decides.
    pub(super) number_edit: bool,
    scroll: Option<GuiScrollChain>,
    blockers: Vec<super::super::query::GuiPickingBlocker>,
    pub(super) queue_owner: u64,
    /// Routed commands not yet settled, by target World.
    pub(super) queued_worlds: Rc<RefCell<BTreeMap<crate::WorldRef, usize>>>,
}

impl GuiRoutingContext {
    /// The explicit picking blockers this context routes against.
    pub(super) fn blockers(&self) -> &[super::super::query::GuiPickingBlocker] {
        &self.blockers
    }

    /// The attachment path of a control a pointer of this context hovers.
    pub(super) fn hovered_path(&self) -> Option<Vec<WorldAttachmentToken>> {
        self.pointers
            .values()
            .find(|active| !active.pressed)
            .or_else(|| self.pointers.values().next())
            .map(|active| active.target.path.clone())
    }
}

/// Host-owned router shared by physical adapters, not a World evaluator.
#[derive(Default)]
pub struct GuiInputRouter {
    pub(super) service: GuiInputService,
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

    /// Cancel stale physical ownership after the Host frame, including idle
    /// input, and make logical focus a client command set in a presented World
    /// this context's keyboard target. Completed policy plus the current
    /// scheduling domain determine eligibility; adopting focus queues ordinary
    /// routed commands, and no extra World evaluation occurs here.
    pub fn synchronize(
        &mut self,
        host: &mut HostRuntime,
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
        if let Some(view) = view
            && self.follow_logical_focus(host, context, view).is_err()
        {
            context.focus = None;
        }
        if context.focus.is_none() && context.context.0.native.borrow().focus.is_some() {
            cancelled.focus = true;
            let _ = self.service.set_native_focus(&context.context, None);
        }

        // Once its World settled this context's work, the publication names
        // the active item.
        if context.active.as_ref().is_some_and(|active| {
            !context
                .queued_worlds
                .borrow()
                .contains_key(&active.control.record.target.world)
        }) {
            context.active = None;
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
                active: None,
                number_edit: false,
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
            context.active = None;
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

        // A press of any button outside an open light overlay closes it and
        // is swallowed, wherever it lands.
        if let GuiPhysicalInput::PointerDown {
            point,
            ..
        } = &input
            && self.dismiss_outside(host, context, view, query, *point)?
        {
            return Ok(GuiRoutingDisposition::Blocked);
        }
        if picking_blocked(host, query, &input, context)? {
            return Ok(GuiRoutingDisposition::Blocked);
        }

        // A secondary press is a context request, routed below; other presses
        // and releases of non-primary buttons only report what they hit.
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
            && !matches!(
                input,
                GuiPhysicalInput::PointerDown {
                    button: GuiPhysicalButton::Secondary,
                    ..
                }
            )
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
                if super::overlay::under_modal(host, view, &target)? {
                    return Ok(GuiRoutingDisposition::Blocked);
                }

                // Enter activates the active item of the group the input drives.
                if matches!(edit, crate::systems::gui::local::GuiTextEdit::Submit)
                    && let Some(disposition) = self.group_key(
                        host,
                        context,
                        view,
                        &target,
                        GuiPhysicalKey::Enter,
                        delivery,
                    )?
                {
                    return Ok(disposition);
                }
                if target.control.number.is_some()
                    && matches!(
                        edit,
                        GuiTextEdit::Insert(_)
                            | GuiTextEdit::Backspace
                            | GuiTextEdit::Delete
                            | GuiTextEdit::CommitComposition
                    )
                {
                    context.number_edit = true;
                }
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
                point,
                button: GuiPhysicalButton::Secondary,
                ..
            } => self.pointer_context(host, context, query, point, delivery),
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
                // A press on a range takes one thumb, which it focuses and drags.
                let target = self.pointed_part(host, context, query, target, point, true)?;
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
                // A press on a control that does not take focus leaves focus,
                // including a focused text input's, where it is, and so does
                // a press on a colour control's swatch, which is no part.
                if target.control.record.focusable
                    && !matches!(
                        target.control.record.kind,
                        GuiControlKind::ScrollView | GuiControlKind::VirtualList
                    )
                    && (target.control.color.is_none() || target.focus_part.is_some())
                {
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
                super::group::note_hover(context, &target);
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
                        // A captured ray can miss a curved shell. Keep ownership and
                        // the last drag coordinate until a valid continuation returns.
                        if project_composed_point(
                            host,
                            query,
                            &active.target.path,
                            point,
                            true,
                            active.target.control.hit.layer,
                        )
                        .map_err(|_| GuiInputError::StalePath)?
                        .is_none()
                        {
                            return Ok(GuiRoutingDisposition::Routed {
                                target: identity,
                            });
                        }
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
                            Some(Drag::Color(part)) => self.pick(
                                host,
                                context,
                                &active.target,
                                query,
                                point,
                                part,
                                delivery,
                            )?,
                            Some(Drag::Dial(drag)) => {
                                let turned = self.turn(
                                    host,
                                    context,
                                    &active.target,
                                    query,
                                    point,
                                    drag,
                                    delivery,
                                )?;
                                active.drag = Some(Drag::Dial(turned));
                            }
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
                let target = hit(host, query, point, &context.blockers)?
                    .map(|target| self.pointed_part(host, context, query, target, point, false))
                    .transpose()?;
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
                        super::group::note_hover(context, &target);
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
                    && interaction_part(&active.target) != interaction_part(&target)
                {
                    // Moving between a control's body, its scroll bar parts and
                    // its focus parts keeps the hover and names the part now
                    // under the pointer.
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
                ..
            } => {
                // Escape first discards a pending numeric edit, then closes
                // the topmost overlay that is not manual, and only then blurs.
                if matches!(input, GuiPhysicalInput::Key { .. }) {
                    if let Some(focus) = context.focus.clone()
                        && self.number_edit_pending(host, context, &focus)
                    {
                        self.focus(host, context, focus.clone(), true, delivery)?;
                        let input = self.input(host, context, &focus, delivery)?;
                        enqueue(
                            host,
                            context.queue_owner,
                            focus.control.record.target,
                            GuiLocalCommand::number_discard(input)?,
                        )?;
                        context.number_edit = false;
                        return Ok(GuiRoutingDisposition::Routed {
                            target: focus.control.record.target,
                        });
                    }
                    if let Some(disposition) = self.escape_overlay(host, context, view, delivery)? {
                        return Ok(disposition);
                    }
                }
                while let Some(pointer) = context.pointers.keys().next().copied() {
                    self.cancel_pointer(host, context, pointer, delivery)?;
                }
                if let Some(focus) = context.focus.take() {
                    self.blur(host, context, &focus, delivery)?;
                }
                self.service.set_native_focus(&context.context, None)?;
                Ok(GuiRoutingDisposition::Unhandled)
            }
            GuiPhysicalInput::Wheel {
                point,
                delta,
                shift,
            } => {
                // Over the focused value control the wheel steps its value,
                // so an unfocused one never takes the wheel from scrolling:
                // a slider's anywhere over it, a colour control's focused
                // rail only over that rail. Anything else, including a failed
                // hit, scrolls as before.
                if let Some(focus) = context
                    .focus
                    .clone()
                    .filter(|focus| focus.control.color.is_some() && focus.focus_part() > 0)
                    && let Some(steps) = wheel_steps(delta)
                    && let Some(over) = hit(host, query, point, &context.blockers).ok().flatten()
                    && over.control.record.target == focus.control.record.target
                    && over.path == focus.path
                    && self
                        .pointed_part(host, context, query, over, point, false)
                        .ok()
                        .and_then(|over| over.focus_part)
                        == Some(focus.focus_part())
                {
                    let channel = color_rail_channel(focus.focus_part());
                    self.color_step(host, context, &focus, channel, steps, shift, delivery)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: focus.control.record.target,
                    });
                }
                if let Some(focus) = context
                    .focus
                    .clone()
                    .filter(|focus| focus.control.steps_value())
                    && let Some(steps) = wheel_steps(delta)
                    && hit(host, query, point, &context.blockers)
                        .ok()
                        .flatten()
                        .is_some_and(|target| {
                            target.control.record.target == focus.control.record.target
                                && target.path == focus.path
                        })
                {
                    self.step_value(host, context, &focus, steps, shift, delivery)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: focus.control.record.target,
                    });
                }
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
                shift,
            } => {
                let key = match key {
                    GuiPhysicalKey::Tab if shift => GuiPhysicalKey::BackTab,
                    key => key,
                };
                if matches!(key, GuiPhysicalKey::Tab | GuiPhysicalKey::BackTab) {
                    let candidates = super::group::tab_stops(
                        super::keyboard::targets(host, view, context.focus.as_ref())?,
                        context.focus.as_ref(),
                    );
                    if candidates.is_empty() {
                        return Ok(GuiRoutingDisposition::Unhandled);
                    }
                    let current = context.focus.as_ref().and_then(|focus| {
                        candidates
                            .iter()
                            .position(|candidate| candidate.same_stop(focus))
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

                // An open modal overlay blocks keys to what lies under it.
                if super::overlay::under_modal(host, view, &target)? {
                    return Ok(GuiRoutingDisposition::Blocked);
                }
                self.focus(host, context, target.clone(), true, delivery)?;
                // The Menu key and Shift+F10 request a context at the
                // bottom-left corner of the focused control's visible box.
                if key == GuiPhysicalKey::ContextMenu || (key == GuiPhysicalKey::F10 && shift) {
                    let hit = &target.control.hit;
                    let corner = [
                        hit.bounds[0].max(hit.clip[0]),
                        hit.bounds[3].min(hit.clip[3]),
                    ];
                    self.request_context(host, context, &target, corner, delivery)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: target.control.record.target,
                    });
                }
                if let Some(disposition) =
                    self.group_key(host, context, view, &target, key, delivery)?
                {
                    return Ok(disposition);
                }
                // Up and Down step a numeric text input, by the fine step
                // with Shift, after committing its edit; Left and Right stay
                // with its text.
                if target.control.number.is_some()
                    && let Some(steps) = match key {
                        GuiPhysicalKey::Down => Some(-1.0),
                        GuiPhysicalKey::Up => Some(1.0),
                        _ => None,
                    }
                {
                    let input = self.input(host, context, &target, delivery)?;
                    enqueue(
                        host,
                        context.queue_owner,
                        target.control.record.target,
                        GuiLocalCommand::number_step(input, steps, shift)?,
                    )?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: target.control.record.target,
                    });
                }
                if target.control.color.is_some()
                    && let Some(disposition) =
                        self.color_key(host, context, &target, key, shift, delivery)?
                {
                    return Ok(disposition);
                }
                // In either orientation, Right and Up increase a value and
                // Left and Down decrease it, by the fine step with Shift.
                if target.control.steps_value()
                    && let Some(steps) = match key {
                        GuiPhysicalKey::Left | GuiPhysicalKey::Down => Some(-1.0),
                        GuiPhysicalKey::Right | GuiPhysicalKey::Up => Some(1.0),
                        _ => None,
                    }
                {
                    self.step_value(host, context, &target, steps, shift, delivery)?;
                    return Ok(GuiRoutingDisposition::Routed {
                        target: target.control.record.target,
                    });
                }
                // Home and End move the focused thumb to its legal bound: the
                // range's end, or a range's other thumb.
                if target.control.record.kind == GuiControlKind::Slider
                    && matches!(key, GuiPhysicalKey::Home | GuiPhysicalKey::End)
                {
                    let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
                    let bound = if key == GuiPhysicalKey::Home {
                        slider.min
                    } else {
                        slider.max
                    };
                    self.move_thumb(host, context, &target, bound, delivery)?;
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

    pub(super) fn input(
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

    pub(super) fn action(
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

    /// A secondary press on a control focuses it as a primary press does and
    /// requests a context at the press point in its canvas. Panel content
    /// without an eligible control is blocked, as for any press.
    fn pointer_context(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        query: ViewQueryTarget,
        point: [f32; 2],
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<GuiRoutingDisposition, GuiInputError> {
        let target = match locate(host, query, point, &context.blockers)? {
            Located::Target(target) => target,
            Located::Panel => return Ok(GuiRoutingDisposition::Blocked),
            Located::Miss => return Ok(GuiRoutingDisposition::Miss),
        };
        if target.control.record.focusable
            && !matches!(
                target.control.record.kind,
                GuiControlKind::ScrollView | GuiControlKind::VirtualList
            )
        {
            self.focus(host, context, target.clone(), false, delivery)?;
        }
        // The press point on the plane of the target's own layer.
        let layer = target.control.hit.layer;
        let canvas = project_composed_point(host, query, &target.path, point, true, layer)
            .map_err(|_| GuiInputError::StalePath)?
            .ok_or(GuiInputError::Unavailable)?;
        self.request_context(host, context, &target, canvas.point, delivery)?;
        Ok(GuiRoutingDisposition::Routed {
            target: target.control.record.target,
        })
    }

    fn request_context(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        point: [f32; 2],
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::context(input, point)?,
        )
    }

    pub(super) fn focus(
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
            self.blur(host, context, &previous, delivery)?;
        }
        let input = self.input(host, context, &target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::focus(input, target.focus_part(), visible)?,
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
                interaction_part(target),
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
        // A press on a numeric input's step part steps it and holds the
        // part, which repeats on the World's clock until released.
        if let Some(step) = target.step {
            let input = self.input(host, context, target, delivery)?;
            enqueue(
                host,
                context.queue_owner,
                target.control.record.target,
                GuiLocalCommand::number_part(input, step)?,
            )?;
            return Ok(None);
        }
        // Placing a caret needs the input's focus.
        if target.control.record.kind == GuiControlKind::TextInput
            && target.control.record.focusable
        {
            self.caret(host, context, target, query, point, false, delivery)?;
            return Ok(Some(Drag::Text));
        }
        // A press on a colour control's surface sets that surface's
        // channels at the pointer and drags them; the swatch takes none.
        if target.control.color.is_some() {
            let Some(part) = target.focus_part else {
                return Ok(None);
            };
            self.pick(host, context, target, query, point, part, delivery)?;
            return Ok(Some(Drag::Color(part)));
        }
        // A press on the thumb keeps its grab offset along the rail; a press
        // elsewhere on the control jumps the thumb to the pointer. A press on
        // a dial leaves its value and starts a relative drag from there.
        if target.control.record.kind == GuiControlKind::Slider {
            let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
            if slider.dial_travel.is_some() {
                let crate::systems::gui::presentation::GuiRoutingValue::Scalar(current) =
                    target.control.record.value
                else {
                    return Err(GuiInputError::Unavailable);
                };
                return Ok(Some(Drag::Dial(GuiDialDrag {
                    origin: local[1],
                    start: slider.fraction(current),
                })));
            }
            let rect = match (slider.range, target.focus_part) {
                (Some(range), Some(part)) => range.thumb_rects[part.min(1) as usize],
                _ => slider.thumb_rect,
            };
            let grabbing = (0..2)
                .all(|axis| local[axis] >= rect[axis] && local[axis] < rect[axis] + rect[axis + 2]);
            let axis = slider.axis;
            let offset = if grabbing {
                local[axis] - rect[axis] - rect[axis + 2] * 0.5
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
        let mut local = local_point(host, query, target, point)?;
        local[slider.axis] -= offset;
        let fraction = slider.fraction_at(local);
        let crate::systems::gui::presentation::GuiRoutingValue::Scalar(lower) =
            target.control.record.value
        else {
            return Err(GuiInputError::Unavailable);
        };
        let current = match (slider.range, target.focus_part) {
            (Some(range), Some(part)) => range.values[part.min(1) as usize],
            _ => lower,
        };
        let value = crate::systems::gui::local::slider::value_at(
            slider.min,
            slider.max,
            slider.step,
            fraction,
            current,
        )
        .ok_or(GuiInputError::Unavailable)?;
        self.move_thumb(host, context, target, value, delivery)
    }

    /// Move the slider thumb `target` names towards `value`; the World clamps
    /// it to the range and stops it at a range's other thumb.
    fn move_thumb(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        value: f32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::slider_thumb(input, target.focus_part(), value)?,
        )
    }

    /// `target` naming the thumb of a range slider that a pointer at `point`
    /// takes ([`range_thumb`]), the step part of a numeric text input it is
    /// over, or the colour control's surface under it; other controls name no
    /// part. A hover over a range's track names none and a press there takes
    /// the nearer thumb; a colour control's swatch names none.
    #[allow(clippy::too_many_arguments)]
    fn pointed_part(
        &self,
        host: &HostRuntime,
        context: &GuiRoutingContext,
        query: ViewQueryTarget,
        mut target: Target,
        point: [f32; 2],
        press: bool,
    ) -> Result<Target, GuiInputError> {
        if let Some(number) = target
            .control
            .number
            .filter(|number| number.steps.is_some())
        {
            let local = local_point(host, query, &target, point)?;
            target.step = number.step_at(local);
            return Ok(target);
        }
        if let Some(layout) = target.control.color {
            let local = local_point(host, query, &target, point)?;
            target.focus_part = layout.part_at(local);
            return Ok(target);
        }
        let Some(slider) = target
            .control
            .slider
            .filter(|slider| slider.range.is_some())
        else {
            return Ok(target);
        };
        let local = local_point(host, query, &target, point)?;
        let focused = context
            .focus
            .as_ref()
            .filter(|focus| {
                focus.control.record.target == target.control.record.target
                    && focus.path == target.path
            })
            .map(Target::focus_part);
        target.focus_part = range_thumb(&slider, local, focused, press);
        Ok(target)
    }

    /// Turn a dial by its drag to `point` and return the drag that continues
    /// from there; the value snaps to the step as a rail drag's does.
    #[allow(clippy::too_many_arguments)]
    fn turn(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        drag: GuiDialDrag,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<GuiDialDrag, GuiInputError> {
        let slider = target.control.slider.ok_or(GuiInputError::Unavailable)?;
        let travel = slider.dial_travel.ok_or(GuiInputError::Unavailable)?;
        let local = local_point(host, query, target, point)?;
        let (fraction, drag) = drag
            .turned(local[1], travel)
            .ok_or(GuiInputError::Unavailable)?;
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
        )?;
        Ok(drag)
    }

    /// Blur the context's keyboard target, committing a numeric text input's
    /// pending edit first, as blur does.
    pub(super) fn blur(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        if target.control.number.is_some() {
            let input = self.input(host, context, target, delivery)?;
            enqueue(
                host,
                context.queue_owner,
                target.control.record.target,
                GuiLocalCommand::number_commit(input)?,
            )?;
        }
        context.number_edit = false;
        self.action(host, context, target, GuiLocalAction::Blur, delivery)
    }

    /// Whether the focused numeric text input `focus` holds a pending edit
    /// for Escape to discard: the GUI System's record once its World applied
    /// this context's work, else whether this context routed an edit since.
    fn number_edit_pending(
        &self,
        host: &mut HostRuntime,
        context: &GuiRoutingContext,
        focus: &Target,
    ) -> bool {
        if focus.control.number.is_none() {
            return false;
        }
        let target = focus.control.record.target;
        if context.queued_worlds.borrow().contains_key(&target.world) {
            return context.number_edit;
        }
        host.world_mut(target.world.id())
            .is_some_and(|world| world.gui_number_edit(target, context.session.id()))
    }

    /// Step a value control by `steps` of its step, or of its fine step when
    /// `fine`: arrow keys on the focused control and the wheel over it.
    #[allow(clippy::too_many_arguments)]
    fn step_value(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        steps: f32,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::slider_thumb_step(input, target.focus_part(), steps, fine)?,
        )
    }

    /// Set the channels of colour control part `part` at the pointer: the
    /// field's saturation and value, or a rail's hue or alpha.
    #[allow(clippy::too_many_arguments)]
    fn pick(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        query: ViewQueryTarget,
        point: [f32; 2],
        part: u32,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let layout = target.control.color.ok_or(GuiInputError::Unavailable)?;
        let local = local_point(host, query, target, point)?;
        self.color_channels(
            host,
            context,
            target,
            layout.channels_at(part, local),
            delivery,
        )
    }

    fn color_channels(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        channels: [Option<f32>; 4],
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::color_channels(input, channels)?,
        )
    }

    /// Step colour channel `channel` by `steps` of the colour step, or of the
    /// fine step when `fine`.
    #[allow(clippy::too_many_arguments)]
    fn color_step(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        channel: usize,
        steps: f32,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<(), GuiInputError> {
        let input = self.input(host, context, target, delivery)?;
        enqueue(
            host,
            context.queue_owner,
            target.control.record.target,
            GuiLocalCommand::color_step(input, channel, steps, fine)?,
        )
    }

    /// A key on the focused part of a colour control: on the field, Left
    /// and Right step the saturation and Down and Up the value, and Home and
    /// End take the saturation to its bounds; on a rail, Right and Up raise
    /// its channel and Left and Down lower it, and Home and End take it to
    /// its bounds. Shift steps finely. Other keys are left to the caller.
    #[allow(clippy::too_many_arguments)]
    fn color_key(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        target: &Target,
        key: GuiPhysicalKey,
        fine: bool,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<Option<GuiRoutingDisposition>, GuiInputError> {
        use crate::systems::gui::local::color::{SATURATION, VALUE};

        let part = target.focus_part();
        let channel = |vertical: bool| match (part, vertical) {
            (0, false) => SATURATION,
            (0, true) => VALUE,
            _ => color_rail_channel(part),
        };
        let routed = Some(GuiRoutingDisposition::Routed {
            target: target.control.record.target,
        });
        let (channel, steps) = match key {
            GuiPhysicalKey::Left => (channel(false), -1.0),
            GuiPhysicalKey::Right => (channel(false), 1.0),
            GuiPhysicalKey::Down => (channel(true), -1.0),
            GuiPhysicalKey::Up => (channel(true), 1.0),
            GuiPhysicalKey::Home | GuiPhysicalKey::End => {
                let mut channels = [None; 4];
                channels[channel(false)] = Some(f32::from(u8::from(key == GuiPhysicalKey::End)));
                self.color_channels(host, context, target, channels, delivery)?;
                return Ok(routed);
            }
            _ => return Ok(None),
        };
        self.color_step(host, context, target, channel, steps, fine, delivery)?;
        Ok(routed)
    }
}

/// The channel a colour control's rail `part` holds: hue for 1, alpha for 2.
fn color_rail_channel(part: u32) -> usize {
    use crate::systems::gui::local::color::{ALPHA, HUE};

    if part == 2 {
        ALPHA
    } else {
        HUE
    }
}

/// The thumb of a range a pointer at control-local `local` takes: the one
/// under it, or for a press on the track the nearer one. Where both thumbs
/// are under it, or a track press is as near to both, it takes the one that
/// can move when both sit at one end of the range, else the last active one,
/// the thumb `focused` while the slider holds focus, else the one on the
/// pointer's side: the upper towards the maximum. A hover over the track
/// takes none.
fn range_thumb(
    slider: &crate::systems::gui::presentation::GuiSliderGeometry,
    local: [f32; 2],
    focused: Option<u32>,
    press: bool,
) -> Option<u32> {
    let range = slider.range?;
    let axis = slider.axis;
    let over = range.thumb_rects.map(|rect| {
        (0..2).all(|axis| local[axis] >= rect[axis] && local[axis] < rect[axis] + rect[axis + 2])
    });
    match over {
        [true, false] => return Some(0),
        [false, true] => return Some(1),
        [false, false] if !press => return None,
        _ => {}
    }
    let centre = |rect: [f32; 4]| rect[axis] + rect[axis + 2] * 0.5;
    let distance = range
        .thumb_rects
        .map(|rect| (local[axis] - centre(rect)).abs());
    if over == [false, false] && distance[0] != distance[1] {
        return Some(u32::from(distance[1] < distance[0]));
    }
    let [lower, upper] = range.values;
    if lower == upper && lower >= slider.max {
        return Some(0);
    }
    if lower == upper && upper <= slider.min {
        return Some(1);
    }
    if let Some(part) = focused {
        return Some(part.min(1));
    }
    let towards_max = slider.thumb_centers[1] - slider.thumb_centers[0];
    Some(u32::from(
        (local[axis] - centre(range.thumb_rects[0])) * towards_max > 0.0,
    ))
}

/// The value steps one wheel event makes: one, along its larger component,
/// up increasing and down decreasing. A horizontal component counts as a
/// vertical one turned to the right, since browsers deliver Shift with the
/// wheel, the fine step, as horizontal movement. None for no movement.
fn wheel_steps(delta: [f32; 2]) -> Option<f32> {
    let along = if delta[1].abs() >= delta[0].abs() {
        delta[1]
    } else {
        delta[0]
    };
    (along.is_finite() && along != 0.0).then(|| -along.signum())
}

pub(super) fn live(host: &HostRuntime, world: crate::WorldRef) -> Result<(), GuiInputError> {
    if host.world_ref(world.id()) == Some(world) {
        Ok(())
    } else {
        Err(GuiInputError::Unavailable)
    }
}

pub(super) fn enqueue(
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

/// Provider lifetimes along one completed path, without retaining historical geometry.
pub(super) fn surface_lifetimes(
    host: &HostRuntime,
    source: WorldPublicationId,
    path: &[WorldAttachmentToken],
) -> Result<Vec<Option<(u16, u64)>>, GuiInputError> {
    let mut publication = host.publication(source).ok_or(GuiInputError::StalePath)?;
    let mut lifetimes = Vec::with_capacity(path.len());
    for token in path {
        let edge = publication
            .attachments
            .iter()
            .find(|edge| edge.token == *token)
            .ok_or(GuiInputError::StalePath)?;
        lifetimes.push(match edge.mode {
            crate::WorldAttachmentMode::Spatial => None,
            _ => Some((
                edge.surface_geometry
                    .as_ref()
                    .ok_or(GuiInputError::StalePath)?
                    .component(),
                edge.surface_incarnation.ok_or(GuiInputError::StalePath)?,
            )),
        });
        publication = host
            .attached_publication(edge)
            .ok_or(GuiInputError::StalePath)?;
    }
    Ok(lifetimes)
}

pub(super) fn refresh(
    host: &HostRuntime,
    view: ViewDescriptor,
    target: &Target,
) -> Result<Target, GuiInputError> {
    if surface_lifetimes(host, view.publication, &target.path)? != target.surface_lifetimes {
        return Err(GuiInputError::StalePath);
    }
    let control = locate_control(host, view, &target.path, target.control.record.target)?;
    // A control that lost the named part, as a range made a single slider,
    // keeps its last part, or names none.
    let parts = control.record.focus_parts;
    let focus_part = target
        .focus_part
        .filter(|_| parts > 1)
        .map(|part| part.min(parts - 1));
    // A control that lost its step parts names none.
    let step = target
        .step
        .filter(|_| control.number.is_some_and(|number| number.steps.is_some()));
    Ok(Target {
        control,
        path: target.path.clone(),
        surface_lifetimes: target.surface_lifetimes.clone(),
        source: view.publication,
        part: target.part.clone(),
        focus_part,
        step,
    })
}

/// The completed observation of control `target` through attachment `path`
/// of `view`.
pub(super) fn locate_control(
    host: &HostRuntime,
    view: ViewDescriptor,
    path: &[WorldAttachmentToken],
    target: crate::systems::gui::local::GuiEntityTarget,
) -> Result<Arc<GuiControlObservation>, GuiInputError> {
    semantic_view(host, view, path)?
        .controls
        .iter()
        .find(|control| control.record.target == target)
        .cloned()
        .ok_or(GuiInputError::Unavailable)
}

/// The completed control observations of the canvas reached through
/// attachment `path` of `view`.
pub(super) fn semantic_view(
    host: &HostRuntime,
    view: ViewDescriptor,
    path: &[WorldAttachmentToken],
) -> Result<Arc<GuiCanvasSemanticView>, GuiInputError> {
    let mut publication = host
        .publication(view.publication)
        .ok_or(GuiInputError::StalePath)?;
    let mut output = view.output;
    for token in path {
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
    publication
        .chunk(CanvasSystem::ID)
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .and_then(|gui| gui.views.get(&output))
        .cloned()
        .ok_or(GuiInputError::Unavailable)
}

pub(super) fn current_target(
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
        // An open light or modal overlay takes the press from what lies
        // beneath and selects no control.
        GuiQueryOutcome::Hit(hit) if hit.hit.kind == CanvasHitKind::Overlay => Ok(Located::Panel),
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
                    surface_lifetimes: surface_lifetimes(
                        host,
                        result.view.publication,
                        &hit.path
                            .iter()
                            .map(|step| step.token.clone())
                            .collect::<Vec<_>>(),
                    )?,
                    source: result.view.publication,
                    part: hit.hit.kind.clone(),
                    focus_part: None,
                    step: None,
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
    if append_scroll_targets(
        host,
        hit.publication,
        hit.output,
        &hit.hit.ancestry,
        &path,
        result.view.publication,
        &mut targets,
    ) {
        return Ok(targets);
    }
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
            && let Some(anchor) = canvas.hits.iter().find(|hit| matches!(&hit.kind, crate::systems::canvas::CanvasHitKind::Attachment { token, .. } if *token == edge.token))
            && append_scroll_targets(host, publication.id, output, &anchor.ancestry, &path, result.view.publication, &mut targets) {
            break;
        }
    }
    Ok(targets)
}

/// Append the scroll views and virtual lists of `ancestry`, innermost first,
/// up to the first open overlay: the wheel's scroll chain ends at an overlay.
/// Returns whether it reached one.
#[allow(clippy::too_many_arguments)]
fn append_scroll_targets(
    host: &HostRuntime,
    source: WorldPublicationId,
    output: crate::OutputRef,
    ancestry: &[crate::EntityId],
    path: &[WorldAttachmentToken],
    root: WorldPublicationId,
    targets: &mut Vec<Target>,
) -> bool {
    let Ok(lifetimes) = surface_lifetimes(host, root, path) else {
        return false;
    };
    let Some(gui) = host
        .publication(source)
        .and_then(|publication| publication.chunk(CanvasSystem::ID))
        .and_then(|chunk| chunk.data::<GuiCanvasPublication>())
        .and_then(|gui| gui.views.get(&output))
    else {
        return false;
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
                surface_lifetimes: lifetimes.clone(),
                source: root,
                part: CanvasHitKind::Entity,
                focus_part: None,
                step: None,
            });
        }
        if gui
            .overlays
            .iter()
            .any(|overlay| overlay.target.entity == *entity)
        {
            return true;
        }
    }
    false
}

/// The control part a pointer names for skin feedback: a focus part of a
/// control with several, a numeric input's step part, else the hit part.
fn interaction_part(target: &Target) -> GuiInteractionPart {
    if let Some(part) = target.focus_part {
        return GuiInteractionPart::FocusPart(part);
    }
    if let Some(step) = target.step {
        return GuiInteractionPart::Step(step);
    }
    let axis = |axis: &CanvasAxis| usize::from(*axis == CanvasAxis::Vertical);
    match &target.part {
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
    let hit = &target.control.hit;
    // Captured input keeps projecting onto the plane of the captured target's layer.
    let projected = project_composed_point(host, query, &target.path, point, true, hit.layer)
        .map_err(|_| GuiInputError::StalePath)?
        .ok_or(GuiInputError::Unavailable)?;
    Ok(std::array::from_fn(|axis| {
        (projected.point[axis] - hit.position[axis]) / hit.scale[axis]
    }))
}
