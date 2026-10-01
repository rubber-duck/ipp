use std::collections::{BTreeMap, BTreeSet};

use ipp_core::{OutputPublicationObservation, OutputRef, WorldPublicationId};

#[derive(Default)]
pub(super) struct OutputSources {
    pub sources: BTreeMap<OutputRef, WorldPublicationId>,
    pub stale_image: bool,
    pub collect_image: bool,
    pub image_outputs: BTreeSet<OutputRef>,
}

#[derive(Default)]
pub(super) struct OutputInclusions {
    requested: BTreeSet<OutputRef>,
    pub active: OutputSources,
    images: BTreeMap<OutputRef, OutputSources>,
}

impl OutputInclusions {
    pub fn reset_frame(&mut self) {
        self.active = OutputSources::default();
        self.images.clear();
    }

    pub fn begin(&mut self, outputs: &[OutputPublicationObservation]) {
        *self = Self::default();
        self.requested
            .extend(outputs.iter().map(|entry| entry.output));
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
        self.images.insert(output, image);
    }

    pub fn composite(&mut self, output: OutputRef) {
        if let Some(image) = self.images.get(&output) {
            if self.active.collect_image {
                self.active
                    .image_outputs
                    .extend(image.image_outputs.iter().copied());
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

        *self = Self::default();
    }
}
