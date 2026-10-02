use super::system_state::{CanvasGeometry, CanvasWalkState, interaction_counts};
use super::walk::{CanvasWalk, CanvasWalkInputs, Placement};
use super::*;
use crate::services::asset_management::{
    AssetKey, AssetManagementService, AssetTypeId, drawing::DRAWING_TYPE, font::FONT_TYPE,
    font::FontAsset,
};
use crate::systems::SystemRuntimeAccess;
use crate::systems::surface::{
    TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use crate::world::WorldSimulationState;
use crate::{ComponentValue, EntityId, OutputRef, WorldRef};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) fn incarnation(
    world: &WorldSimulationState,
    entity: EntityId,
    component: u16,
) -> Option<u64> {
    world
        .state
        .entities
        .get(&entity)?
        .input(component)
        .map(|input| input.incarnation)
}

/// Published entries in painter order.
pub(super) type CanvasEntries = Arc<[Arc<CanvasPaintEntry>]>;

/// What one evaluation publishes against the previous publication; each
/// replacement is present only when it differs.
pub(super) struct CanvasOutputChanges<'a> {
    /// Whether a leaf, slot or the layout moved or changed shape.
    pub layout: bool,
    /// Replacement entries, also when only the extent, density or layers
    /// changed, with the entries they replace in place.
    pub entries: Option<(CanvasEntries, Option<CanvasPaintChanges>)>,
    /// Replacement hits, when the hits or the GUI observations changed.
    pub hits: Option<Arc<[CanvasHit]>>,
    pub layers: &'a [u32],
    pub interaction: CanvasInteractionPriority,
    pub paints: Option<Arc<[CanvasPaintInstance]>>,
    pub resources: Option<Arc<[AssetKey]>>,
}

impl CanvasSystem {
    pub(super) fn evaluate(&mut self, context: &SystemRuntimeAccess<'_>) {
        self.state.work = CanvasWork::default();
        let world = &*context.world;
        let structure_changed = self.state.tree_dirty || !self.state.initialized;
        if structure_changed {
            self.rebuild_order(world);
        }
        let controls_changed = {
            let revision = self
                .gui
                .and_then(|binding| context.dependency(binding))
                .map_or(
                    0,
                    crate::systems::gui::GuiSystem::local_presentation_revision,
                );
            let changed = revision != self.state.gui_revision || self.state.gui_dirty;
            self.state.gui_revision = revision;
            changed
        };
        let selection = OutputRef::canvas(WorldRef {
            id: world.id,
            incarnation: world.identity,
        });
        let canvas = self.state.canvas;
        match logical_extent(canvas, selection, context.frame_context()) {
            None => self.state.publication = None,
            Some(extent) => {
                self.state.evaluated = Some(super::CanvasEvaluatedExtent {
                    extent,
                    tick: world.tick.saturating_add(1),
                });
                let gui_changed = {
                    let revision = self
                        .layout
                        .and_then(|binding| context.dependency(binding))
                        .and_then(|layout| layout.entity_view())
                        .map_or(0, |view| view.revision);
                    std::mem::replace(&mut self.state.layout_revision, revision) != revision
                };
                let changed_extent = self.state.publication.as_ref().is_none_or(|output| {
                    output.logical_extent != extent
                        || output.units_per_metre != canvas.units_per_metre
                });
                if structure_changed
                    || self.state.dirty
                    || changed_extent
                    || gui_changed
                    || controls_changed
                {
                    self.evaluate_canvas(
                        context,
                        selection,
                        extent,
                        canvas.units_per_metre,
                        structure_changed || changed_extent || gui_changed,
                    );
                } else if !self.state.patch.is_empty() {
                    self.patch_canvas(context, selection, extent, canvas.units_per_metre);
                }
            }
        }
        self.state.dirty = false;
        self.state.patch.clear();
        self.state.geometry_dirty.clear();
        self.state.tree_dirty = false;
        self.state.initialized = true;
        self.state.gui.complete(self.state.publication.as_ref());
        self.state.gui_dirty = false;
    }

