use super::system_state::{CanvasGeometry, CanvasPreparedLeaf};
use super::*;
use crate::components::GuiOverlay;
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

const LEAF_COMPONENTS: [u16; 5] = [
    ComponentValue::CANVAS_TEXT,
    ComponentValue::CANVAS_GLYPH_RUN,
    ComponentValue::CANVAS_DRAWING,
    ComponentValue::CANVAS_BITMAP,
    ComponentValue::CANVAS_BOX,
];

#[derive(Clone, Copy)]
struct Placement {
    position: [f32; 2],
    scale: [f32; 2],
    color: [f32; 4],
    opacity: f32,
    clip: CanvasClip,
    available: bool,
    enabled: bool,
    /// Plane id of the placed entity's layer; children replace it with theirs.
    layer: u32,
}

struct GuiPaintScope {
    entity: EntityId,
    placement: Placement,
    bars: Vec<crate::systems::gui::layout::scroll_bars::GuiScrollBar>,
    paint: Vec<CanvasPrimitive>,
    observation: Option<Arc<crate::systems::gui::presentation::GuiControlObservation>>,
}

impl Placement {
    fn root(extent: [f32; 2]) -> Self {
        Self {
            position: [0.0; 2],
            scale: [1.0; 2],
            color: [1.0; 4],
            opacity: 1.0,
            clip: [0.0, 0.0, extent[0], extent[1]],
            available: true,
            enabled: true,
            layer: 0,
        }
    }

    /// Compose a child's style. A nonzero layer raises the child above its
    /// parent and starts a new clip scope at the `canvas` extent before the
    /// child's own clip applies.
    fn child(self, style: Option<&CanvasStyle>, canvas: CanvasClip) -> Self {
        let Some(style) = style else {
            return self;
        };
        let (position, scale) = style.compose(self.position, self.scale);
        let mut placed = Self {
            position,
            scale,
            color: [
                self.color[0] * style.red,
                self.color[1] * style.green,
                self.color[2] * style.blue,
                self.color[3] * style.alpha,
            ],
            opacity: self.opacity * style.opacity,
            clip: if style.layer == 0 {
                self.clip
            } else {
                canvas
            },
            available: self.available,
            enabled: self.enabled,
            layer: self.layer,
        };
        if style.clipped {
            let bounds = placed.bounds([
                style.clip_min_x,
                style.clip_min_y,
                style.clip_max_x,
                style.clip_max_y,
            ]);
            placed.clip = intersect(placed.clip, bounds);
        }
        placed
    }

    fn bounds(self, local: CanvasClip) -> CanvasClip {
        if local[2] <= local[0] || local[3] <= local[1] {
            return [
                self.position[0],
                self.position[1],
                self.position[0],
                self.position[1],
            ];
        }
        let start = [
            self.position[0] + self.scale[0] * local[0],
            self.position[1] + self.scale[1] * local[1],
        ];
        let end = [
            self.position[0] + self.scale[0] * local[2],
            self.position[1] + self.scale[1] * local[3],
        ];
        [
            start[0].min(end[0]),
            start[1].min(end[1]),
            start[0].max(end[0]),
            start[1].max(end[1]),
        ]
    }

    fn style(self, target: CanvasTarget) -> CanvasPrimitiveStyle {
        CanvasPrimitiveStyle {
            identity: CanvasPrimitiveId {
                target,
                part: CanvasPart::Content,
            },
            position: self.position,
            scale: self.scale,
            color: self.color,
            opacity: self.opacity,
            clip: self.clip,
            layer: self.layer,
        }
    }
}

fn intersect(outer: CanvasClip, inner: CanvasClip) -> CanvasClip {
    [
        outer[0].max(inner[0]),
        outer[1].max(inner[1]),
        outer[2].min(inner[2]),
        outer[3].min(inner[3]),
    ]
}

fn incarnation(world: &WorldSimulationState, entity: EntityId, component: u16) -> Option<u64> {
    world
        .state
        .entities
        .get(&entity)?
        .input(component)
        .map(|input| input.incarnation)
}

