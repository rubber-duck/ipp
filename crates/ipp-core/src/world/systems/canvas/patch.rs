//! Patched evaluation: a change that affects neither structure, GUI layout nor
//! layers re-walks only the subtrees of the changed entities and replaces
//! their output in the previous publication in place.
//!
//! Each changed entity's subtree is one contiguous range of the tree-order
//! output its [`CanvasWalkRecord`] marks, and the stack its walk re-enters is
//! seeded from its ancestors' records, so inherited tint, opacity, clip,
//! translation and scale reach every descendant. A paint property write
//! re-reads only its paint instance. The publication's layers and the
//! published index of every entry, hit and overlay stay as they were.
//!
//! The patch walks the whole canvas instead when a changed entity's subtree
//! would resolve to other layers, a paint changed its lifetime, or the
//! re-walked subtree produces a different number of entries, hits, controls or
//! overlays, or an entry on another layer. A re-walk that falls back in the
//! middle has updated retained leaves and parts, which the whole walk then
//! finds unchanged, so it passes on what moved. Under `checked-invariants`
//! every patch is compared with the whole walk of the same state.

use super::system_state::{CanvasWalkRecord, interaction_counts};
use super::update::{
    CanvasOutputChanges, entry_paint, incarnation, paint_instance, paint_instances,
    retained_resources,
};
use super::walk::CanvasWalk;
use super::*;
use crate::systems::SystemRuntimeAccess;
use crate::systems::gui::presentation::{GuiControlObservation, GuiOverlayObservation};
use crate::world::WorldSimulationState;
use crate::{ComponentValue, EntityId, OutputRef};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A changed entity whose subtree a patch re-walks, at its walk position, with
/// the walk positions of its ancestors, root first.
struct CanvasPatchRoot {
    position: usize,
    ancestors: Vec<usize>,
}

/// Replacements one patch stages before it publishes, by published index.
#[derive(Default)]
struct CanvasPatchOutput {
    records: Vec<(usize, Vec<CanvasWalkRecord>)>,
    entries: Vec<(usize, Arc<CanvasPaintEntry>)>,
    hits: Vec<(usize, CanvasHit)>,
    controls: Vec<(usize, Arc<GuiControlObservation>)>,
    overlays: Vec<(usize, GuiOverlayObservation)>,
    forgotten_parts: Vec<CanvasPrimitiveId>,
    forgotten_controls: Vec<CanvasTarget>,
    bounds: Vec<(EntityId, [f32; 4])>,
    interaction: [i64; 4],
    /// A replaced entry changed the resource it retains.
    resources: bool,
    /// A replaced entry changed the paint it names.
    paint_targets: bool,
    layout: bool,
    entities: usize,
    primitives: usize,
}

impl CanvasSystem {
    /// Patch the previous publication with the changes since it, or walk the
    /// whole canvas when the patch cannot apply them.
    pub(super) fn patch_canvas(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
    ) {
        let world = &*context.world;
        let Some(roots) = self.patch_roots(world).filter(|_| self.paints_hold(world)) else {
            self.evaluate_canvas(context, selection, extent, density, false);
            return;
        };
        // Plot row hits are independent of ordinary Canvas hit/entry ranges.
        // Rebuild only when a patched subtree contains a chart; other GUI
        // subtrees preserve the incremental patch path and retained Plot hits.
        if self
            .plot
            .and_then(|binding| context.dependency(binding))
            .is_some_and(|plot| {
                roots.iter().any(|root| {
                    let records = &self.state.walk.records;
                    records[root.position..records[root.position].subtree_end]
                        .iter()
                        .any(|record| plot.canvas_charts(record.entity).next().is_some())
                })
            })
        {
            self.evaluate_canvas(context, selection, extent, density, false);
            return;
        }
        #[cfg(feature = "checked-invariants")]
        let whole = CanvasSystem {
            state: self.state.clone(),
            layout: self.layout,
            gui: self.gui,
            plot: self.plot,
        };
        let output = match self.patch_walk(context, &roots, extent, density) {
            Ok(output) => output,
            Err(layout_changed) => {
                self.evaluate_canvas(context, selection, extent, density, layout_changed);
                return;
            }
        };
        self.publish_patch(context, selection, extent, density, output);
        #[cfg(feature = "checked-invariants")]
        self.check_patch(whole, context, selection, extent, density);
    }