    /// Tree order: depth-first pre-order over the top-level entities in core
    /// sibling order. Painter order is this order within each layer.
    fn rebuild_order(&mut self, world: &WorldSimulationState) {
        let mut order = Vec::new();
        let mut pending: Vec<_> = world.state.links.children(None).collect();
        pending.reverse();
        while let Some(entity) = pending.pop() {
            if world.state.links.invalid.contains(&entity) {
                continue;
            }
            order.push(entity);
            let children: Vec<_> = world.state.links.children(Some(entity)).collect();
            pending.extend(children.into_iter().rev());
        }
        let live: BTreeSet<_> = order.iter().copied().collect();
        self.state
            .leaves
            .retain(|(entity, _), _| live.contains(entity));
        self.state.slots.retain(|entity, _| live.contains(entity));
        self.state.order = order;
    }

    /// Inputs of one walk of the canvas at `extent` and `density`.
    pub(super) fn walk_inputs<'a, 'w>(
        &self,
        context: &'a SystemRuntimeAccess<'w>,
        extent: [f32; 2],
        density: f32,
    ) -> CanvasWalkInputs<'a, 'w> {
        CanvasWalkInputs {
            context,
            layout: self
                .layout
                .and_then(|binding| context.dependency(binding))
                .and_then(|layout| layout.entity_view())
                .cloned(),
            gui: self.gui.and_then(|binding| context.dependency(binding)),
            extent,
            density,
        }
    }

    /// Walk the whole canvas and publish what changed.
    pub(super) fn evaluate_canvas(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
        mut layout_changed: bool,
    ) {
        let world = &*context.world;
        let assets = context.asset_resources();
        let inputs = self.walk_inputs(context, extent, density);
        let order = std::mem::take(&mut self.state.order);
        // A closed or unraised overlay and its subtree have no paint, hits,
        // controls or layer; a canvas without overlays walks its whole order.
        let shown = inputs.layout.as_ref().and_then(|view| {
            hidden_overlays(
                world,
                view.overlays
                    .iter()
                    .filter(|overlay| !overlay.inputs.shown())
                    .map(|overlay| overlay.entity)
                    .collect(),
                &order,
            )
        });
        if let Some((_, hidden)) = &shown {
            for entity in hidden {
                layout_changed |= self.state.slots.remove(entity).is_some();
            }
        }
        let walked = shown.as_ref().map_or(&order[..], |(walk, _)| &walk[..]);
        let layers = super::layers::CanvasLayers::resolve(world, walked);
        let mut walk = CanvasWalk::new();
        for (position, &entity) in walked.iter().enumerate() {
            let layer = layers.as_ref().map_or(0, |layers| layers.layer(position));
            self.visit(&inputs, &mut walk, position, entity, layer);
        }
        self.unwind(&mut walk, None, walked.len());
        let positions: BTreeMap<_, _> = walked
            .iter()
            .enumerate()
            .map(|(position, &entity)| (entity, position))
            .collect();
        drop(shown);
        self.state.order = order;

        let CanvasWalk {
            entries,
            hits,
            controls,
            overlays,
            records,
            bounds,
            layout_changed: walked_layout,
            ..
        } = walk;
        layout_changed |= walked_layout;
        self.state.bounds.extend(bounds);
        let used = layers.as_ref().map_or(&[0][..], |layers| layers.used());
        let (entries, hits, overlays, entry_order, hit_order, overlay_order) = if used.len() > 1 {
            let entry_order = super::layers::layer_order(entries.iter().map(|entry| entry.layer()));
            let hit_order = super::layers::layer_order(hits.iter().map(|hit| hit.layer));
            let overlay_order =
                super::layers::layer_order(overlays.iter().map(|overlay| overlay.layer));
            (
                super::layers::in_layer_order(entries, &entry_order),
                super::layers::in_layer_order(hits, &hit_order),
                super::layers::in_layer_order(overlays, &overlay_order),
                entry_order,
                hit_order,
                overlay_order,
            )
        } else {
            (entries, hits, overlays, Vec::new(), Vec::new(), Vec::new())
        };
        let paints = paint_instances(world, assets, &entries);
        let resources = retained_resources(&entries, &paints);
        let interaction = records.iter().fold([0; 4], |counts, record| {
            let record = interaction_counts(record.interaction);
            std::array::from_fn(|flag| counts[flag] + record[flag])
        });
        self.state.work = CanvasWork {
            full: true,
            entities: records.len(),
            primitives: entries.len(),
            ..Default::default()
        };
        self.state.walk = CanvasWalkState {
            positions,
            records,
            entry_order,
            hit_order,
            overlay_order,
            interaction,
        };

        let previous = self.state.publication.take();
        let same_frame = previous.as_ref().is_some_and(|previous| {
            previous.logical_extent == extent
                && previous.units_per_metre == density
                && *previous.layers == *used
        });
        let replaced = previous
            .as_ref()
            .filter(|previous| same_frame && previous.entries.len() == entries.len())
            .map(|previous| {
                previous
                    .entries
                    .iter()
                    .zip(&entries)
                    .enumerate()
                    .filter(|(_, (before, after))| !Arc::ptr_eq(before, after))
                    .map(|(index, _)| index as u32)
                    .collect::<Vec<_>>()
            });
        let changed_paint = !same_frame
            || replaced
                .as_ref()
                .is_none_or(|replaced| !replaced.is_empty());
        let paint_changes =
            previous
                .as_ref()
                .zip(replaced)
                .map(|(previous, replaced)| CanvasPaintChanges {
                    base: previous.paint_revision,
                    entries: replaced.into(),
                });
        let changed_resources = previous
            .as_ref()
            .is_none_or(|previous| previous.resources != resources);
        let changed_paints = previous
            .as_ref()
            .is_none_or(|previous| *previous.paints != *paints);
        let changed_input = previous
            .as_ref()
            .is_none_or(|previous| previous.hits.as_ref() != hits)
            || self.state.gui.differs(&controls, &overlays);
        let input_revision = self.install(
            selection,
            extent,
            density,
            previous,
            CanvasOutputChanges {
                layout: layout_changed,
                entries: changed_paint.then(|| (entries.into(), paint_changes)),
                hits: changed_input.then(|| hits.into()),
                layers: used,
                interaction: self.state.walk.priority(),
                paints: changed_paints.then(|| paints.into()),
                resources: changed_resources.then_some(resources),
            },
        );
        let publication = self.state.publication.as_ref().unwrap();
        self.state.gui.finish(
            selection,
            input_revision,
            controls,
            overlays,
            &publication.entries,
        );
    }

