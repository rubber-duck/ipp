//! Canvas System work counters, separate from semantic output.

/// Actual Canvas System work in the latest evaluated frame.
///
/// A frame without relevant changes does no work. A change that affects
/// neither structure, GUI layout nor layers patches the publication: it walks
/// only the subtrees of the changed entities and replaces their entries in
/// place, and a paint property write re-reads only its paint. Every other
/// change walks the whole canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanvasWork {
    /// Whether the evaluation walked the whole canvas.
    pub full: bool,
    /// Whether the evaluation patched the previous publication in place.
    pub patched: bool,
    /// Entities walked.
    pub entities: usize,
    /// Primitives the walked entities produced.
    pub primitives: usize,
    /// Published entries a patch replaced.
    pub replaced: usize,
    /// Paint instances a patch re-read.
    pub paints: usize,
}

impl crate::WorldContext<'_> {
    /// Latest evaluated Canvas System work; observing never evaluates a World.
    pub fn canvas_work(&self) -> Option<CanvasWork> {
        Some(
            self.system::<super::CanvasSystem>(super::CanvasSystem::ID)?
                .state
                .work,
        )
    }
}
