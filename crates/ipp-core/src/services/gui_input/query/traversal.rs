use super::*;
use crate::{HostRuntime, OutputKind, ViewQueryTarget, WorldPublication};
use std::collections::{BTreeMap, BTreeSet};

/// Worlds a geometry-independent traversal has entered. Keyboard traversal
/// visits each attachment once; repeated Worlds indicate inconsistent topology.
#[derive(Default)]
pub(crate) struct GuiQueryWorlds(BTreeSet<WorldRef>);

impl GuiQueryWorlds {
    /// Record entry into `world`, failing if this walk already entered it.
    pub(crate) fn enter(&mut self, world: WorldRef) -> Result<(), GuiQueryUnavailable> {
        if self.0.insert(world) {
            Ok(())
        } else {
            Err(GuiQueryUnavailable::RepeatedWorld)
        }
    }
}

/// Distinct geometric candidates may revisit the same attachment path. Reject
/// cycles and competing topology, while allowing every root and its descendants.
#[derive(Default)]
pub(super) struct GuiQueryPaths(BTreeMap<WorldRef, Vec<(u64, u64)>>);

impl GuiQueryPaths {
    pub(super) fn enter(
        &mut self,
        world: WorldRef,
        path: &[GuiQueryStep],
    ) -> Result<(), GuiQueryUnavailable> {
        let mut ancestry = BTreeSet::new();
        let mut previous = path.first().map(|step| step.token.parent());
        if let Some(root) = previous {
            ancestry.insert(root);
        }
        for step in path {
            if Some(step.token.parent()) != previous {
                return Err(GuiQueryUnavailable::RepeatedWorld);
            }
            let child = step
                .token
                .child()
                .ok_or(GuiQueryUnavailable::RepeatedWorld)?;
            if !ancestry.insert(child) {
                return Err(GuiQueryUnavailable::RepeatedWorld);
            }
            previous = Some(child);
        }
        if previous.is_some_and(|last| last != world) {
            return Err(GuiQueryUnavailable::RepeatedWorld);
        }
        let identity: Vec<_> = path.iter().map(|step| step.token.identity()).collect();
        match self.0.get(&world) {
            Some(known) if *known != identity => Err(GuiQueryUnavailable::RepeatedWorld),
            Some(_) => Ok(()),
            None => {
                self.0.insert(world, identity);
                Ok(())
            }
        }
    }
}

pub(super) struct QueryView {
    pub output: OutputRef,
    pub publication: WorldPublicationId,
    pub point: [f32; 2],
    pub extent: [f64; 2],
    pub path: Vec<GuiQueryStep>,
    /// The one canvas layer a Surface shell holds where the Surface separates
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
    pub paths: GuiQueryPaths,
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
        paths: GuiQueryPaths::default(),
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
            self.paths.enter(view.output.world(), &view.path)?;
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

#[cfg(test)]
#[path = "traversal_tests.rs"]
mod tests;