    /// Publish `changes` against `previous`: every changed part takes one new
    /// revision. Returns the input revision.
    pub(super) fn install(
        &mut self,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
        previous: Option<CanvasPublication>,
        changes: CanvasOutputChanges<'_>,
    ) -> u64 {
        let changed_input = changes.hits.is_some();
        let revision = if changes.layout
            || changes.entries.is_some()
            || changes.resources.is_some()
            || changed_input
            || changes.paints.is_some()
        {
            self.state.next_revision()
        } else {
            previous.as_ref().unwrap().paint_revision
        };
        let previous = previous.as_ref();
        let kept = || previous.unwrap();
        let (paint_revision, entries, paint_changes) = match changes.entries {
            Some((entries, paint_changes)) => (revision, entries, paint_changes),
            None => (
                kept().paint_revision,
                kept().entries.clone(),
                kept().paint_changes.clone(),
            ),
        };
        let publication = CanvasPublication {
            selection,
            logical_extent: extent,
            units_per_metre: density,
            layout_revision: if changes.layout {
                revision
            } else {
                kept().layout_revision
            },
            paint_revision,
            resource_revision: if changes.resources.is_some() {
                revision
            } else {
                kept().resource_revision
            },
            input_revision: if changed_input {
                revision
            } else {
                kept().input_revision
            },
            entries,
            hits: changes.hits.unwrap_or_else(|| kept().hits.clone()),
            layers: match previous {
                Some(previous) if *previous.layers == *changes.layers => previous.layers.clone(),
                _ => changes.layers.into(),
            },
            interaction: changes.interaction,
            paints_revision: if changes.paints.is_some() {
                revision
            } else {
                kept().paints_revision
            },
            paints: changes.paints.unwrap_or_else(|| kept().paints.clone()),
            paint_changes,
            resources: changes
                .resources
                .unwrap_or_else(|| kept().resources.clone()),
        };
        let input_revision = publication.input_revision;
        self.state.publication = Some(publication);
        input_revision
    }
}

