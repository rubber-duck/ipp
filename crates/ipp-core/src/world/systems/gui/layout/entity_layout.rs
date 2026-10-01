use super::entity_evaluation::evaluate;
use crate::systems::canvas::{CanvasGeometry, logical_extent};
use crate::systems::{SystemCommitContext, SystemRuntimeAccess};
use crate::{ComponentValue, EntityId, OutputRef, WorldRef};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Derived ordinary-entity geometry, never an authored property or an alternate tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiEntityLayout {
    /// Parent-local logical origin before Canvas visual transforms.
    pub origin: [f32; 2],
    /// Settled local logical extent before visual transforms.
    pub size: [f32; 2],
    /// Local padding offset of this entity's own content, after its visual transform.
    /// Outer bounds and clips stay at the entity origin.
    pub content_offset: [f32; 2],
    /// Whether this box clips descendants through the shared Canvas mapping.
    pub clip: bool,
    /// False for invalid layout declarations; descendants cannot fall back to raw placement.
    pub available: bool,
}

/// Observable reason a declared layout branch cannot currently produce content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiEntityLayoutDiagnostic {
    /// The existing bounded GUI constraint policy was exceeded, not a core-tree failure.
    DepthLimit {
        /// First entity outside the supported constraint depth.
        entity: EntityId,
        /// Maximum supported managed-layout depth, with the layout root at zero.
        limit: usize,
    },
    /// A single-child container has additional children; their branches are suppressed.
    ExtraChild {
        /// The excluded child, still present in the authoritative core tree.
        entity: EntityId,
    },
}

/// Actual ordinary-layout work, excluding raw Canvas preparation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiEntityLayoutWork {
    /// Canvas evaluations repeated because a layout, content, extent or resource input changed.
    pub reflows: u64,
    /// Managed entities visited by those constraint passes, including unavailable branches.
    pub visited_entities: u64,
    /// Successful calls to the shared text measurement algorithm.
    pub text_measurements: u64,
    /// Text runs retained without measurement during constraint passes.
    pub reused_texts: u64,
}

impl GuiEntityLayoutWork {
    fn accumulate(&mut self, work: Self) {
        self.reflows += work.reflows;
        self.visited_entities += work.visited_entities;
        self.text_measurements += work.text_measurements;
        self.reused_texts += work.reused_texts;
    }
}

/// Current-pass and cumulative ordinary-layout work counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuiEntityLayoutStatistics {
    /// Work of the latest scheduled pass, zero on unchanged or visual-only passes.
    pub latest: GuiEntityLayoutWork,
    /// Work since this System instance was created.
    pub total: GuiEntityLayoutWork,
}

pub(in crate::world::systems) struct GuiEntityLayoutView {
    pub revision: u64,
    pub extent: [f32; 2],
    pub density: f32,
    pub placements: BTreeMap<EntityId, GuiEntityLayout>,
    pub geometry: BTreeMap<(EntityId, u16), Option<CanvasGeometry>>,
    pub text_constraints: BTreeMap<EntityId, f32>,
    pub control_labels:
        BTreeMap<EntityId, super::super::presentation::measurement::GuiControlLabel>,
    /// Scroll geometry and normalized positions written to the controls' fields.
    pub scrolls: BTreeMap<EntityId, super::scroll_layout::GuiScrollLayout>,
    pub diagnostics: Vec<GuiEntityLayoutDiagnostic>,
    pub work: GuiEntityLayoutWork,
}

#[derive(Default)]
pub(super) struct GuiEntityLayoutState {
    pub view: Option<Arc<GuiEntityLayoutView>>,
    dirty: BTreeSet<EntityId>,
    /// Controls whose fields changed; their scope reflows only if the
    /// measured label text changed.
    label_dirty: BTreeSet<EntityId>,
    geometry_dirty: BTreeSet<EntityId>,
    structure_dirty: bool,
    resources_dirty: bool,
    initialized: bool,
    revision: u64,
    text_revision: u64,
    native_entity: Option<EntityId>,
    /// Whether the latest pass evaluated the canvas, whose scroll fields are written.
    evaluated: bool,
    statistics: GuiEntityLayoutStatistics,
}

pub(in crate::world::systems::gui) fn entity_layout_input(component: u16) -> bool {
    matches!(
        component,
        ComponentValue::GUI_LAYOUT
            | ComponentValue::CANVAS_BOX
            | ComponentValue::CANVAS_TEXT
            | ComponentValue::CANVAS_GLYPH_RUN
            | ComponentValue::CANVAS_DRAWING
            | ComponentValue::CANVAS_BITMAP
            | ComponentValue::SURFACE
            | ComponentValue::GUI_BUTTON
            | ComponentValue::GUI_CHECKBOX
            | ComponentValue::GUI_SLIDER
            | ComponentValue::GUI_TEXT_INPUT
            | ComponentValue::GUI_FONT
            | ComponentValue::GUI_SCROLL_VIEW
            | ComponentValue::GUI_VIRTUAL_LIST
            | ComponentValue::GUI_VIRTUAL_ITEM
    )
}

/// Control components whose value fields do not affect layout; only a label
/// change reflows their scope.
fn label_input(component: u16) -> bool {
    matches!(
        component,
        ComponentValue::GUI_BUTTON
            | ComponentValue::GUI_CHECKBOX
            | ComponentValue::GUI_SLIDER
            | ComponentValue::GUI_TEXT_INPUT
    )
}

