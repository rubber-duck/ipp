use std::collections::{BTreeMap, BTreeSet};

use ipp_core::{OutputPublicationObservation, OutputRef, WorldPublicationId};

#[derive(Default)]
pub(super) struct OutputSources {
    pub sources: BTreeMap<OutputRef, WorldPublicationId>,
    #[cfg(feature = "surfaces")]
    pub stale_image: bool,
    #[cfg(feature = "surfaces")]
    pub collect_image: bool,
    #[cfg(feature = "surfaces")]
    pub image_outputs: BTreeSet<OutputRef>,
}

#[derive(Default)]
pub(super) struct OutputInclusions {
    requested: BTreeSet<OutputRef>,
    pub active: OutputSources,
    #[cfg(feature = "surfaces")]
    images: BTreeMap<OutputRef, OutputSources>,
}

impl OutputInclusions {
    pub fn reset_frame(&mut self) {
        self.active = OutputSources::default();
        #[cfg(feature = "surfaces")]
        self.images.clear();
    }

    pub fn begin(&mut self, outputs: &[OutputPublicationObservation]) {
        *self = Self::default();
        self.requested
            .extend(outputs.iter().map(|entry| entry.output));
    }

    pub fn record(&mut self, output: OutputRef, publication: WorldPublicationId) {
        #[cfg(feature = "surfaces")]
        if self.active.stale_image {
            return;
        }

        #[cfg(feature = "surfaces")]
        if self.active.collect_image {
            self.active.image_outputs.insert(output);
        }

        if self.requested.contains(&output) {
            self.active.sources.insert(output, publication);
        }
    }

    #[cfg(feature = "surfaces")]
    pub fn observing(&self) -> bool {
        self.active.collect_image || !self.requested.is_empty()
    }

    #[cfg(feature = "surfaces")]
    pub fn save_image(&mut self, output: OutputRef, parent: OutputSources) {
        let image = std::mem::replace(&mut self.active, parent);
        self.images.insert(output, image);
    }

    #[cfg(feature = "surfaces")]
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
