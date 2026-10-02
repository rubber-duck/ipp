//! Overlay modes in physical routing.
//!
//! The completed canvas observations list each canvas's open overlays in
//! stacking order, the topmost last. A press of any button walks the open
//! overlays of every canvas the context presents from the topmost down: it
//! passes manual overlays and hints, stops at a modal overlay, and stops at
//! the first light overlay it lands inside of, or inside its parent's
//! subtree; every light overlay above that one closes. A press that closes
//! one is swallowed: it activates, focuses and hovers nothing, and does not
//! reach another panel or the scene. A press on a light overlay's parent,
//! such as its trigger, is inside it and reaches the parent, so the client's
//! own toggle closes it. Where a press lands is what its completed hit names:
//! a control, a light overlay's box, the canvas under a modal one, or the
//! attachment of a nested panel.
//!
//! A modal overlay keeps the controls under it, on lower layers and earlier
//! on its own layer, from keys and native text edits, which are blocked, and
//! from keyboard traversal; Tab from a control of its canvas stays inside it.
//! Its canvas-wide hit already keeps them from the pointer, hover and wheel.
//!
//! Escape first discards a pending edit of a focused numeric text input,
//! then closes the topmost overlay that is not manual in the canvas of the
//! focused control, or of a hovered one when nothing is focused, giving the
//! focused control the keyboard's focus ring as any key does; only then does
//! it blur. The GUI System of the overlay's World applies the closing,
//! returns focus to the overlay's invoker and closes light overlays that
//! focus leaves.