    /// The outermost changed entities on the walk, whose subtrees resolve to
    /// the layers they had; none when a layer changed.
    fn patch_roots(&self, world: &WorldSimulationState) -> Option<Vec<CanvasPatchRoot>> {
        let walk = &self.state.walk;
        let mut positions: Vec<usize> = self
            .state
            .patch
            .subtrees
            .iter()
            .filter_map(|entity| walk.positions.get(entity).copied())
            .collect();
        positions.sort_unstable();
        let mut roots = Vec::new();
        let mut covered = 0;
        for position in positions {
            if position < covered {
                continue;
            }
            covered = walk.records[position].subtree_end;
            let mut ancestors = Vec::new();
            let mut parent = parent(world, walk.records[position].entity);
            while let Some(entity) = parent {
                ancestors.push(*walk.positions.get(&entity)?);
                parent = self::parent(world, entity);
            }
            ancestors.reverse();
            if !self.layers_hold(world, position, ancestors.last().copied()) {
                return None;
            }
            roots.push(CanvasPatchRoot {
                position,
                ancestors,
            });
        }
        Some(roots)
    }

    /// Whether every entity of the subtree at walk `root`, under the entity at
    /// walk position `parent`, still resolves to its recorded layer.
    fn layers_hold(
        &self,
        world: &WorldSimulationState,
        root: usize,
        parent: Option<usize>,
    ) -> bool {
        let records = &self.state.walk.records;
        let base = parent.map_or(0, |parent| records[parent].placed.layer);
        let mut planes: Vec<(EntityId, u32)> = Vec::new();
        records[root..records[root].subtree_end]
            .iter()
            .all(|record| {
                let parent = self::parent(world, record.entity);
                while planes
                    .last()
                    .is_some_and(|(ancestor, _)| Some(*ancestor) != parent)
                {
                    planes.pop();
                }
                let authored = world
                    .components
                    .canvas_style(record.entity.index() as usize)
                    .map_or(0, |style| style.layer);
                let plane = super::layers::resolve(
                    planes.last().map_or(base, |(_, plane)| *plane),
                    authored,
                );
                planes.push((record.entity, plane));
                plane == record.placed.layer
            })
    }

    /// Whether every changed paint the publication carries keeps its lifetime.
    fn paints_hold(&self, world: &WorldSimulationState) -> bool {
        let Some(publication) = &self.state.publication else {
            return false;
        };
        publication
            .paints
            .iter()
            .filter(|paint| self.state.patch.paints.contains(&paint.target.entity))
            .all(|paint| {
                incarnation(world, paint.target.entity, ComponentValue::CANVAS_PAINT)
                    == Some(paint.target.incarnation)
            })
    }

    /// Re-walk each root's subtree and stage its replacements; on a change the
    /// patch cannot apply, whether anything walked so far moved.
    fn patch_walk(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        roots: &[CanvasPatchRoot],
        extent: [f32; 2],
        density: f32,
    ) -> Result<CanvasPatchOutput, bool> {
        let inputs = self.walk_inputs(context, extent, density);
        let mut output = CanvasPatchOutput::default();
        for root in roots {
            let (start, end, subtree_end) = {
                let record = &self.state.walk.records[root.position];
                (record.start, record.end, record.subtree_end)
            };
            let mut walk = CanvasWalk::at(root.position, start);
            for &ancestor in &root.ancestors {
                walk.seed(ancestor, &self.state.walk.records[ancestor]);
            }
            for position in root.position..subtree_end {
                let (entity, layer) = {
                    let record = &self.state.walk.records[position];
                    (record.entity, record.placed.layer)
                };
                self.visit(&inputs, &mut walk, position, entity, layer);
            }
            self.unwind(&mut walk, None, subtree_end);
            output.layout |= walk.layout_changed;
            output.entities += subtree_end - root.position;
            output.primitives += walk.entries.len();
            if walk.entries.len() != (end.entries - start.entries) as usize
                || walk.hits.len() != (end.hits - start.hits) as usize
                || walk.controls.len() != (end.controls - start.controls) as usize
                || walk.overlays.len() != (end.overlays - start.overlays) as usize
                || !self.stage(&mut output, &walk)
            {
                return Err(output.layout);
            }
            let previous = &self.state.walk.records[root.position..subtree_end];
            for (before, after) in previous.iter().zip(&walk.records) {
                let before = interaction_counts(before.interaction);
                let after = interaction_counts(after.interaction);
                for flag in 0..4 {
                    output.interaction[flag] += i64::from(after[flag]) - i64::from(before[flag]);
                }
            }
            output
                .records
                .push((root.position, std::mem::take(&mut walk.records)));
            output.bounds.append(&mut walk.bounds);
        }
        Ok(output)
    }

