//! Keyboard target adoption.
//!
//! Logical focus is the keyboard target of the input context presenting its
//! World, however it was set. A `GuiAction` command, and an overlay moving
//! focus in or back, set focus in one World without an owning input session,
//! so at its routing boundary the context finds such focus in the Worlds it
//! presents and makes it its keyboard target: it blurs the previous target in
//! another World and focuses the new one through ordinary routed commands in
//! the focus's own modality, which keep the focus unowned and give a text
//! input this session's native record. A World that no longer focuses
//! the keyboard target, because a command blurred or moved its focus, ends it.
//!
//! A World holding routed work of this context that has not settled is left
//! for a later boundary, so focus the context itself just queued is never
//! mistaken for a change.

use super::GuiInputError;
use super::GuiRoutingDelivery;
use super::routing::{
    GuiInputRouter, GuiRoutingContext, Target, current_target, locate_control, refresh,
    surface_lifetimes,
};
use crate::services::gui_input::{GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal};
use crate::systems::canvas::CanvasHitKind;
use crate::systems::gui::local::{GuiLocalAction, GuiLocalEffect, GuiNativeTextState};
use crate::{HostRuntime, ViewDescriptor, WorldAttachmentToken, WorldRef};
use std::collections::BTreeSet;

impl GuiInputRouter {
    /// Make logical focus a command set in a presented World this context's
    /// keyboard target, and end a keyboard target its World no longer focuses.
    /// Only queueing the adopting commands fails.
    pub(super) fn follow_logical_focus(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        view: ViewDescriptor,
    ) -> Result<(), GuiInputError> {
        let mut adopted: Vec<(Target, bool)> = Vec::new();
        for (world, path) in presented_worlds(host, view) {
            if context.queued_worlds.borrow().contains_key(&world) {
                continue;
            }
            let Some(focus) = host
                .world_mut(world.id())
                .map(|world| world.gui_logical_focus())
            else {
                continue;
            };
            let keyboard = context
                .focus
                .as_ref()
                .map(|focus| (focus.control.record.target, focus.focus_part()));
            if keyboard.is_some_and(|(keyboard, _)| keyboard.world == world)
                && focus.map(|(target, ..)| target) != keyboard.map(|(target, _)| target)
            {
                context.focus = None;
            }
            let Some((target, part, true, visible)) = focus else {
                continue;
            };
            if keyboard == Some((target, part)) {
                continue;
            }
            let Ok(control) = locate_control(host, view, &path, target) else {
                continue;
            };
            let focus_part = (control.record.focus_parts > 1).then_some(part);
            let Ok(lifetimes) = surface_lifetimes(host, view.publication, &path) else {
                continue;
            };
            let candidate = Target {
                control,
                path,
                surface_lifetimes: lifetimes,
                source: view.publication,
                part: CanvasHitKind::Entity,
                focus_part,
                step: None,
            };
            if current_target(host, view, &candidate).is_ok() {
                adopted.push((candidate, visible));
            }
        }

        // Commands that focused controls in several presented Worlds since the
        // last boundary are not ordered against each other; the last in
        // presentation order becomes the target and the others are blurred,
        // so the context keeps one keyboard target.
        let Some((target, visible)) = adopted.pop() else {
            return Ok(());
        };

        // The previous target is blurred through the view being routed. The
        // adopted focus keeps its modality: a command shows the ring, while
        // focus an overlay moved after a pointer press does not.
        context.focus = context
            .focus
            .take()
            .and_then(|focus| refresh(host, view, &focus).ok());
        let delivery = &mut Unreported;
        for (other, _) in &adopted {
            self.action(host, context, other, GuiLocalAction::Blur, delivery)?;
        }
        self.focus(host, context, target, visible, delivery)
    }
}

/// Every World `view` presents with its attachment path, root first and in
/// attachment order. Attachments without an available publication present
/// nothing.
fn presented_worlds(
    host: &HostRuntime,
    view: ViewDescriptor,
) -> Vec<(WorldRef, Vec<WorldAttachmentToken>)> {
    let mut worlds = Vec::new();
    let mut entered = BTreeSet::new();
    let mut pending = vec![(view.publication, Vec::new())];
    while let Some((source, path)) = pending.pop() {
        let Some(publication) = host.publication(source) else {
            continue;
        };
        if !entered.insert(publication.world) {
            continue;
        }
        for edge in publication.attachments.iter().rev() {
            if let Some(child) = host.attached_publication(edge) {
                let mut child_path = path.clone();
                child_path.push(edge.token.clone());
                pending.push((child.id, child_path));
            }
        }
        worlds.push((publication.world, path));
    }
    worlds
}

/// Delivery for the commands adopting a keyboard target: no physical request
/// awaits their terminals, and clients observe the focus in its World.
struct Unreported;

impl GuiRoutingDelivery for Unreported {
    fn command(&mut self) -> Result<Box<dyn GuiDeliveryPermit>, GuiInputError> {
        Ok(Box::new(Unreported))
    }
}

impl GuiDeliveryPermit for Unreported {
    fn prepare_native(&mut self, _: &GuiNativeTextState) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, _: GuiDeliveryTerminal) {}
}
