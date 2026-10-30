//! Parent-first local evaluation and child-first immutable output assembly.

use super::publication::{RetainedPublication, WorldOutputBuilder, WorldPublication};
use super::*;

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

#[derive(Default)]
pub(super) struct HostFrameScratch {
    order: Vec<WorldId>,
    worlds: BTreeMap<WorldId, WorldFrameScratch>,
}

#[derive(Default)]
struct WorldFrameScratch {
    admitted: Option<crate::WorldUpdateReport>,
    context: Option<WorldFrameContext>,
    attachments: Vec<PublishedWorldAttachment>,
    evaluated: bool,
    publication: super::publication::PublicationScratch,
}

impl HostRuntime {
    /// Advance each eligible World once; no client-facing time operation is introduced.
    pub fn frame(&mut self, delta: f64) -> Result<HostFrameReport, ErrorReason> {
        let mut report = HostFrameReport::default();
        self.frame_into(delta, &mut report)?;
        Ok(report)
    }

    /// Advance a frame while reusing the caller's report storage.
    pub fn frame_into(
        &mut self,
        delta: f64,
        report: &mut HostFrameReport,
    ) -> Result<(), ErrorReason> {
        let mut scratch = std::mem::take(&mut self.frame_scratch);
        let result = self.frame_reusing(delta, report, &mut scratch);
        self.frame_scratch = scratch;
        result
    }

    fn frame_reusing(
        &mut self,
        delta: f64,
        report: &mut HostFrameReport,
        scratch: &mut HostFrameScratch,
    ) -> Result<(), ErrorReason> {
        #[cfg(feature = "instrumentation")]
        let _profile = self.profile_scope();
        #[cfg(feature = "instrumentation")]
        let _cpu_trace = crate::profiling::CpuTraceScope::new("HostRuntime.frame");
        #[cfg(feature = "instrumentation")]
        let _allocations = crate::profiling::AllocationScope::new(229, "host.frame");

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
        report.frame = self.frame;
        report.evaluation_order.clear();
        report.publication_order.clear();
        report.publication_errors.clear();
        report
            .worlds
            .retain(|world, _| self.worlds.contains_key(world));
        scratch
            .worlds
            .retain(|world, _| self.worlds.contains_key(world));
        for world in self.worlds.keys() {
            let entry = scratch.worlds.entry(*world).or_default();
            entry.admitted = None;
            entry.context = None;
            entry.evaluated = false;
        }
        scratch.order.clear();
        scratch.order.extend_from_slice(self.topology.order());
        for &world in &scratch.order {
            let references = self.worlds[&world].queued_reference_worlds();
            let result = self
                .reference_context(world, references)
                .expect("live graph World")
                .admit_frame(delta);
            match result {
                Ok(value) => {
                    scratch.worlds.get_mut(&world).unwrap().admitted = Some(value);
                }
                Err(error) => {
                    report.worlds.insert(world, Err(error));
                }
            }
        }
        scratch.order.clear();
        scratch.order.extend_from_slice(self.topology.order());
        for &world in &scratch.order {
            let Some(admitted) = scratch
                .worlds
                .get_mut(&world)
                .and_then(|entry| entry.admitted.take())
            else {
                continue;
            };
            let mut context = scratch
                .worlds
                .get_mut(&world)
                .unwrap()
                .context
                .take()
                .unwrap_or(WorldFrameContext {
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
                    &mut self.web_content,
                );
            access.frame_context = Some(&context);
            let result = access.evaluate_frame(delta, admitted);
            if let Some(fault) = access.fault() {
                report.publication_errors.insert(world, fault.to_string());
            } else if result.is_ok() {
                report.evaluation_order.push(world);
                let mut attachments =
                    std::mem::take(&mut scratch.worlds.get_mut(&world).unwrap().attachments);
                access.completed_attachments(&mut attachments);
                for edge in &attachments {
                    scratch.worlds.entry(edge.child.id).or_default().context =
                        Some(WorldFrameContext {
                            frame: self.frame,
                            delta,
                            placement: multiply(context.placement, edge.placement),
                            viewport: context.viewport,
                            selected_output: (edge.mode != WorldAttachmentMode::Spatial)
                                .then_some(edge.output)
                                .flatten()
                                .filter(|_| edge.surface_extent.is_some()),
                            surface_extent: edge.surface_extent,
                        });
                }
                let entry = scratch.worlds.get_mut(&world).unwrap();
                entry.attachments = attachments;
                entry.evaluated = true;
            }
            report.worlds.insert(world, result);
        }
        self.flush_resource_lifecycle();
        for &world in scratch.order.iter().rev() {
            let entry = scratch.worlds.get_mut(&world).unwrap();
            if !entry.evaluated {
                continue;
            }
            let mut attachments = std::mem::take(&mut entry.attachments);
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
            entry.publication.clear();
            let mut builder = WorldOutputBuilder {
                previous,
                version: id.revision,
                changed: None,
                scratch: &mut entry.publication,
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
                    &mut self.web_content,
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
            let content = builder.finish();
            let previous = self
                .publications
                .latest
                .get(&world)
                .and_then(|id| self.publications.completed.get(id));
            let (lease, web_lease, lease_readers) = if let Some(previous) =
                previous.filter(|previous| {
                    previous.available
                        && previous.data.content.resources == content.resources
                        && previous.data.content.web_images == content.web_images
                }) {
                (
                    previous.lease,
                    previous.web_lease,
                    previous.lease_readers.clone(),
                )
            } else {
                let lease = match self
                    .assets
                    .retain_publication(content.resources.iter().copied())
                {
                    Ok(lease) => lease,
                    Err(error) => {
                        report.publication_errors.insert(world, error);
                        continue;
                    }
                };
                let web_lease = self.web_content.retain_publication(&content.web_images);
                (lease, web_lease, Some(std::rc::Rc::default()))
            };
            let publication = WorldPublication {
                id,
                world: identity,
                tick,
                time,
                attachments,
                content,
            };
            let recycled = self
                .publications
                .recycled
                .iter()
                .position(|entry| entry.data.world == identity)
                .map(|index| self.publications.recycled.swap_remove(index));
            let readers = if let Some(mut recycled) = recycled {
                entry.attachments = std::mem::take(&mut recycled.data.attachments);
                recycled.readers
            } else {
                std::rc::Rc::default()
            };
            self.publications.completed.insert(
                id,
                RetainedPublication {
                    data: publication,
                    lease,
                    web_lease,
                    available: true,
                    readers,
                    lease_readers,
                    retained: false,
                },
            );
            self.publications.latest.insert(world, id);
            report.publication_order.push(world);
        }
        self.retire_publications();
        self.flush_resource_lifecycle();
        Ok(())
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