/// The canvas order without the subtrees of the `hidden` overlays, which the
/// layout did not show, and those subtrees' entities; none when no overlay is
/// hidden.
fn hidden_overlays(
    world: &WorldSimulationState,
    mut hidden: BTreeSet<EntityId>,
    order: &[EntityId],
) -> Option<(Vec<EntityId>, BTreeSet<EntityId>)> {
    if hidden.is_empty() {
        return None;
    }
    let mut walk = Vec::with_capacity(order.len());
    for &entity in order {
        let parent = world
            .state
            .links
            .effective(entity)
            .and_then(|link| link.parent);
        if hidden.contains(&entity) || parent.is_some_and(|parent| hidden.contains(&parent)) {
            hidden.insert(entity);
        } else {
            walk.push(entity);
        }
    }
    Some((walk, hidden))
}

/// A scrolling control's last layout geometry, committed offset and authored
/// bar geometry, read from its fields; default bar fields follow `font_size`.
pub(super) fn scroll_fields(
    world: &WorldSimulationState,
    entity: EntityId,
    font_size: f32,
) -> Option<(
    crate::systems::gui::layout::scroll_bars::GuiScrollExtent,
    [f32; 2],
    crate::systems::gui::layout::scroll_bars::GuiScrollBarStyle,
)> {
    use crate::systems::gui::layout::scroll_bars::{GuiScrollBarStyle, GuiScrollExtent};
    let index = entity.index() as usize;
    // Axis fields: horizontal 0, vertical 1, both 2.
    let axes = |axis: u32| [axis == 0 || axis == 2, axis == 1 || axis == 2];
    if let Some(list) = world.components.gui_virtual_list(index) {
        return Some((
            GuiScrollExtent {
                axes: axes(list.axis),
                viewport: [list.viewport_x, list.viewport_y],
                content: [list.content_x, list.content_y],
                capacity: [list.capacity_x, list.capacity_y],
            },
            [list.offset_x, list.offset_y],
            GuiScrollBarStyle::from_fields(
                [list.bar_thickness, list.bar_inset, list.bar_end_inset],
                font_size,
            ),
        ));
    }
    world.components.gui_scroll_view(index).map(|view| {
        (
            GuiScrollExtent {
                axes: axes(view.axis),
                viewport: [view.viewport_x, view.viewport_y],
                content: [view.content_x, view.content_y],
                capacity: [view.capacity_x, view.capacity_y],
            },
            [view.offset_x, view.offset_y],
            GuiScrollBarStyle::from_fields(
                [view.bar_thickness, view.bar_inset, view.bar_end_inset],
                font_size,
            ),
        )
    })
}

/// The canvas's logical extent: a Surface presenting it scales its physical
/// size by the density, a root viewport supplies CSS pixels and ignores the
/// density, and the stored extent applies while the canvas is not presented.
pub(in crate::world::systems) fn logical_extent(
    canvas: super::CanvasState,
    selection: OutputRef,
    frame: Option<&crate::WorldFrameContext>,
) -> Option<[f32; 2]> {
    let extent = match frame.filter(|frame| frame.selected_output == Some(selection)) {
        Some(frame) => {
            if let Some(physical) = frame.surface_extent {
                physical.map(|extent| (extent * f64::from(canvas.units_per_metre)) as f32)
            } else if let Some(viewport) = frame.viewport {
                [
                    (f64::from(viewport.width) / viewport.device_pixel_ratio) as f32,
                    (f64::from(viewport.height) / viewport.device_pixel_ratio) as f32,
                ]
            } else {
                canvas.extent
            }
        }
        None => canvas.extent,
    };
    extent
        .iter()
        .all(|value| value.is_finite() && *value > 0.0)
        .then_some(extent)
}

fn ready_resource(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    kind: AssetTypeId,
    source: &str,
    variant: u32,
) -> Option<AssetKey> {
    let key = assets.find_source(world.id, kind, source, variant)?;
    assets.get(key)?.data()?;
    Some(key)
}

pub(super) fn resource_key(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    entity: EntityId,
    component: u16,
) -> Option<AssetKey> {
    let index = entity.index() as usize;
    let (kind, source, variant) = match component {
        ComponentValue::CANVAS_TEXT => {
            let text = world.components.canvas_text(index)?;
            (FONT_TYPE, &*text.source, text.variant)
        }
        ComponentValue::CANVAS_GLYPH_RUN => {
            let run = world.components.canvas_glyph_run(index)?;
            (FONT_TYPE, &*run.source, run.variant)
        }
        ComponentValue::CANVAS_DRAWING => {
            let drawing = world.components.canvas_drawing(index)?;
            (DRAWING_TYPE, &*drawing.source, drawing.variant)
        }
        ComponentValue::CANVAS_BITMAP => {
            let bitmap = world.components.canvas_bitmap(index)?;
            (crate::TEXTURE_TYPE, &*bitmap.source, bitmap.variant)
        }
        _ => return None,
    };
    assets.find_source(world.id, kind, source, variant)
}