impl CanvasSystem {
    pub(super) fn evaluate(&mut self, context: &SystemRuntimeAccess<'_>) {
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
                }
            }
        }
        self.state.dirty = false;
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

    fn evaluate_canvas(
        &mut self,
        context: &SystemRuntimeAccess<'_>,
        selection: OutputRef,
        extent: [f32; 2],
        density: f32,
        mut layout_changed: bool,
    ) {
        let world = &*context.world;
        let assets = context.asset_resources();
        let layout = self
            .layout
            .and_then(|binding| context.dependency(binding))
            .and_then(|layout| layout.entity_view())
            .cloned();
        let order = std::mem::take(&mut self.state.order);
        // A closed or unraised overlay and its subtree have no paint, hits,
        // controls or layer; a canvas without overlays walks its whole order.
        let shown = layout.as_ref().and_then(|view| {
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
        let walk = shown.as_ref().map_or(&order[..], |(walk, _)| &walk[..]);
        let layers = super::layers::CanvasLayers::resolve(world, walk);
        let canvas_clip = [0.0, 0.0, extent[0], extent[1]];
        let mut placements = BTreeMap::new();
        let mut entries = Vec::new();
        let mut hits = Vec::new();
        let mut gui_scopes: Vec<GuiPaintScope> = Vec::new();
        let gui = self.gui.and_then(|binding| context.dependency(binding));
        let mut controls = Vec::new();
        let mut overlays = Vec::new();
        // Entities from the top level to the current one, each with the
        // nearest focus scope at or above it, which ends at the World
        // boundary, and whether it lies in an open hint, which is inert to
        // input.
        let mut path: Vec<(EntityId, Option<EntityId>, bool)> = Vec::new();
        let mut interaction = CanvasInteractionPriority::default();
        for (position, &entity) in walk.iter().enumerate() {
            let parent = world
                .state
                .links
                .effective(entity)
                .and_then(|link| link.parent);
            let inert = {
                while path
                    .last()
                    .is_some_and(|(ancestor, ..)| Some(*ancestor) != parent)
                {
                    path.pop();
                }
                let inherited = path.last().and_then(|(_, scope, _)| *scope);
                let scope = world
                    .components
                    .gui_behavior(entity.index() as usize)
                    .is_some_and(|behavior| behavior.focus_scope);
                let inert = path.last().is_some_and(|(.., inert)| *inert)
                    || world
                        .components
                        .gui_overlay(entity.index() as usize)
                        .is_some_and(|overlay| overlay.mode == GuiOverlay::MODE_HINT);
                path.push((
                    entity,
                    if scope {
                        Some(entity)
                    } else {
                        inherited
                    },
                    inert,
                ));
                inert
            };
            while gui_scopes
                .last()
                .is_some_and(|scope| Some(scope.entity) != parent)
            {
                let GuiPaintScope {
                    paint,
                    observation,
                    ..
                } = gui_scopes.pop().unwrap();
                for primitive in paint {
                    if let Some(hit) = observation.as_ref().and_then(|control| {
                        control.scroll_hit(primitive.style().identity.part, entries.len() as u32)
                    }) {
                        hits.push(hit);
                    }
                    entries.push(self.state.gui.primitive(primitive));
                }
            }
            let mut inherited = parent
                .and_then(|parent| placements.get(&parent).copied())
                .unwrap_or_else(|| Placement::root(extent));
            let mapping = layout
                .as_ref()
                .and_then(|view| view.placements.get(&entity));
            if let Some(mapping) = mapping {
                inherited.position[0] += mapping.origin[0] * inherited.scale[0];
                inherited.position[1] += mapping.origin[1] * inherited.scale[1];
                inherited.available &= mapping.available;
            }
            let mut placed = inherited.child(
                world.components.canvas_style(entity.index() as usize),
                canvas_clip,
            );
            placed.layer = layers.as_ref().map_or(0, |layers| layers.layer(position));
            if gui.is_some()
                && let Some(behavior) = world.components.gui_behavior(entity.index() as usize)
            {
                placed.enabled &= behavior.enabled;
                if !behavior.visible {
                    placed.opacity = 0.0;
                }
            }
            if let Some(mapping) = mapping.filter(|mapping| mapping.clip) {
                placed.clip = intersect(
                    placed.clip,
                    placed.bounds([0.0, 0.0, mapping.size[0], mapping.size[1]]),
                );
            }
            let layout_size = mapping.map(|mapping| mapping.size);
            if gui.is_some()
                && let Some(size) = layout_size
                && let Some(overlay) = world.components.gui_overlay(entity.index() as usize)
                && let Some(lifetime) = incarnation(world, entity, ComponentValue::GUI_OVERLAY)
                && placed.available
                && placed.opacity > 0.0
                && crate::systems::gui::local::control::eligibility(world, entity).visible
            {
                let bounds = placed.bounds([0.0, 0.0, size[0], size[1]]);
                overlays.push(crate::systems::gui::presentation::GuiOverlayObservation {
                    target: crate::systems::gui::local::GuiEntityTarget {
                        world: WorldRef {
                            id: world.id,
                            incarnation: world.identity,
                        },
                        entity,
                        component: ComponentValue::GUI_OVERLAY,
                        incarnation: lifetime,
                    },
                    parent,
                    mode: overlay.mode,
                    layer: placed.layer,
                    order: entries.len() as u32,
                    bounds,
                });

                // A light overlay's box, and the whole canvas under a modal
                // one, take the pointer from what lies beneath, below the
                // overlay's own content.
                let blocker = match overlay.mode {
                    GuiOverlay::MODE_LIGHT => Some((bounds, placed.clip)),
                    GuiOverlay::MODE_MODAL => Some((canvas_clip, canvas_clip)),
                    _ => None,
                };
                if let Some((bounds, clip)) = blocker {
                    hits.push(CanvasHit {
                        target: CanvasTarget {
                            entity,
                            component: ComponentValue::GUI_OVERLAY,
                            incarnation: lifetime,
                        },
                        kind: CanvasHitKind::Overlay,
                        paint_order: entries.len() as u32,
                        layer: placed.layer,
                        bounds,
                        clip,
                        position: placed.position,
                        scale: placed.scale,
                        eligible: true,
                        ancestry: path.iter().map(|(entity, ..)| *entity).collect(),
                    });
                }
            }
            placements.insert(entity, placed);
            let mut content_placement = placed;
            let mut deferred_paint = Vec::new();
            let mut scroll_bars = Vec::new();
            let mut observation = None;
            if let Some(mapping) = mapping {
                content_placement.position[0] += mapping.content_offset[0] * placed.scale[0];
                content_placement.position[1] += mapping.content_offset[1] * placed.scale[1];
            }
            if let Some(mapping) = mapping
                && world
                    .components
                    .canvas_bounds(entity.index() as usize)
                    .is_some()
            {
                let bounds = placed.bounds([0.0, 0.0, mapping.size[0], mapping.size[1]]);
                self.state.bounds.push((
                    entity,
                    [
                        bounds[0],
                        bounds[1],
                        bounds[2] - bounds[0],
                        bounds[3] - bounds[1],
                    ],
                ));
            }
            let control = gui.and_then(|_| {
                crate::systems::gui::local::control::entity_control(world, &world.state, entity)
            });
            let paint = paint_target(world, entity);
            if let Some(gui) = gui
                && let Some(mapping) = mapping
                && let Some(control) = control
            {
                let target = control.target.canvas_target();
                let eligibility = crate::systems::gui::local::control::eligibility(world, entity);
                let font_size = layout
                    .as_ref()
                    .and_then(|view| view.control_labels.get(&entity))
                    .map_or(
                        crate::systems::gui::presentation::GUI_DEFAULT_FONT_SIZE,
                        |label| label.font_size,
                    );
                let scroll = scroll_fields(world, entity, font_size);
                if let Some((extent, offset, bar_style)) = &scroll {
                    let offset_origin = mapping.content_offset;
                    let mut children = placed;
                    children.clip = intersect(
                        placed.clip,
                        placed.bounds([
                            offset_origin[0],
                            offset_origin[1],
                            offset_origin[0] + extent.viewport[0],
                            offset_origin[1] + extent.viewport[1],
                        ]),
                    );
                    placements.insert(entity, children);
                    let mut obstacles = Vec::new();
                    if placed.scale.iter().all(|scale| *scale != 0.0) {
                        for scope in &gui_scopes {
                            let ancestor = scope.placement;
                            for bar in &scope.bars {
                                let mut mapped = *bar;
                                for rect in [&mut mapped.track, &mut mapped.thumb] {
                                    for axis in 0..2 {
                                        let start = (ancestor.position[axis]
                                            + rect[axis] * ancestor.scale[axis]
                                            - placed.position[axis])
                                            / placed.scale[axis];
                                        let end = start
                                            + rect[axis + 2] * ancestor.scale[axis]
                                                / placed.scale[axis];
                                        rect[axis] = start.min(end);
                                        rect[axis + 2] = (end - start).abs();
                                    }
                                }
                                obstacles.push(mapped);
                            }
                        }
                    }
                    scroll_bars = crate::systems::gui::layout::scroll_bars::ordinary_scroll_bars(
                        extent,
                        mapping.size,
                        *bar_style,
                        *offset,
                        &obstacles,
                    );
                }

                // Pointer feedback shows only where the placement can take input.
                let interactive = eligibility.eligible()
                    && placed.available
                    && placed.enabled
                    && placed.opacity > 0.0
                    && placed.scale.iter().all(|scale| *scale != 0.0);
                let painted = crate::systems::gui::presentation::GuiPaintedControl {
                    control,
                    eligibility,
                    interaction: if interactive {
                        gui.interaction_flags(control.target)
                    } else {
                        Default::default()
                    },
                };
                let control_primitives = crate::systems::gui::presentation::control_paint(
                    context,
                    &painted,
                    gui.focus_ring(control.target),
                    gui.native_text_state(control.target),
                    placed.style(target),
                    mapping,
                    layout
                        .as_ref()
                        .and_then(|view| view.control_labels.get(&entity)),
                    &scroll_bars,
                    gui.part_interaction(control.target, painted.interaction),
                    |identity| self.state.gui.retained_resource(identity),
                );
                let available =
                    placed.available && eligibility.available && control_primitives.is_some();
                let bounds = placed.bounds([0.0, 0.0, mapping.size[0], mapping.size[1]]);
                let ancestry: Arc<[EntityId]> = path
                    .iter()
                    .map(|(entity, ..)| *entity)
                    .collect::<Vec<_>>()
                    .into();
                let hit = CanvasHit {
                    target,
                    kind: CanvasHitKind::Entity,
                    paint_order: entries.len() as u32,
                    layer: placed.layer,
                    bounds,
                    clip: placed.clip,
                    position: placed.position,
                    scale: placed.scale,
                    eligible: available
                        && !inert
                        && eligibility.enabled
                        && eligibility.visible
                        && placed.enabled
                        && placed.opacity > 0.0
                        && placed.scale.iter().all(|scale| *scale != 0.0),
                    ancestry: ancestry.clone(),
                };
                interaction.focused |= gui.focused(control.target) && hit.eligible;
                interaction.hovered |= painted.interaction.hovered && hit.eligible;
                interaction.pressed |= painted.interaction.pressed && hit.eligible;
                interaction.captured |= painted.interaction.captured && hit.eligible;
                let slider = world
                    .components
                    .gui_slider(entity.index() as usize)
                    .filter(|_| control.target.component == ComponentValue::GUI_SLIDER);
                let color = world
                    .components
                    .gui_color(entity.index() as usize)
                    .filter(|_| control.target.component == ComponentValue::GUI_COLOR);
                let value = match (&scroll, slider, color) {
                    (Some((extent, offset, _)), ..) => {
                        crate::systems::gui::presentation::GuiRoutingValue::Scroll {
                            offset: *offset,
                            capacity: extent.capacity,
                        }
                    }
                    (None, Some(slider), _) => {
                        crate::systems::gui::presentation::GuiRoutingValue::Scalar(slider.value)
                    }
                    (None, None, Some(color)) => {
                        crate::systems::gui::presentation::GuiRoutingValue::Color(color.channels())
                    }
                    (None, None, None) => crate::systems::gui::presentation::GuiRoutingValue::None,
                };
                let control = self.state.gui.observation(
                    crate::systems::gui::presentation::GuiControlObservation {
                        text: layout
                            .as_ref()
                            .and_then(|view| view.control_labels.get(&entity))
                            .and_then(|label| Some((label.layout.clone()?, label.origin()))),
                        scroll_bars: std::array::from_fn(|axis| {
                            scroll_bars
                                .iter()
                                .find(|bar| bar.axis == axis && bar.enabled())
                                .copied()
                        }),
                        focus_scope: path.len().checked_sub(2).and_then(|index| path[index].1),
                        group: gui.group_item(world, control, &ancestry),
                        record: crate::systems::gui::presentation::GuiControlRecord {
                            target: control.target,
                            kind: control.kind,
                            value,
                            enabled: eligibility.enabled,
                            visible: eligibility.visible,
                            available: eligibility.available,
                            focusable: crate::systems::gui::local::control::focusable(
                                world, entity,
                            ),
                            focus_parts: crate::systems::gui::local::control::focus_parts(
                                world, control,
                            ),
                            ancestry,
                        },
                        hit: hit.clone(),
                        available,
                        slider: slider.and_then(|slider| {
                            crate::systems::gui::presentation::GuiSliderGeometry::new(
                                slider,
                                slider.value,
                                mapping.size,
                            )
                        }),
                        number: world
                            .components
                            .gui_text_input(entity.index() as usize)
                            .filter(|input| {
                                input.numeric
                                    && control.target.component == ComponentValue::GUI_TEXT_INPUT
                            })
                            .map(
                                |input| crate::systems::gui::presentation::GuiNumberGeometry {
                                    steps: input.step_parts.then(|| {
                                        crate::systems::gui::local::number::number_step_rects(
                                            mapping.size,
                                        )
                                    }),
                                },
                            ),
                        color: color.map(|color| {
                            crate::systems::gui::local::color::GuiColorLayout::new(
                                mapping.size,
                                font_size,
                                color.alpha_rail,
                            )
                        }),
                    },
                );
                observation = Some(control.clone());
                controls.push(control);
                hits.push(hit);
                if available && let Some(primitives) = control_primitives {
                    for primitive in primitives {
                        let primitive = match paint {
                            Some(paint)
                                if primitive.style().identity.part == CanvasPart::Background =>
                            {
                                with_paint(primitive, paint)
                            }
                            _ => primitive,
                        };
                        if matches!(
                            primitive.style().identity.part,
                            CanvasPart::ScrollTrackX
                                | CanvasPart::ScrollTrackY
                                | CanvasPart::ScrollThumbX
                                | CanvasPart::ScrollThumbY
                        ) {
                            deferred_paint.push(primitive);
                        } else {
                            entries.push(self.state.gui.primitive(primitive));
                        }
                    }
                }
            }

            // A skinned entity that is not a control paints its Background part
            // before its content and children, over its layout bounds or, where
            // GUI layout does not place it, its CanvasBox. That part is the box's
            // only paint. It has no hit target, observation or focus.
            let skinned = gui
                .filter(|_| control.is_none())
                .and_then(|_| incarnation(world, entity, ComponentValue::GUI_SKIN));
            if let Some(lifetime) = skinned
                && placed.available
                && let Some(size) = layout_size.or_else(|| {
                    world
                        .components
                        .canvas_box(entity.index() as usize)
                        .map(|shape| [shape.width, shape.height])
                })
            {
                let target = CanvasTarget {
                    entity,
                    component: ComponentValue::GUI_SKIN,
                    incarnation: lifetime,
                };
                let background = crate::systems::gui::presentation::skinned_background(
                    context,
                    entity,
                    placed.style(target),
                    size,
                    |identity| self.state.gui.retained_resource(identity),
                );
                if let Some(background) = background {
                    let background = match paint {
                        Some(paint) => with_paint(background, paint),
                        None => background,
                    };
                    entries.push(self.state.gui.primitive(background));
                }
            }

            for component in LEAF_COMPONENTS {
                let lifetime = incarnation(world, entity, component)
                    .filter(|_| skinned.is_none() || component != ComponentValue::CANVAS_BOX);
                let Some(lifetime) = lifetime else {
                    self.state.leaves.remove(&(entity, component));
                    continue;
                };
                let target = CanvasTarget {
                    entity,
                    component,
                    incarnation: lifetime,
                };
                let style = content_placement.style(target);
                let leaf_paint = paint.filter(|_| component == ComponentValue::CANVAS_BOX);
                let previous = self.state.leaves.remove(&(entity, component));
                let rebuild_geometry = self.state.geometry_dirty.contains(&(entity, component))
                    || previous
                        .as_ref()
                        .is_some_and(|leaf| leaf.layout_size != layout_size)
                    || previous
                        .as_ref()
                        .is_none_or(|leaf| leaf.incarnation != lifetime);
                let override_geometry = layout
                    .as_ref()
                    .and_then(|view| view.geometry.get(&(entity, component)));
                let mut geometry = if let Some(geometry) = override_geometry {
                    geometry.clone()
                } else if rebuild_geometry {
                    prepare_geometry(world, assets, target)
                } else {
                    previous.as_ref().and_then(|leaf| leaf.geometry.clone())
                };
                if let Some(layout_size) = layout_size
                    && let Some(
                        CanvasGeometry::Box {
                            size,
                            ..
                        }
                        | CanvasGeometry::Bitmap {
                            size,
                            ..
                        },
                    ) = &mut geometry
                {
                    *size = layout_size;
                }
                let changed_geometry = (rebuild_geometry || override_geometry.is_some())
                    && previous
                        .as_ref()
                        .is_none_or(|leaf| leaf.geometry != geometry);
                let changed_style = previous
                    .as_ref()
                    .is_none_or(|leaf| leaf.style != style || leaf.paint != leaf_paint);
                layout_changed |= changed_geometry
                    || previous.as_ref().is_none_or(|leaf| {
                        leaf.style.position != style.position
                            || leaf.style.scale != style.scale
                            || leaf.style.clip != style.clip
                    });
                let mut leaf = if !changed_geometry && !changed_style {
                    previous.unwrap()
                } else {
                    let revision = self.state.next_revision();
                    let geometry_revision = if changed_geometry {
                        revision
                    } else {
                        previous.as_ref().unwrap().geometry_revision
                    };
                    let material_revision = revision;
                    let entry = geometry.as_ref().map(|geometry| {
                        Arc::new(CanvasPaintEntry::Primitive {
                            geometry_revision,
                            material_revision,
                            primitive: geometry.primitive(style, leaf_paint),
                        })
                    });
                    CanvasPreparedLeaf {
                        layout_size,
                        incarnation: lifetime,
                        geometry,
                        style,
                        paint: leaf_paint,
                        geometry_revision,
                        entry,
                    }
                };
                leaf.layout_size = layout_size;
                if let Some(entry) = &leaf.entry
                    && placed.available
                {
                    entries.push(entry.clone());
                }
                self.state.leaves.insert((entity, component), leaf);
            }
            if let Some(slot) =
                attachment_slot(context, entity, content_placement, density, layout_size)
            {
                layout_changed |= !self.state.slots.contains_key(&entity);
                let entry = self
                    .state
                    .slots
                    .entry(entity)
                    .or_insert_with(|| Arc::new(CanvasPaintEntry::Attachment(slot.clone())));
                if entry.as_ref() != &CanvasPaintEntry::Attachment(slot.clone()) {
                    if let CanvasPaintEntry::Attachment(previous) = entry.as_ref() {
                        layout_changed |= previous.physical_extent != slot.physical_extent
                            || previous.position != slot.position
                            || previous.scale != slot.scale
                            || previous.clip != slot.clip;
                    }
                    *entry = Arc::new(CanvasPaintEntry::Attachment(slot.clone()));
                }
                let ancestry: Vec<_> = path.iter().map(|(entity, ..)| *entity).collect();
                hits.push(CanvasHit {
                    target: CanvasTarget {
                        entity,
                        component: ComponentValue::SURFACE,
                        incarnation: slot.surface_incarnation,
                    },
                    kind: CanvasHitKind::Attachment {
                        anchor: entity,
                        token: slot.token.clone(),
                    },
                    paint_order: entries.len() as u32,
                    layer: slot.layer,
                    bounds: content_placement.bounds([
                        0.0,
                        0.0,
                        layout_size
                            .map_or(slot.physical_extent[0] as f32 * density, |size| size[0]),
                        layout_size
                            .map_or(slot.physical_extent[1] as f32 * density, |size| size[1]),
                    ]),
                    clip: placed.clip,
                    position: content_placement.position,
                    scale: [
                        slot.scale[0] as f32 / density,
                        -slot.scale[1] as f32 / density,
                    ],
                    eligible: placed.enabled
                        && !inert
                        && placed.opacity > 0.0
                        && placed.scale.iter().all(|scale| *scale != 0.0),
                    ancestry: ancestry.into(),
                });
                entries.push(entry.clone());
            } else {
                layout_changed |= self.state.slots.remove(&entity).is_some();
            }
            gui_scopes.push(GuiPaintScope {
                entity,
                placement: placed,
                bars: scroll_bars,
                paint: deferred_paint,
                observation,
            });
        }
        while let Some(GuiPaintScope {
            paint: primitives,
            observation,
            ..
        }) = gui_scopes.pop()
        {
            for primitive in primitives {
                if let Some(hit) = observation.as_ref().and_then(|control| {
                    control.scroll_hit(primitive.style().identity.part, entries.len() as u32)
                }) {
                    hits.push(hit);
                }
                entries.push(self.state.gui.primitive(primitive));
            }
        }
        self.state.order = order;
        let used = layers.as_ref().map_or(&[0][..], |layers| layers.used());
        if used.len() > 1 {
            super::layers::order_by_layer(&mut entries, &mut hits);
            overlays.sort_by_key(|overlay| overlay.layer);
        }
        let paints = paint_instances(world, assets, &entries);
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
        let resources: Arc<[_]> = resources.into_iter().collect::<Vec<_>>().into();
        let previous = self.state.publication.take();
        let changed_paint = previous.as_ref().is_none_or(|previous| {
            previous.entries.len() != entries.len()
                || previous
                    .entries
                    .iter()
                    .zip(&entries)
                    .any(|(before, after)| !Arc::ptr_eq(before, after))
                || previous.logical_extent != extent
                || previous.units_per_metre != density
                || *previous.layers != *used
        });
        let changed_resources = previous
            .as_ref()
            .is_none_or(|previous| previous.resources != resources);
        let changed_paints = previous
            .as_ref()
            .is_none_or(|previous| *previous.paints != *paints);
        let mut changed_input = previous
            .as_ref()
            .is_none_or(|previous| previous.hits.as_ref() != hits);
        changed_input |= self.state.gui.differs(&controls, &overlays);
        let revision = if layout_changed
            || changed_paint
            || changed_resources
            || changed_input
            || changed_paints
        {
            self.state.next_revision()
        } else {
            previous.as_ref().unwrap().paint_revision
        };
        self.state.gui.finish(
            selection,
            if changed_input {
                revision
            } else {
                previous.as_ref().unwrap().input_revision
            },
            controls,
            overlays,
            &entries,
        );
        self.state.publication = Some(CanvasPublication {
            selection,
            logical_extent: extent,
            units_per_metre: density,
            layout_revision: if layout_changed {
                revision
            } else {
                previous.as_ref().unwrap().layout_revision
            },
            paint_revision: if changed_paint {
                revision
            } else {
                previous.as_ref().unwrap().paint_revision
            },
            resource_revision: if changed_resources {
                revision
            } else {
                previous.as_ref().unwrap().resource_revision
            },
            input_revision: if changed_input {
                revision
            } else {
                previous.as_ref().unwrap().input_revision
            },
            entries: if changed_paint {
                entries.into()
            } else {
                previous.as_ref().unwrap().entries.clone()
            },
            hits: if changed_input {
                hits.into()
            } else {
                previous.as_ref().unwrap().hits.clone()
            },
            layers: match &previous {
                Some(previous) if *previous.layers == *used => previous.layers.clone(),
                _ => used.into(),
            },
            interaction,
            paints_revision: if changed_paints {
                revision
            } else {
                previous.as_ref().unwrap().paints_revision
            },
            paints: if changed_paints {
                paints.into()
            } else {
                previous.as_ref().unwrap().paints.clone()
            },
            resources: if changed_resources {
                resources
            } else {
                previous.as_ref().unwrap().resources.clone()
            },
        });
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

/// Scroll geometry and offset from a scrolling control's layout fields.
/// A scrolling control's last layout geometry, committed offset and authored
/// bar geometry, read from its fields; default bar fields follow `font_size`.
fn scroll_fields(
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

impl CanvasGeometry {
    /// The primitive of this geometry under `style`; a box takes the custom
    /// `paint` of its entity's CanvasPaint, when it has one.
    fn primitive(
        &self,
        style: CanvasPrimitiveStyle,
        paint: Option<CanvasTarget>,
    ) -> CanvasPrimitive {
        match self {
            Self::Glyphs {
                font,
                font_size,
                glyphs,
                ..
            } => CanvasPrimitive::Glyphs {
                style,
                font: *font,
                font_size: *font_size,
                glyphs: glyphs.clone(),
            },
            Self::Drawing {
                drawing,
            } => CanvasPrimitive::Drawing {
                style,
                drawing: *drawing,
            },
            Self::Bitmap {
                bitmap,
                size,
            } => CanvasPrimitive::Bitmap {
                style,
                bitmap: *bitmap,
                size: *size,
            },
            Self::Box {
                size,
                corner_radius,
            } => {
                let primitive = CanvasPrimitive::Box {
                    style,
                    size: *size,
                    corner_radius: *corner_radius,
                    border_width: 0.0,
                    border_color: [0.0; 4],
                    fill: CanvasShapeFill::Solid([1.0; 4]),
                    glow: None,
                    shape: CanvasBoxShape::RECT,
                };
                match paint {
                    Some(paint) => with_paint(primitive, paint),
                    None => primitive,
                }
            }
        }
    }
}

/// The CanvasPaint component lifetime of `entity`, if it has one.
fn paint_target(world: &WorldSimulationState, entity: EntityId) -> Option<CanvasTarget> {
    world.components.canvas_paint(entity.index() as usize)?;
    Some(CanvasTarget {
        entity,
        component: ComponentValue::CANVAS_PAINT,
        incarnation: incarnation(world, entity, ComponentValue::CANVAS_PAINT)?,
    })
}

/// `primitive` filled by the custom `paint` when it is a box: the fill becomes the
/// paint called with the fill's colour, and a checker is left out, since the paint
/// replaces the fill and the checker beneath it. Other primitives are unchanged.
fn with_paint(primitive: CanvasPrimitive, paint: CanvasTarget) -> CanvasPrimitive {
    match primitive {
        CanvasPrimitive::Box {
            style,
            size,
            corner_radius,
            border_width,
            border_color,
            fill,
            glow,
            shape,
        } => CanvasPrimitive::Box {
            style,
            size,
            corner_radius,
            border_width,
            border_color,
            fill: CanvasShapeFill::Paint {
                color: fill.paint_color(),
                paint,
            },
            glow,
            shape: match shape {
                CanvasBoxShape::Rect {
                    corner_cut,
                    corner_accent,
                    corner_accent_width,
                    ..
                } => CanvasBoxShape::Rect {
                    corner_cut,
                    corner_accent,
                    corner_accent_width,
                    checker: None,
                },
                shape => shape,
            },
        },
        primitive => primitive,
    }
}

/// The paint instances the painted boxes of `entries` name, in target order, with
/// their shaders' ready keys and current numeric property values.
fn paint_instances(
    world: &WorldSimulationState,
    assets: &AssetManagementService,
    entries: &[Arc<CanvasPaintEntry>],
) -> Vec<CanvasPaintInstance> {
    let targets: BTreeSet<CanvasTarget> = entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
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
        })
        .collect();
    targets
        .into_iter()
        .filter_map(|target| {
            let component = world
                .components
                .canvas_paint(target.entity.index() as usize)?;
            let properties: Vec<_> = component
                .properties
                .descriptors()
                .keys()
                .filter_map(|name| {
                    let value = component.properties.get(name)?;
                    (value.kind() != crate::DynamicPropertyKind::Asset)
                        .then(|| (name.clone(), value))
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
        })
        .collect()
}

fn attachment_slot(
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
