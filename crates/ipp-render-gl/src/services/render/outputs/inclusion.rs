use std::collections::{BTreeMap, BTreeSet};

use super::selection_set::OutputSelectionSet;

use ipp_core::{OutputPublicationObservation, OutputRef, WorldPublicationId};

#[derive(Default)]
pub(in crate::services::render) struct OutputSources {
    pub sources: BTreeMap<OutputRef, WorldPublicationId>,
    pub stale_image: bool,
    pub collect_image: bool,
    pub image_outputs: OutputSelectionSet,
    /// Optional images represented by pixels, regardless of publication freshness.
    pub represented_images: OutputSelectionSet,
}

#[derive(Default)]
pub(in crate::services::render) struct OutputInclusions {
    requested: BTreeSet<OutputRef>,
    pub active: OutputSources,
    images: Vec<(OutputRef, OutputSources)>,
    free: Vec<OutputSources>,
}

impl OutputInclusions {
    pub fn reset_frame(&mut self) {
        self.active.sources.clear();
        self.active.stale_image = false;
        self.active.collect_image = false;
        self.active.image_outputs.clear();
        self.active.represented_images.clear();
        self.free
            .extend(self.images.drain(..).map(|(_, image)| image));
    }

    pub fn begin(&mut self, outputs: &[OutputPublicationObservation]) {
        self.reset_frame();
        self.requested.clear();
        self.requested
            .extend(outputs.iter().map(|entry| entry.output));
    }

    pub fn push(&mut self) -> OutputSources {
        let mut next = self.free.pop().unwrap_or_default();
        next.sources.clear();
        next.stale_image = false;
        next.collect_image = false;
        next.image_outputs.clear();
        next.represented_images.clear();
        std::mem::replace(&mut self.active, next)
    }

    pub fn record(&mut self, output: OutputRef, publication: WorldPublicationId) {
        if self.active.stale_image {
            return;
        }

        if self.active.collect_image {
            self.active.image_outputs.insert(output);
        }

        if self.requested.contains(&output) {
            self.active.sources.insert(output, publication);
        }
    }

    pub fn observing(&self) -> bool {
        self.active.collect_image || !self.requested.is_empty()
    }

    pub fn save_image(&mut self, output: OutputRef, parent: OutputSources) {
        let image = std::mem::replace(&mut self.active, parent);
        self.images.push((output, image));
    }

    pub fn composite(&mut self, output: OutputRef) {
        if let Some((_, image)) = self.images.iter().rev().find(|entry| entry.0 == output) {
            if self.active.collect_image {
                self.active
                    .image_outputs
                    .extend(image.image_outputs.iter().copied());
                self.active
                    .represented_images
                    .extend(image.represented_images.iter().copied());
            }
            self.active.sources.extend(
                image
                    .sources
                    .iter()
                    .map(|(output, publication)| (*output, *publication)),
            );
            self.active.stale_image |= image.stale_image;
        }
    }

    pub fn finish(&mut self, outputs: &mut [OutputPublicationObservation], completed: bool) {
        for output in outputs {
            output.publication = completed
                .then(|| self.active.sources.get(&output.output).copied())
                .flatten();
        }

        self.reset_frame();
        self.requested.clear();
    }
}