impl GuiEntityLayoutState {
    pub fn dirty(&mut self, entity: EntityId, component: u16) {
        if label_input(component) {
            self.label_dirty.insert(entity);
        } else if entity_layout_input(component) {
            self.dirty.insert(entity);
        }
        if matches!(
            component,
            ComponentValue::CANVAS_BOX
                | ComponentValue::CANVAS_TEXT
                | ComponentValue::CANVAS_GLYPH_RUN
                | ComponentValue::CANVAS_DRAWING
                | ComponentValue::CANVAS_BITMAP
        ) {
            self.geometry_dirty.insert(entity);
        }
    }

    pub fn resources_changed(&mut self) {
        self.resources_dirty = true;
    }

    pub fn before_commit(&mut self, context: &mut SystemCommitContext<'_>) {
        for entity in context.changed_entity_links() {
            self.structure_dirty = true;
            self.dirty.insert(entity);
        }
        for (entity, component) in context.changed_components() {
            let retained = context.retains_component(entity, component);
            if label_input(component) && !retained {
                self.dirty.insert(entity);
            } else {
                self.dirty(entity, component);
            }
        }
    }

    pub fn update(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        gui: Option<&super::super::GuiSystem>,
    ) {
        self.statistics.latest = Default::default();

        let world = &*context.world;
        let text_revision = gui.map_or(0, super::super::GuiSystem::local_text_revision);
        let native_entity = gui.and_then(super::super::GuiSystem::native_text_entity);
        if text_revision != self.text_revision || native_entity != self.native_entity {
            self.label_dirty.extend(self.native_entity);
            self.label_dirty.extend(native_entity);
        }
        self.text_revision = text_revision;
        self.native_entity = native_entity;
        let initial = !self.initialized;
        for entity in std::mem::take(&mut self.label_dirty) {
            if !self.dirty.contains(&entity)
                && world.state.entities.contains_key(&entity)
                && self.label_changed(context, gui, entity)
            {
                self.dirty.insert(entity);
            }
        }
        let dirty = self.structure_dirty || !self.dirty.is_empty();
        self.evaluated = false;
        // The layout reads the canvas state the Canvas System committed at the
        // mutation boundary; the canvas evaluates after this pass.
        let selection = OutputRef::canvas(WorldRef {
            id: world.id,
            incarnation: world.identity,
        });
        let extent = context.canvas_state().and_then(|canvas| {
            Some((
                logical_extent(canvas, selection, context.frame_context())?,
                canvas.units_per_metre,
            ))
        });
        match extent {
            None => self.view = None,
            Some((extent, density)) => {
                let previous = self.view.as_ref();
                if initial
                    || self.resources_dirty
                    || dirty
                    || previous.is_none_or(|view| view.extent != extent || view.density != density)
                {
                    self.revision = self
                        .revision
                        .checked_add(1)
                        .expect("GUI layout revision exhausted");
                    let view = evaluate(
                        context,
                        extent,
                        density,
                        previous.map(Arc::as_ref),
                        &self.geometry_dirty,
                        self.resources_dirty,
                        self.revision,
                        gui,
                    );
                    self.statistics.latest.accumulate(view.work);
                    self.statistics.total.accumulate(view.work);
                    self.view = Some(Arc::new(view));
                    self.evaluated = true;
                }
            }
        }
        self.dirty.clear();
        self.geometry_dirty.clear();
        self.structure_dirty = false;
        self.resources_dirty = false;
        self.initialized = true;
    }

    /// Whether a control's label text differs from its last measurement.
    fn label_changed(
        &self,
        context: &SystemRuntimeAccess<'_>,
        gui: Option<&super::super::GuiSystem>,
        entity: EntityId,
    ) -> bool {
        let Some(gui) = gui else {
            return false;
        };
        let current = super::super::presentation::measurement::label_text(context, gui, entity);
        let previous = self
            .view
            .as_ref()
            .and_then(|view| view.control_labels.get(&entity));
        match (current, previous) {
            (Some(current), Some(previous)) => !previous.measures(&current),
            (None, None) => false,
            _ => true,
        }
    }

    /// Write the scroll geometry and normalized positions of the latest pass
    /// to the controls' fields.
    pub fn write_scroll_fields(
        &self,
        components: &mut crate::components::registry::ComponentStorage,
        state: &crate::world::WorldEntityState,
    ) {
        let Some(view) = self.view.as_ref().filter(|_| self.evaluated) else {
            return;
        };
        for (&entity, scroll) in &view.scrolls {
            if super::super::local::control::component_incarnation(state, entity, scroll.component)
                == Some(scroll.incarnation)
            {
                scroll.write(components, entity);
            }
        }
    }

    pub fn snapshot(&self, entity: EntityId) -> Option<GuiEntityLayout> {
        self.view.as_ref()?.placements.get(&entity).copied()
    }
}

impl crate::WorldContext<'_> {
    /// Last evaluated ordinary layout. Querying does not require presentation or mutate layout.
    pub fn gui_entity_layout(&self, entity: EntityId) -> Option<GuiEntityLayout> {
        self.system::<super::GuiLayoutSystem>(super::GuiLayoutSystem::ID)?
            .entity_layout
            .snapshot(entity)
    }

    /// Diagnostics from the last completed constraint evaluation of the canvas.
    pub fn gui_entity_layout_diagnostics(&self) -> Option<&[GuiEntityLayoutDiagnostic]> {
        Some(
            &self
                .system::<super::GuiLayoutSystem>(super::GuiLayoutSystem::ID)?
                .entity_layout
                .view
                .as_ref()?
                .diagnostics,
        )
    }

    /// Actual constraint and text work, read on demand without hot-path logs.
    pub fn gui_entity_layout_statistics(&self) -> Option<GuiEntityLayoutStatistics> {
        Some(
            self.system::<super::GuiLayoutSystem>(super::GuiLayoutSystem::ID)?
                .entity_layout
                .statistics,
        )
    }
}