use super::routing::{GuiInputRouter, GuiRoutingContext, semantic_view};
use super::{GuiInputError, GuiRoutingDelivery, GuiRoutingDisposition};
use crate::components::GuiOverlay;
use crate::services::gui_input::query::{
    GuiPickingBlocker, GuiQueryOptions, GuiQueryOutcome, query_composed_input,
};
use crate::systems::canvas::{CanvasHitKind, CanvasPublication, CanvasSystem};
use crate::systems::gui::GuiSystem;
use crate::systems::gui::local::{GuiEntityTarget, GuiOverlayCommand};
use crate::systems::gui::presentation::{
    GuiCanvasPublication, GuiCanvasSemanticView, GuiOverlayObservation,
};
use crate::{
    EntityId, HostRuntime, OutputRef, ViewDescriptor, ViewQueryTarget, WorldAttachmentToken,
    WorldPublicationId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The topmost open modal overlay of a canvas.
pub(super) fn topmost_modal(view: &GuiCanvasSemanticView) -> Option<&GuiOverlayObservation> {
    view.overlays
        .iter()
        .rev()
        .find(|overlay| overlay.mode == GuiOverlay::MODE_MODAL)
}

/// A presented canvas with open overlays and the attachment path to it.
struct OverlayCanvas {
    path: Vec<WorldAttachmentToken>,
    publication: WorldPublicationId,
    view: Arc<GuiCanvasSemanticView>,
}

/// Every canvas `view` presents that has an open light overlay.
fn light_canvases(host: &HostRuntime, view: ViewDescriptor) -> Vec<OverlayCanvas> {
    let mut found = Vec::new();
    let mut entered = BTreeSet::new();
    let mut pending = vec![(view.publication, Vec::new())];
    while let Some((source, path)) = pending.pop() {
        let Some(publication) = host.publication(source) else {
            continue;
        };
        if !entered.insert(publication.world) {
            continue;
        }
        let gui = publication
            .chunk(CanvasSystem::ID)
            .and_then(|chunk| chunk.data::<GuiCanvasPublication>());
        for canvas in gui.into_iter().flat_map(|gui| gui.views.values()) {
            if canvas
                .overlays
                .iter()
                .any(|overlay| overlay.mode == GuiOverlay::MODE_LIGHT)
            {
                found.push(OverlayCanvas {
                    path: path.clone(),
                    publication: publication.id,
                    view: canvas.clone(),
                });
            }
        }
        for edge in &publication.attachments {
            if let Some(child) = host.attached_publication(edge) {
                let mut child_path = path.clone();
                child_path.push(edge.token.clone());
                pending.push((child.id, child_path));
            }
        }
    }
    found
}

/// Where a press landed: the attachment path to the World it reached and,
/// when it hit something there, the canvas and the hit's root-first ancestry.
struct PressSite {
    path: Vec<WorldAttachmentToken>,
    hit: Option<(OutputRef, Arc<[EntityId]>)>,
}

impl PressSite {
    /// Locate a press at `point`; none when the completed view cannot say.
    fn locate(
        host: &HostRuntime,
        query: ViewQueryTarget,
        point: [f32; 2],
        blockers: &[GuiPickingBlocker],
    ) -> Result<Option<Self>, GuiInputError> {
        if point.iter().any(|coordinate| !coordinate.is_finite()) {
            return Err(GuiInputError::Unavailable);
        }
        if point
            .iter()
            .any(|coordinate| !(0.0..=1.0).contains(coordinate))
        {
            return Ok(Some(Self {
                path: Vec::new(),
                hit: None,
            }));
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
        let tokens = |path: &[crate::services::gui_input::query::GuiQueryStep]| {
            path.iter().map(|step| step.token.clone()).collect()
        };
        Ok(match result.outcome {
            GuiQueryOutcome::Hit(hit) => Some(Self {
                path: tokens(&hit.path),
                hit: Some((hit.output, hit.hit.ancestry.clone())),
            }),
            GuiQueryOutcome::Blocked {
                reason:
                    crate::services::gui_input::query::GuiQueryBlockReason::Panel
                    | crate::services::gui_input::query::GuiQueryBlockReason::PickingGeometry,
                path,
            } => Some(Self {
                path: tokens(&path),
                hit: None,
            }),
            GuiQueryOutcome::Miss => Some(Self {
                path: Vec::new(),
                hit: None,
            }),
            GuiQueryOutcome::Blocked {
                ..
            }
            | GuiQueryOutcome::Unavailable(_) => None,
        })
    }

    /// The ancestry, in `canvas`, of what the press landed on: the hit when
    /// it is in that canvas, or the attachment through which it reached a
    /// nested World.
    fn ancestry_in(&self, host: &HostRuntime, canvas: &OverlayCanvas) -> Option<Arc<[EntityId]>> {
        if self.path == canvas.path {
            return self
                .hit
                .as_ref()
                .filter(|(output, _)| *output == canvas.view.selection)
                .map(|(_, ancestry)| ancestry.clone());
        }
        let token = self
            .path
            .strip_prefix(canvas.path.as_slice())
            .and_then(|rest| rest.first())?;
        host.publication(canvas.publication)?
            .output(canvas.view.selection)
            .and_then(|chunk| chunk.data::<CanvasPublication>())?
            .hits
            .iter()
            .find(|hit| {
                matches!(&hit.kind, CanvasHitKind::Attachment { token: edge, .. } if edge == token)
            })
            .map(|hit| hit.ancestry.clone())
    }
}

/// Queue the closing of one open overlay for its World's next mutation
/// boundary.
fn close(
    host: &mut HostRuntime,
    owner: u64,
    overlay: GuiEntityTarget,
) -> Result<(), GuiInputError> {
    host.world_mut(overlay.world.id())
        .ok_or(GuiInputError::Unavailable)?
        .enqueue_system_command(GuiSystem::ID, owner, GuiOverlayCommand::close(overlay))
        .map_err(|_| GuiInputError::Unavailable)
}

impl GuiInputRouter {
    /// Close the open light overlays a press at `point` lands outside of, and
    /// return whether it closed any, so that the press is swallowed.
    pub(super) fn dismiss_outside(
        &mut self,
        host: &mut HostRuntime,
        context: &GuiRoutingContext,
        view: ViewDescriptor,
        query: ViewQueryTarget,
        point: [f32; 2],
    ) -> Result<bool, GuiInputError> {
        let canvases = light_canvases(host, view);
        if canvases.is_empty() {
            return Ok(false);
        }
        let Some(site) = PressSite::locate(host, query, point, context.blockers())? else {
            return Ok(false);
        };
        let mut closing = Vec::new();
        for canvas in &canvases {
            let ancestry = site.ancestry_in(host, canvas);
            for overlay in canvas.view.overlays.iter().rev() {
                match overlay.mode {
                    GuiOverlay::MODE_MODAL => break,
                    GuiOverlay::MODE_LIGHT
                        if ancestry
                            .as_deref()
                            .is_some_and(|ancestry| overlay.holds(ancestry)) =>
                    {
                        break;
                    }
                    GuiOverlay::MODE_LIGHT => closing.push(overlay.target),
                    _ => {}
                }
            }
        }
        for &overlay in &closing {
            close(host, context.queue_owner, overlay)?;
        }
        Ok(!closing.is_empty())
    }

    /// Escape's overlay step: close the topmost overlay that is not manual in
    /// the canvas of the focused control, or of a hovered one when nothing is
    /// focused. None when there is none.
    pub(super) fn escape_overlay(
        &mut self,
        host: &mut HostRuntime,
        context: &mut GuiRoutingContext,
        view: ViewDescriptor,
        delivery: &mut dyn GuiRoutingDelivery,
    ) -> Result<Option<GuiRoutingDisposition>, GuiInputError> {
        let Some(path) = context
            .focus
            .as_ref()
            .map(|focus| focus.path.clone())
            .or_else(|| context.hovered_path())
        else {
            return Ok(None);
        };
        let Ok(canvas) = semantic_view(host, view, &path) else {
            return Ok(None);
        };
        let Some(overlay) = canvas
            .overlays
            .iter()
            .rev()
            .find(|overlay| overlay.mode != GuiOverlay::MODE_MANUAL)
        else {
            return Ok(None);
        };
        if let Some(focus) = context.focus.clone() {
            self.focus(host, context, focus, true, delivery)?;
        }
        close(host, context.queue_owner, overlay.target)?;
        Ok(Some(GuiRoutingDisposition::Routed {
            target: overlay.target,
        }))
    }
}

/// Whether an open modal overlay of its canvas keeps the focused control
/// `focus` from keys and text edits.
pub(super) fn under_modal(
    host: &HostRuntime,
    view: ViewDescriptor,
    focus: &super::routing::Target,
) -> Result<bool, GuiInputError> {
    let canvas = semantic_view(host, view, &focus.path)?;
    Ok(topmost_modal(&canvas).is_some_and(|modal| modal.blocks(&focus.control)))
}