    /// Stage the replacements of one re-walked subtree, whose output has the
    /// recorded lengths; false when an entry, hit or overlay changed layer.
    fn stage(&self, output: &mut CanvasPatchOutput, walk: &CanvasWalk) -> bool {
        let (Some(publication), Some(view)) = (&self.state.publication, self.state.gui.view())
        else {
            return false;
        };
        let order = &self.state.walk;
        let start = walk.records[0].start;

        let mut parts = BTreeSet::new();
        for (index, after) in walk.entries.iter().enumerate() {
            let at = order.entry(start.entries as usize + index);
            let before = &publication.entries[at];
            if before.layer() != after.layer() {
                return false;
            }
            if let CanvasPaintEntry::Primitive {
                primitive,
                ..
            } = before.as_ref()
            {
                parts.insert(primitive.style().identity);
            }
            if Arc::ptr_eq(before, after) {
                continue;
            }
            output.resources |= resource(before) != resource(after);
            output.paint_targets |= entry_paint(before) != entry_paint(after);
            output.entries.push((at, after.clone()));
        }
        for after in &walk.entries {
            if let CanvasPaintEntry::Primitive {
                primitive,
                ..
            } = after.as_ref()
            {
                parts.remove(&primitive.style().identity);
            }
        }
        output.forgotten_parts.extend(parts);

        for (index, after) in walk.hits.iter().enumerate() {
            let at = order.hit(start.hits as usize + index);
            let before = &publication.hits[at];
            if before.layer != after.layer {
                return false;
            }
            if before != after {
                output.hits.push((at, after.clone()));
            }
        }

        let mut controls = BTreeSet::new();
        for (index, after) in walk.controls.iter().enumerate() {
            let at = start.controls as usize + index;
            let before = &view.controls[at];
            controls.insert(before.hit.target);
            if before != after {
                output.controls.push((at, after.clone()));
            }
        }
        for after in &walk.controls {
            controls.remove(&after.hit.target);
        }
        output.forgotten_controls.extend(controls);

        for (index, after) in walk.overlays.iter().enumerate() {
            let at = order.overlay(start.overlays as usize + index);
            let before = &view.overlays[at];
            if before.layer != after.layer {
                return false;
            }
            if before != after {
                output.overlays.push((at, after.clone()));
            }
        }
        true
    }

    /// Publish the staged replacements and re-read changed paints.
    fn publish_patch(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
        output: CanvasPatchOutput,
    ) {
        let world = &*context.world;
        let assets = context.asset_resources();
        let previous = self.state.publication.take().unwrap();
        for (position, records) in output.records {
            for (offset, record) in records.into_iter().enumerate() {
                self.state.walk.records[position + offset] = record;
            }
        }
        for (count, delta) in self
            .state
            .walk
            .interaction
            .iter_mut()
            .zip(output.interaction)
        {
            *count = u32::try_from(i64::from(*count) + delta).expect("Canvas interaction count");
        }
        self.state
            .gui
            .forget(output.forgotten_parts, output.forgotten_controls);
        self.state.bounds.extend(output.bounds);

        let replaced = output.entries.len();
        let entries = (!output.entries.is_empty()).then(|| {
            let mut entries = previous.entries.to_vec();
            let mut indices = Vec::with_capacity(output.entries.len());
            for (at, entry) in output.entries {
                entries[at] = entry;
                indices.push(at as u32);
            }
            indices.sort_unstable();
            (
                Arc::<[_]>::from(entries),
                CanvasPaintChanges {
                    base: previous.paint_revision,
                    entries: indices.into(),
                },
            )
        });
        let current = entries
            .as_ref()
            .map_or(&previous.entries, |(entries, _)| entries);

        // A replaced entry naming another paint re-reads every paint;
        // otherwise only the changed paints are read again.
        let mut reread = 0;
        let paints = if output.paint_targets {
            let paints = paint_instances(world, assets, current);
            reread = paints.len();
            paints
        } else {
            previous
                .paints
                .iter()
                .map(|paint| {
                    if !self.state.patch.paints.contains(&paint.target.entity) {
                        return paint.clone();
                    }
                    reread += 1;
                    paint_instance(world, assets, paint.target)
                        .expect("a changed paint kept its lifetime")
                })
                .collect()
        };
        let changed_paints = *paints != *previous.paints;
        let resources = (output.resources
            || (changed_paints
                && paints
                    .iter()
                    .map(|paint| paint.shader)
                    .ne(previous.paints.iter().map(|paint| paint.shader))))
        .then(|| retained_resources(current, &paints))
        .filter(|resources| *resources != previous.resources);

        let view = self.state.gui.view().expect("patched canvas view");
        let changed_input =
            !output.hits.is_empty() || !output.controls.is_empty() || !output.overlays.is_empty();
        let replace = |published: &[CanvasHit], replacements: Vec<(usize, CanvasHit)>| {
            let mut published = published.to_vec();
            for (at, hit) in replacements {
                published[at] = hit;
            }
            Arc::<[_]>::from(published)
        };
        let hits = changed_input.then(|| replace(&previous.hits, output.hits));
        let observations = changed_input.then(|| {
            let mut controls = view.controls.to_vec();
            for (at, control) in output.controls {
                controls[at] = control;
            }
            let mut overlays = view.overlays.to_vec();
            for (at, overlay) in output.overlays {
                overlays[at] = overlay;
            }
            (Arc::<[_]>::from(controls), Arc::<[_]>::from(overlays))
        });

        self.state.work = CanvasWork {
            patched: true,
            entities: output.entities,
            primitives: output.primitives,
            replaced,
            paints: reread,
            ..Default::default()
        };
        let layers = previous.layers.clone();
        let input_revision = self.install(
            selection,
            extent,
            density,
            Some(previous),
            CanvasOutputChanges {
                layout: output.layout,
                entries: entries.map(|(entries, changes)| (entries, Some(changes))),
                hits,
                plot_hits: None,
                layers: &layers,
                interaction: self.state.walk.priority(),
                paints: changed_paints.then(|| paints.into()),
                resources,
            },
        );
        if let Some((controls, overlays)) = observations {
            self.state
                .gui
                .replace_view(selection, input_revision, controls, overlays);
        }
    }

