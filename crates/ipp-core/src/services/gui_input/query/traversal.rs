use super::*;
use crate::{HostRuntime, OutputKind, ViewQueryTarget, WorldPublication};
use std::collections::BTreeSet;

/// Worlds one composed walk has entered.
///
/// A World has at most one active parent attachment and the Host rejects
/// cycles, so a completed composition is a tree and a walk enters each World
/// once, or once per layer plane of a Surface separating its canvas's layers.
/// This replaces a fixed work budget: the walk is bounded by the composition
/// it reads, and a World reached twice is an inconsistent composition that
/// fails the one query instead of looping.
#[derive(Default)]
pub(crate) struct GuiQueryWorlds(BTreeSet<(WorldRef, Option<u32>)>);

impl GuiQueryWorlds {
    /// Record entry into `world`, failing if this walk already entered it.
    pub(crate) fn enter(&mut self, world: WorldRef) -> Result<(), GuiQueryUnavailable> {
        self.enter_plane(world, None)
    }

    /// Record entry into one layer plane of `world`'s canvas, or into the
    /// whole World without `layer`.
    pub(crate) fn enter_plane(
        &mut self,
        world: WorldRef,
        layer: Option<u32>,
    ) -> Result<(), GuiQueryUnavailable> {
        if self.0.insert((world, layer)) {
            Ok(())
        } else {
            Err(GuiQueryUnavailable::RepeatedWorld)
        }
    }
}

pub(super) struct QueryView {
    pub output: OutputRef,
    pub publication: WorldPublicationId,
    pub point: [f32; 2],
    pub extent: [f64; 2],
    pub path: Vec<GuiQueryStep>,
    /// The one canvas layer a Surface plane holds where the Surface separates
    /// layers; `None` tests every layer in reverse painter order.
    pub layer: Option<u32>,
}

pub(super) enum QueryTask<'a> {
    View(QueryView),
    Outcome(GuiQueryOutcome<'a>),
}

pub(super) struct QueryWalk<'a, 'options> {
    pub host: &'a HostRuntime,
    pub options: GuiQueryOptions<'options>,
    pub worlds: GuiQueryWorlds,
    pub pending: Vec<QueryTask<'a>>,
    /// Path to the nearest Surface a Camera ray entered, reported when the
    /// walk ends without a hit.
    pub panel: Option<Vec<GuiQueryStep>>,
}

/// Query real completed outputs without evaluating, dispatching, or reading mutable control values.
/// The point is normalized root viewport space. Retained headless views are not display claims.
pub fn query_composed_input<'a>(
    host: &'a HostRuntime,
    target: ViewQueryTarget,
    point: [f32; 2],
    options: GuiQueryOptions<'_>,
) -> Result<GuiQueryResult<'a>, ErrorReason> {
    if !point
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return Err(ErrorReason::InvalidViewport);
    }
    let view = host.resolve_view(target)?;
    let mut walk = QueryWalk {
        host,
        options,
        worlds: GuiQueryWorlds::default(),
        pending: vec![QueryTask::View(QueryView {
            output: view.output,
            publication: view.publication,
            point,
            extent: [
                f64::from(view.viewport.width),
                f64::from(view.viewport.height),
            ],
            path: Vec::new(),
            layer: None,
        })],
        panel: None,
    };
    let outcome = match walk.run() {
        Ok(outcome) => outcome,
        Err(reason) => GuiQueryOutcome::Unavailable(reason),
    };
    Ok(GuiQueryResult {
        view,
        outcome,
    })
}

impl<'a> QueryWalk<'a, '_> {
    pub(super) fn branch_block(&self, world: WorldRef) -> Option<GuiQueryBlockReason> {
        (self.host.world_fault(world) != Ok(None)).then_some(GuiQueryBlockReason::Unavailable)
    }

    pub(super) fn resources_available(&self, publication: &WorldPublication) -> bool {
        publication.resources().all(|key| {
            self.host
                .publication_resource(publication.id, key)
                .is_some_and(|resource| resource.data().is_some())
        })
    }

    fn run(&mut self) -> Result<GuiQueryOutcome<'a>, GuiQueryUnavailable> {
        while let Some(task) = self.pending.pop() {
            let view = match task {
                QueryTask::Outcome(outcome) => return Ok(outcome),
                QueryTask::View(view) => view,
            };
            // Candidates pop nearest first, so the first Surface entered is the
            // nearest panel on the ray.
            if self.panel.is_none() && view.path.last().is_some_and(|step| step.distance.is_some())
            {
                self.panel = Some(view.path.clone());
            }
            self.worlds.enter_plane(view.output.world(), view.layer)?;
            if let Some(reason) = self.branch_block(view.output.world()) {
                return Ok(GuiQueryOutcome::Blocked {
                    reason,
                    path: view.path,
                });
            }
            let Some(publication) = self.host.publication(view.publication) else {
                return Ok(GuiQueryOutcome::Blocked {
                    reason: GuiQueryBlockReason::Unavailable,
                    path: view.path,
                });
            };
            if !self.resources_available(publication) {
                return Ok(GuiQueryOutcome::Blocked {
                    reason: GuiQueryBlockReason::Unavailable,
                    path: view.path,
                });
            }
            match view.output.kind() {
                OutputKind::Camera => self.camera(view)?,
                OutputKind::Canvas => self.canvas(view)?,
            }
        }
        Ok(match self.panel.take() {
            Some(path) => GuiQueryOutcome::Blocked {
                reason: GuiQueryBlockReason::Panel,
                path,
            },
            None => GuiQueryOutcome::Miss,
        })
    }
}
