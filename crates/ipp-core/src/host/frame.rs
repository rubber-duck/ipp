//! Parent-first local evaluation and child-first immutable output assembly.

use super::publication::{RetainedPublication, WorldOutputBuilder, WorldPublication};
use super::*;
use std::collections::BTreeSet;

/// Headless frame evidence. Platform input, presentation and delivery consume this boundary.
#[derive(Debug, Default)]
pub struct HostFrameReport {
    /// Host frame identity.
    pub frame: u64,
    /// Local reports and recoverable failures by World identity.
    pub worlds: BTreeMap<WorldId, Result<crate::WorldUpdateReport, ErrorReason>>,
    /// Actual parent-first local evaluation order.
    pub evaluation_order: Vec<WorldId>,
    /// Actual child-first publication order.
    pub publication_order: Vec<WorldId>,
    /// Failures that retained the preceding completed contribution.
    pub publication_errors: BTreeMap<WorldId, String>,
}

impl HostRuntime {
    /// Advance each eligible World once; no client-facing time operation is introduced.
    pub fn frame(&mut self, delta: f64) -> Result<HostFrameReport, ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _profile = self.profile_scope();

        if !delta.is_finite() || delta < 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        let next_frame = self.frame.checked_add(1).ok_or(ErrorReason::Capacity)?;
        #[cfg(feature = "instrumentation")]
        let _trace_frame = crate::profiling::trace::frame();
        self.data
            .advance_time(delta)
            .map_err(|_| ErrorReason::InvalidValue)?;
        self.frame = next_frame;
        self.progress_evaluation_assets();
        let mut report = HostFrameReport {
            frame: self.frame,
            ..Default::default()
        };
        let mut admitted = BTreeMap::new();
        for world in self.topology.order().to_vec() {
            let references = self.worlds[&world].queued_reference_worlds();
            let result = self
                .reference_context(world, references)
                .expect("live graph World")
                .admit_frame(delta);
            match result {
                Ok(value) => {
                    admitted.insert(world, value);
                }
                Err(error) => {
                    report.worlds.insert(world, Err(error));
                }
            }
        }
        let order = self.topology.order().to_vec();
        let mut contexts: BTreeMap<WorldId, WorldFrameContext> = BTreeMap::new();
        let mut edges = BTreeMap::new();
        for &world in &order {
            let Some(admitted) = admitted.remove(&world) else {
                continue;
            };
            let mut context = contexts.remove(&world).unwrap_or(WorldFrameContext {
                frame: self.frame,
                delta,
                placement: references::IDENTITY,
                viewport: self
                    .topology
                    .roots
                    .get(&world)
                    .map(|binding| binding.viewport),
                selected_output: self
                    .topology
                    .roots
                    .get(&world)
                    .map(|binding| binding.output),
                surface_extent: None,
            });
            context.selected_output = context
                .selected_output
                .filter(|selection| self.worlds[&world].output_valid(*selection));
            let mut access = self
                .worlds
                .get_mut(&world)
                .expect("live graph World")
                .context(
                    &mut self.assets,
                    &mut self.io,
                    &mut self.data,
                    &mut self.topology,
                );
            access.frame_context = Some(&context);
            let result = access.evaluate_frame(delta, admitted);
            if let Some(fault) = access.fault() {
                report.publication_errors.insert(world, fault.to_string());
            } else if result.is_ok() {
                report.evaluation_order.push(world);
                let attachments = access.completed_attachments();
                for edge in &attachments {
                    contexts.insert(
                        edge.child.id,
                        WorldFrameContext {
                            frame: self.frame,
                            delta,
                            placement: multiply(context.placement, edge.placement),
                            viewport: context.viewport,
                            selected_output: (edge.mode != WorldAttachmentMode::Spatial)
                                .then_some(edge.output)
                                .flatten()
                                .filter(|_| edge.surface_extent.is_some()),
                            surface_extent: edge.surface_extent,
                        },
                    );
                }
                edges.insert(world, attachments);
            }
            report.worlds.insert(world, result);
        }
        self.flush_resource_lifecycle();
        for &world in order.iter().rev() {
            let Some(mut attachments) = edges.remove(&world) else {
                continue;
            };
            for edge in &mut attachments {
                edge.publication = self.latest_publication(edge.child.id);
                if edge.mode != WorldAttachmentMode::Spatial
                    && (edge.surface_extent.is_none()
                        || edge.output.is_none_or(|selection| {
                            edge.publication.is_none_or(|publication| {
                                self.output(publication, selection).is_none()
                            })
                        }))
                {
                    edge.publication = None;
                }
            }
            let id = self.next_publication()?;
            let previous = self
                .publications
                .latest
                .get(&world)
                .and_then(|id| self.publications.completed.get(id))
                .map(|entry| &entry.data);
            let mut builder = WorldOutputBuilder {
                previous,
                version: id.revision,
                chunks: BTreeMap::new(),
                outputs: BTreeMap::new(),
                resources: BTreeSet::new(),
            };
            let access = self
                .worlds
                .get_mut(&world)
                .expect("live graph World")
                .context(
                    &mut self.assets,
                    &mut self.io,
                    &mut self.data,
                    &mut self.topology,
                );
            if let Some(fault) = access.fault() {
                report.publication_errors.insert(world, fault.to_string());
                continue;
            }
            let result = access.publish_output(&mut builder);
            let tick = access.tick();
            let time = access.time();
            let identity = access.world_ref();
            drop(access);
            if let Err(error) = result {
                report.publication_errors.insert(world, error.to_string());
                continue;
            }
            let lease = match self
                .assets
                .retain_publication(builder.resources.iter().copied())
            {
                Ok(lease) => lease,
                Err(error) => {
                    report.publication_errors.insert(world, error);
                    continue;
                }
            };
            let publication = WorldPublication {
                id,
                world: identity,
                tick,
                time,
                attachments,
                chunks: builder.chunks,
                outputs: builder.outputs,
                resources: builder.resources,
            };
            self.publications.completed.insert(
                id,
                RetainedPublication {
                    data: publication,
                    lease,
                    available: true,
                    readers: std::rc::Rc::default(),
                },
            );
            self.publications.latest.insert(world, id);
            report.publication_order.push(world);
        }
        self.retire_publications();
        self.flush_resource_lifecycle();
        Ok(report)
    }
}

pub(super) fn multiply(parent: [f64; 16], local: [f64; 16]) -> [f64; 16] {
    std::array::from_fn(|index| {
        let column = index / 4;
        let row = index % 4;
        (0..4)
            .map(|inner| parent[inner * 4 + row] * local[column * 4 + inner])
            .sum()
    })
}