pub(in crate::world::systems) fn prepare_geometry(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    target: CanvasTarget,
) -> Option<CanvasGeometry> {
    prepare_constrained_geometry(world, assets, target, f32::INFINITY)
}

pub(in crate::world::systems) fn prepare_constrained_geometry(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    target: CanvasTarget,
    max_width: f32,
) -> Option<CanvasGeometry> {
    let index = target.entity.index() as usize;
    match target.component {
        ComponentValue::CANVAS_TEXT => {
            let text = world.components.canvas_text(index)?;
            let key = ready_resource(world, assets, FONT_TYPE, &text.source, text.variant)?;
            let font = assets.get_typed::<FontAsset>(key)?;
            let request = TextMeasureRequest::new(
                &text.text,
                TextFont::Ready {
                    key,
                    font,
                },
                text.font_size,
                TextLinePolicy::Multiline,
                if max_width.is_finite() {
                    TextMaxWidth::Ems((max_width / text.font_size).max(0.001))
                } else {
                    TextMaxWidth::Unbounded
                },
            )
            .ok()?;
            let TextOutcome::Measured(layout) = measure_text(&request) else {
                return None;
            };
            let glyphs: Vec<_> = layout
                .glyphs
                .iter()
                .map(|glyph| CanvasGlyph {
                    glyph_id: glyph.glyph_id,
                    position: glyph.position.map(|value| value * text.font_size),
                    color: None,
                })
                .collect();
            Some(CanvasGeometry::Glyphs {
                font: key,
                font_size: text.font_size,
                glyphs: glyphs.into(),
                size: layout.size.map(|value| value * text.font_size),
            })
        }
        ComponentValue::CANVAS_GLYPH_RUN => {
            let run = world.components.canvas_glyph_run(index)?;
            let font = ready_resource(world, assets, FONT_TYPE, &run.source, run.variant)?;
            let metrics = assets.get_typed::<FontAsset>(font)?;
            let unit_scale = run.font_size / metrics.units_per_em() as f32;
            let glyphs: Vec<_> = run
                .glyphs
                .iter()
                .map(|(_, glyph)| CanvasGlyph {
                    glyph_id: glyph.glyph_id,
                    position: glyph.position,
                    color: glyph.color,
                })
                .collect();
            Some(CanvasGeometry::Glyphs {
                font,
                font_size: run.font_size,
                size: glyphs.iter().fold([0.0_f32; 2], |size, glyph| {
                    metrics.glyph(glyph.glyph_id).map_or(size, |metric| {
                        [
                            size[0].max(
                                glyph.position[0]
                                    + metric.advance.max(metric.bounds[2]) * unit_scale,
                            ),
                            size[1].max(glyph.position[1] - metric.bounds[1] * unit_scale),
                        ]
                    })
                }),
                glyphs: glyphs.into(),
            })
        }
        ComponentValue::CANVAS_DRAWING => {
            let drawing = world.components.canvas_drawing(index)?;
            Some(CanvasGeometry::Drawing {
                drawing: ready_resource(
                    world,
                    assets,
                    DRAWING_TYPE,
                    &drawing.source,
                    drawing.variant,
                )?,
            })
        }
        ComponentValue::CANVAS_BITMAP => {
            let bitmap = world.components.canvas_bitmap(index)?;
            Some(CanvasGeometry::Bitmap {
                bitmap: ready_resource(
                    world,
                    assets,
                    crate::TEXTURE_TYPE,
                    &bitmap.source,
                    bitmap.variant,
                )?,
                size: [bitmap.width, bitmap.height],
            })
        }
        ComponentValue::CANVAS_BOX => {
            let shape = world.components.canvas_box(index)?;
            Some(CanvasGeometry::Box {
                size: [shape.width, shape.height],
                corner_radius: [shape.radius_x, shape.radius_y],
            })
        }
        _ => None,
    }
}