    /// Panic unless the patched state is the state the whole walk of the
    /// same changes evaluates from the state before the patch.
    #[cfg(feature = "checked-invariants")]
    fn check_patch(
        &self,
        mut whole: CanvasSystem,
        context: &SystemRuntimeAccess<'_>,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
    ) {
        whole.evaluate_canvas(context, selection, extent, density, false);
        let (patched, walked) = (&self.state, &whole.state);
        let same = |a: &CanvasPublication, b: &CanvasPublication| {
            a == b
                && a.logical_extent == b.logical_extent
                && a.units_per_metre == b.units_per_metre
                && a.entries == b.entries
                && a.hits == b.hits
                && a.plot_hits == b.plot_hits
                && a.layers == b.layers
                && a.paints == b.paints
                && a.paint_changes == b.paint_changes
                && a.resources == b.resources
        };
        assert!(
            match (&patched.publication, &walked.publication) {
                (Some(a), Some(b)) => same(a, b),
                _ => false,
            },
            "checked invariant: a patched canvas publishes what the whole walk publishes\n\
             patched: {:#?}\nwhole: {:#?}",
            patched.publication,
            walked.publication,
        );
        assert!(
            patched.gui.same(&walked.gui),
            "checked invariant: a patched canvas retains the GUI parts and observations of the whole walk"
        );
        assert!(
            patched.leaves == walked.leaves
                && patched.slots == walked.slots
                && patched.walk == walked.walk
                && patched.revision == walked.revision,
            "checked invariant: a patched canvas retains the leaves, slots and walk of the whole walk"
        );
        let written: std::collections::BTreeMap<_, _> = patched.bounds.iter().copied().collect();
        for (entity, bounds) in &walked.bounds {
            let current = written.get(entity).copied().or_else(|| {
                context
                    .world
                    .components
                    .canvas_bounds(entity.index() as usize)
                    .map(|bounds| [bounds.x, bounds.y, bounds.width, bounds.height])
            });
            assert_eq!(
                current,
                Some(*bounds),
                "checked invariant: a patched canvas writes the bounds the whole walk writes"
            );
        }
    }
}

fn parent(world: &WorldSimulationState, entity: EntityId) -> Option<EntityId> {
    world
        .state
        .links
        .effective(entity)
        .and_then(|link| link.parent)
}

fn resource(entry: &CanvasPaintEntry) -> Option<crate::services::asset_management::AssetKey> {
    match entry {
        CanvasPaintEntry::Primitive {
            primitive,
            ..
        } => primitive.resource(),
        CanvasPaintEntry::Attachment(_) => None,
    }
}
