//! Parent-first local evaluation and child-first immutable output assembly.

use super::publication::{
    RetainedPublication, WorldOutputBuilder, WorldPublication, WorldPublicationId,
};
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
    /// Resolve an exact live World lifetime without selecting presentation.
    pub fn world_ref(&self, world: WorldId) -> Option<WorldRef> {
        self.worlds.get(&world).map(World::runtime_ref)
    }

    /// Validate a transported lifetime without selecting a replacement World.
    pub fn resolve_world_ref(&self, world: WorldId, incarnation: u64) -> Option<WorldRef> {
        self.world_ref(world)
            .filter(|world| world.incarnation() == incarnation)
    }

    /// Validate a transported output selection without rebinding: the World
    /// must still supply its canvas, or the Camera component lifetime must match.
    pub fn resolve_output_ref(
        &self,
        world: WorldRef,
        target: crate::OutputTarget,
    ) -> Result<OutputRef, ErrorReason> {
        if self.world_ref(world.id) != Some(world) {
            return Err(ErrorReason::InvalidEntity);
        }
        self.worlds
            .get(&world.id)
            .ok_or(ErrorReason::InvalidEntity)?
            .bind_output_target(target)
    }

    /// Explicitly bind or rebind the current Camera output of an entity; a
    /// World's canvas names no entity and is selected with [`OutputRef::canvas`].
    pub fn bind_output(
        &self,
        world: WorldRef,
        entity: crate::EntityId,
        kind: OutputKind,
    ) -> Result<OutputRef, ErrorReason> {
        if self.world_ref(world.id) != Some(world) {
            return Err(ErrorReason::InvalidEntity);
        }
        self.worlds
            .get(&world.id)
            .ok_or(ErrorReason::InvalidEntity)?
            .bind_output(entity, kind)
    }

    /// Explicit selection never follows an authoring session or replacement incarnation.
    pub fn set_root_output(
        &mut self,
        output: OutputRef,
        viewport: WorldViewport,
    ) -> Result<(), ErrorReason> {
        viewport.validate()?;
        if !self
            .worlds
            .get(&output.world.id)
            .is_some_and(|world| world.output_valid(output))
        {
            return Err(ErrorReason::InvalidEntity);
        }
        if self.topology.incoming.contains_key(&output.world.id) {
            return Err(ErrorReason::InvalidValue);
        }
        let serial = self
            .topology
            .next_root_binding
            .checked_add(1)
            .ok_or(ErrorReason::Capacity)?;
        self.topology.roots.insert(
            output.world.id,
            RootOutputBinding {
                output,
                viewport,
                generation: RootBindingGeneration {
                    host: self.topology.identity,
                    serial,
                },
            },
        );
        self.topology.next_root_binding = serial;
        Ok(())
    }

    /// Observe the exact root binding independently of output readiness.
    pub fn root_output_binding(
        &self,
        world: WorldRef,
    ) -> Result<Option<RootOutputBinding>, ErrorReason> {
        if self.world_ref(world.id()) != Some(world) {
            return Err(ErrorReason::InvalidEntity);
        }
        Ok(self.topology.roots.get(&world.id()).copied())
    }

    /// Withdraw root presentation without destroying the World.
    pub fn clear_root_output(&mut self, world: WorldId) {
        self.topology.roots.remove(&world);
    }

    /// Observe an available explicitly selected root view.
    pub fn root_output(
        &self,
        world: WorldId,
    ) -> Option<(OutputRef, WorldViewport, WorldPublicationId)> {
        let binding = self.topology.roots.get(&world)?;
        let publication = self.latest_publication(world)?;
        self.output(publication, binding.output)?;
        Some((binding.output, binding.viewport, publication))
    }

    /// Read historical output while its producer is valid; this does not authorize presentation.
    /// Only root selection and completed attachment traversal establish presentation paths.
    pub fn output(
        &self,
        publication: WorldPublicationId,
        selection: OutputRef,
    ) -> Option<&super::WorldDerivedChunk> {
        self.worlds
            .get(&selection.world.id)
            .filter(|world| world.output_valid(selection))?;
        self.publication(publication)?.output(selection)
    }

    /// Advance each eligible World once; no client-facing time operation is introduced.
    pub fn frame(&mut self, delta: f64) -> Result<HostFrameReport, ErrorReason> {
        if !delta.is_finite() || delta < 0.0 {
            return Err(ErrorReason::InvalidValue);
        }
        self.frame = self.frame.checked_add(1).ok_or(ErrorReason::Capacity)?;
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
                placement: attachment::IDENTITY,
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
                .context(&mut self.assets, &mut self.data_sources, &mut self.topology);
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
                .context(&mut self.assets, &mut self.data_sources, &mut self.topology);
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