/// The CanvasPaint component lifetime of `entity`, if it has one.
pub(super) fn paint_target(world: &WorldSimulationState, entity: EntityId) -> Option<CanvasTarget> {
    world.components.canvas_paint(entity.index() as usize)?;
    Some(CanvasTarget {
        entity,
        component: ComponentValue::CANVAS_PAINT,
        incarnation: incarnation(world, entity, ComponentValue::CANVAS_PAINT)?,
    })
}

/// The custom paint an entry's box fills with, if any.
pub(super) fn entry_paint(entry: &CanvasPaintEntry) -> Option<CanvasTarget> {
    match entry {
        CanvasPaintEntry::Primitive {
            primitive:
                CanvasPrimitive::Box {
                    fill:
                        CanvasShapeFill::Paint {
                            paint,
                            ..
                        },
                    ..
                },
            ..
        } => Some(*paint),
        _ => None,
    }
}

/// The paint instances the painted boxes of `entries` name, in target order, with
/// their shaders' ready keys and current numeric property values.
pub(super) fn paint_instances(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    entries: &[Arc<CanvasPaintEntry>],
) -> Vec<CanvasPaintInstance> {
    let targets: BTreeSet<CanvasTarget> = entries
        .iter()
        .filter_map(|entry| entry_paint(entry))
        .collect();
    targets
        .into_iter()
        .filter_map(|target| paint_instance(world, assets, target))
        .collect()
}

/// The paint instance of `target` with its shader's ready key and current
/// numeric property values; none once its entity has no CanvasPaint.
pub(super) fn paint_instance(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    target: CanvasTarget,
) -> Option<CanvasPaintInstance> {
    let component = world
        .components
        .canvas_paint(target.entity.index() as usize)?;
    let properties: Vec<_> = component
        .properties
        .descriptors()
        .keys()
        .filter_map(|name| {
            let value = component.properties.get(name)?;
            (value.kind() != crate::DynamicPropertyKind::Asset).then(|| (name.clone(), value))
        })
        .collect();
    Some(CanvasPaintInstance {
        target,
        source: component.source.clone(),
        shader: ready_resource(
            world,
            assets,
            crate::services::asset_management::shader::SHADER_TYPE,
            &component.source,
            component.variant,
        ),
        properties: properties.into(),
    })
}

/// The resources `entries` and the shaders of `paints` retain, ascending.
pub(super) fn retained_resources(
    entries: &[Arc<CanvasPaintEntry>],
    paints: &[CanvasPaintInstance],
) -> Arc<[AssetKey]> {
    let resources: BTreeSet<_> = entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } => primitive.resource(),
            CanvasPaintEntry::Attachment(_) => None,
        })
        .chain(paints.iter().filter_map(|paint| paint.shader))
        .collect();
    resources.into_iter().collect::<Vec<_>>().into()
}

pub(super) fn attachment_slot(
    context: &SystemRuntimeAccess<'_>,
    entity: EntityId,
    placed: Placement,
    density: f32,
    layout_size: Option<[f32; 2]>,
) -> Option<CanvasAttachmentSlot> {
    if !placed.available {
        return None;
    }
    let world = &*context.world;
    let index = entity.index() as usize;
    let attachment = world.components.world_attachment(index)?;
    if attachment.mode == 0 {
        return None;
    }
    let surface = world.components.surface(index)?;
    let units = f64::from(density);
    let physical_extent = [f64::from(surface.width), f64::from(surface.height)];
    let scale = [
        f64::from(placed.scale[0])
            * layout_size.map_or(units, |size| f64::from(size[0]) / physical_extent[0]),
        -f64::from(placed.scale[1])
            * layout_size.map_or(units, |size| f64::from(size[1]) / physical_extent[1]),
    ];
    Some(CanvasAttachmentSlot {
        anchor: entity,
        surface_incarnation: incarnation(world, entity, ComponentValue::SURFACE)?,
        token: context
            .topology
            .tokens
            .get(&crate::host::topology::AttachmentAnchor {
                world: world.id,
                entity,
            })?
            .clone(),
        physical_extent,
        position: [
            f64::from(placed.position[0]) + physical_extent[0] * scale[0] / 2.0,
            f64::from(placed.position[1]) - physical_extent[1] * scale[1] / 2.0,
        ],
        scale,
        clip: placed.clip,
        opacity: placed.opacity,
        layer: placed.layer,
    })
}
