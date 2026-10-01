use super::{GuiCanvasPublication, GuiCanvasSemanticView, GuiControlObservation};
use crate::OutputRef;
use crate::services::asset_management::AssetKey;
use crate::systems::canvas::{
    CanvasPaintEntry, CanvasPrimitive, CanvasPrimitiveId, CanvasPublication, CanvasTarget,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Default)]
pub(in crate::world::systems) struct GuiCanvasState {
    entries: BTreeMap<CanvasPrimitiveId, Arc<CanvasPaintEntry>>,
    observations: BTreeMap<CanvasTarget, Arc<GuiControlObservation>>,
    view: Option<Arc<GuiCanvasSemanticView>>,
    pub publication: GuiCanvasPublication,
    changed: bool,
    revision: u64,
}

impl GuiCanvasState {
    pub fn retained_resource(&self, identity: CanvasPrimitiveId) -> Option<AssetKey> {
        match self.entries.get(&identity)?.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } => primitive.resource(),
            CanvasPaintEntry::Attachment(_) => None,
        }
    }

    pub fn invalidate_resource(&mut self, key: AssetKey) {
        self.entries.retain(|_, entry| !matches!(entry.as_ref(), CanvasPaintEntry::Primitive { primitive, .. } if primitive.resource() == Some(key)));
    }

    pub fn primitive(&mut self, primitive: CanvasPrimitive) -> Arc<CanvasPaintEntry> {
        let key = primitive.style().identity;
        if let Some(previous) = self.entries.get(&key)
            && matches!(previous.as_ref(), CanvasPaintEntry::Primitive { primitive: old, .. } if old == &primitive)
        {
            return previous.clone();
        }
        self.revision = self
            .revision
            .checked_add(1)
            .expect("GUI paint revision exhausted");
        let geometry_revision = self
            .entries
            .get(&key)
            .and_then(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    geometry_revision,
                    primitive: previous,
                    ..
                } if same_geometry(previous, &primitive) => Some(*geometry_revision),
                _ => None,
            })
            .unwrap_or(self.revision);
        let entry = Arc::new(CanvasPaintEntry::Primitive {
            geometry_revision,
            material_revision: self.revision,
            primitive,
        });
        self.entries.insert(key, entry.clone());
        entry
    }

    pub fn observation(
        &mut self,
        observation: GuiControlObservation,
    ) -> Arc<GuiControlObservation> {
        let key = observation.hit.target;
        if let Some(previous) = self
            .observations
            .get(&key)
            .filter(|old| old.as_ref() == &observation)
        {
            return previous.clone();
        }
        let observation = Arc::new(observation);
        self.observations.insert(key, observation.clone());
        observation
    }

    pub fn differs(&self, controls: &[Arc<GuiControlObservation>]) -> bool {
        self.view
            .as_ref()
            .is_none_or(|view| view.controls.as_ref() != controls)
    }

    pub fn finish(
        &mut self,
        selection: OutputRef,
        revision: u64,
        controls: Vec<Arc<GuiControlObservation>>,
        entries: &[Arc<CanvasPaintEntry>],
    ) {
        let live_parts: BTreeSet<_> = entries
            .iter()
            .filter_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive,
                    ..
                } => Some(primitive.style().identity),
                _ => None,
            })
            .collect();
        let live_controls: BTreeSet<_> =
            controls.iter().map(|control| control.hit.target).collect();
        self.entries
            .retain(|identity, _| live_parts.contains(identity));
        self.observations
            .retain(|identity, _| live_controls.contains(identity));
        if self
            .view
            .as_ref()
            .is_some_and(|view| view.selection == selection && view.input_revision == revision)
        {
            return;
        }
        self.view = Some(Arc::new(GuiCanvasSemanticView {
            selection,
            input_revision: revision,
            controls: controls.into(),
        }));
        self.changed = true;
    }

    /// Publish the semantic view of the canvas output, or none while the
    /// canvas has no output.
    pub fn complete(&mut self, output: Option<&CanvasPublication>) {
        let live = self
            .view
            .as_ref()
            .is_some_and(|view| output.is_some_and(|output| output.selection == view.selection));
        if !live && self.view.take().is_some() {
            self.changed = true;
        }
        if output.is_none() {
            self.entries.clear();
            self.observations.clear();
        }
        if !self.changed {
            return;
        }
        self.publication.revision = self
            .publication
            .revision
            .checked_add(1)
            .expect("GUI semantic revision exhausted");
        self.publication.views = Arc::new(
            self.view
                .iter()
                .map(|view| (view.selection, view.clone()))
                .collect(),
        );
        self.changed = false;
    }
}

fn same_geometry(previous: &CanvasPrimitive, next: &CanvasPrimitive) -> bool {
    match (previous, next) {
        (
            CanvasPrimitive::Glyphs {
                font: old_font,
                font_size: old_size,
                glyphs: old_glyphs,
                ..
            },
            CanvasPrimitive::Glyphs {
                font,
                font_size,
                glyphs,
                ..
            },
        ) => old_font == font && old_size == font_size && old_glyphs == glyphs,
        (
            CanvasPrimitive::Drawing {
                drawing: old,
                ..
            },
            CanvasPrimitive::Drawing {
                drawing,
                ..
            },
        ) => old == drawing,
        (
            CanvasPrimitive::Bitmap {
                bitmap: old,
                size: old_size,
                ..
            },
            CanvasPrimitive::Bitmap {
                bitmap,
                size,
                ..
            },
        ) => old == bitmap && old_size == size,
        (
            CanvasPrimitive::Box {
                size: old_size,
                corner_radius: old_radius,
                ..
            },
            CanvasPrimitive::Box {
                size,
                corner_radius,
                ..
            },
        ) => old_size == size && old_radius == corner_radius,
        _ => false,
    }
}
