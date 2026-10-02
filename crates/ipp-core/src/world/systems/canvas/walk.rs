//! The depth-first canvas walk: each entity's placement, paint, hits, control
//! observation and attachment slot, composed from what its ancestors pass
//! down.
//!
//! The whole-canvas evaluation walks every shown entity from the top level. A
//! patch re-enters the walk at a changed entity: it seeds the ancestor stack
//! from the retained [`CanvasWalkRecord`]s and walks only that entity's
//! subtree, whose output is one contiguous range of each tree-order output.

use super::system_state::{CanvasGeometry, CanvasPreparedLeaf, CanvasWalkMarks, CanvasWalkRecord};
use super::update::{attachment_slot, incarnation, paint_target, prepare_geometry, scroll_fields};
use super::*;
use crate::components::GuiOverlay;
use crate::systems::SystemRuntimeAccess;
use crate::systems::gui::layout::GuiEntityLayoutView;
use crate::systems::gui::layout::scroll_bars::GuiScrollBar;
use crate::systems::gui::presentation::{GuiControlObservation, GuiOverlayObservation};
use crate::{ComponentValue, EntityId, WorldRef};
use std::sync::Arc;

/// Leaf components in the order an entity paints them.
pub(super) const LEAF_COMPONENTS: [u16; 5] = [
    ComponentValue::CANVAS_TEXT,
    ComponentValue::CANVAS_GLYPH_RUN,
    ComponentValue::CANVAS_DRAWING,
    ComponentValue::CANVAS_BITMAP,
    ComponentValue::CANVAS_BOX,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Placement {
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub color: [f32; 4],
    pub opacity: f32,
    pub clip: CanvasClip,
    pub available: bool,
    pub enabled: bool,
    /// Plane id of the placed entity's layer; children replace it with theirs.
    pub layer: u32,
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

    pub(super) fn bounds(self, local: CanvasClip) -> CanvasClip {
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

pub(super) fn intersect(outer: CanvasClip, inner: CanvasClip) -> CanvasClip {
    [
        outer[0].max(inner[0]),
        outer[1].max(inner[1]),
        outer[2].min(inner[2]),
        outer[3].min(inner[3]),
    ]
}

/// Inputs every entity of one evaluation shares.
pub(super) struct CanvasWalkInputs<'a, 'w> {
    pub context: &'a SystemRuntimeAccess<'w>,
    pub layout: Option<Arc<GuiEntityLayoutView>>,
    pub gui: Option<&'a crate::systems::gui::GuiSystem>,
    pub extent: [f32; 2],
    pub density: f32,
}

/// An entity on the walk's ancestor stack: what its descendants inherit and
/// the scroll parts it paints once they are walked.
struct CanvasWalkFrame {
    entity: EntityId,
    position: usize,
    placed: Placement,
    inherited: Placement,
    bars: Vec<GuiScrollBar>,
    scope: Option<EntityId>,
    inert: bool,
    deferred: Vec<CanvasPrimitive>,
    observation: Option<Arc<GuiControlObservation>>,
    /// An ancestor of a patched subtree, seeded from its retained record; the
    /// patch leaves its output alone.
    seeded: bool,
}

/// One walk over a range of the canvas's tree order and its output there.
pub(super) struct CanvasWalk {
    stack: Vec<CanvasWalkFrame>,
    /// Tree-order output before the walked range.
    base: CanvasWalkMarks,
    /// Walk position where the walked range starts.
    first: usize,
    pub entries: Vec<Arc<CanvasPaintEntry>>,
    pub hits: Vec<CanvasHit>,
    pub controls: Vec<Arc<GuiControlObservation>>,
    pub overlays: Vec<GuiOverlayObservation>,
    /// Records of the walked positions, in order.
    pub records: Vec<CanvasWalkRecord>,
    /// Evaluated bounds of the walked entities that have `CanvasBounds`.
    pub bounds: Vec<(EntityId, [f32; 4])>,
    /// Whether a walked leaf, slot or geometry moved or changed shape.
    pub layout_changed: bool,
}

impl CanvasWalk {
    /// A walk of the whole canvas.
    pub fn new() -> Self {
        Self::at(0, CanvasWalkMarks::default())
    }

    /// A walk starting at walk position `first`, whose output follows `base`.
    pub fn at(first: usize, base: CanvasWalkMarks) -> Self {
        Self {
            stack: Vec::new(),
            base,
            first,
            entries: Vec::new(),
            hits: Vec::new(),
            controls: Vec::new(),
            overlays: Vec::new(),
            records: Vec::new(),
            bounds: Vec::new(),
            layout_changed: false,
        }
    }

    /// Seed the stack with an ancestor of the walked range from its record.
    pub fn seed(&mut self, position: usize, record: &CanvasWalkRecord) {
        self.stack.push(CanvasWalkFrame {
            entity: record.entity,
            position,
            placed: record.placed,
            inherited: record.inherited,
            bars: record.bars.clone(),
            scope: record.scope,
            inert: record.inert,
            deferred: Vec::new(),
            observation: None,
            seeded: true,
        });
    }

    /// Tree-order output so far.
    fn marks(&self) -> CanvasWalkMarks {
        CanvasWalkMarks {
            entries: self.entry_index(),
            hits: self.base.hits + self.hits.len() as u32,
            controls: self.base.controls + self.controls.len() as u32,
            overlays: self.base.overlays + self.overlays.len() as u32,
        }
    }

    /// Tree-order index of the next entry, which hits record as their paint order.
    fn entry_index(&self) -> u32 {
        self.base.entries + self.entries.len() as u32
    }
}

impl CanvasSystem {
    /// Close every walked frame whose subtree ends before walk position `next`,
    /// whose entity is a child of `parent`. Seeded frames stay.
    pub(super) fn unwind(&mut self, walk: &mut CanvasWalk, parent: Option<EntityId>, next: usize) {
        while walk
            .stack
            .last()
            .is_some_and(|frame| !frame.seeded && Some(frame.entity) != parent)
        {
            let frame = walk.stack.pop().unwrap();
            for primitive in frame.deferred {
                if let Some(hit) = frame.observation.as_ref().and_then(|control| {
                    control.scroll_hit(primitive.style().identity.part, walk.entry_index())
                }) {
                    walk.hits.push(hit);
                }
                walk.entries.push(self.state.gui.primitive(primitive));
            }
            let end = walk.marks();
            let record = &mut walk.records[frame.position - walk.first];
            record.end = end;
            record.subtree_end = next;
        }
    }

    /// Walk the entity at walk `position` on plane `layer`, after its
    /// preceding siblings' subtrees.
    pub(super) fn visit(
        &mut self,
        inputs: &CanvasWalkInputs<'_, '_>,
        walk: &mut CanvasWalk,
        position: usize,
        entity: EntityId,
        layer: u32,
    ) {
        let context = inputs.context;
        let world = &*context.world;
        let assets = context.asset_resources();
        let layout = inputs.layout.as_deref();
        let gui = inputs.gui;
        let extent = inputs.extent;
        let density = inputs.density;
        let canvas_clip = [0.0, 0.0, extent[0], extent[1]];
        let parent = world
            .state
            .links
            .effective(entity)
            .and_then(|link| link.parent);
        self.unwind(walk, parent, position);
        let start = walk.marks();
        let parent_frame = walk
            .stack
            .last()
            .filter(|frame| Some(frame.entity) == parent);

        // The nearest focus scope at or above the entity, which ends at the
        // World boundary, and whether it lies in an open hint, which is inert
        // to input.
        let parent_scope = parent_frame.and_then(|frame| frame.scope);
        let scope = if world
            .components
            .gui_behavior(entity.index() as usize)
            .is_some_and(|behavior| behavior.focus_scope)
        {
            Some(entity)
        } else {
            parent_scope
        };
        let inert = parent_frame.is_some_and(|frame| frame.inert)
            || world
                .components
                .gui_overlay(entity.index() as usize)
                .is_some_and(|overlay| overlay.mode == GuiOverlay::MODE_HINT);

        let mut inherited = parent_frame
            .map(|frame| frame.inherited)
            .unwrap_or_else(|| Placement::root(extent));
        let mapping = layout.and_then(|view| view.placements.get(&entity));
        if let Some(mapping) = mapping {
            inherited.position[0] += mapping.origin[0] * inherited.scale[0];
            inherited.position[1] += mapping.origin[1] * inherited.scale[1];
            inherited.available &= mapping.available;
        }
        let mut placed = inherited.child(
            world.components.canvas_style(entity.index() as usize),
            canvas_clip,
        );
        placed.layer = layer;
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
            walk.overlays.push(GuiOverlayObservation {
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
                order: walk.entry_index(),
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
                walk.hits.push(CanvasHit {
                    target: CanvasTarget {
                        entity,
                        component: ComponentValue::GUI_OVERLAY,
                        incarnation: lifetime,
                    },
                    kind: CanvasHitKind::Overlay,
                    paint_order: walk.entry_index(),
                    layer: placed.layer,
                    bounds,
                    clip,
                    position: placed.position,
                    scale: placed.scale,
                    eligible: true,
                    ancestry: ancestry(&walk.stack, entity),
                });
            }
        }
        let mut children_placement = placed;
        let mut content_placement = placed;
        let mut deferred_paint = Vec::new();
        let mut scroll_bars = Vec::new();
        let mut observation = None;
        let mut interaction = CanvasInteractionPriority::default();
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
            walk.bounds.push((
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
                .and_then(|view| view.control_labels.get(&entity))
                .map_or(
                    crate::systems::gui::presentation::GUI_DEFAULT_FONT_SIZE,
                    |label| label.font_size,
                );
            let scroll = scroll_fields(world, entity, font_size);
            if let Some((extent, offset, bar_style)) = &scroll {
                let offset_origin = mapping.content_offset;
                children_placement.clip = intersect(
                    placed.clip,
                    placed.bounds([
                        offset_origin[0],
                        offset_origin[1],
                        offset_origin[0] + extent.viewport[0],
                        offset_origin[1] + extent.viewport[1],
                    ]),
                );
                let mut obstacles = Vec::new();
                if placed.scale.iter().all(|scale| *scale != 0.0) {
                    for frame in &walk.stack {
                        let ancestor = frame.placed;
                        for bar in &frame.bars {
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
                layout.and_then(|view| view.control_labels.get(&entity)),
                &scroll_bars,
                gui.part_interaction(control.target, painted.interaction),
                |identity| self.state.gui.retained_resource(identity),
            );
            let available =
                placed.available && eligibility.available && control_primitives.is_some();
            let bounds = placed.bounds([0.0, 0.0, mapping.size[0], mapping.size[1]]);
            let ancestry = ancestry(&walk.stack, entity);
            let hit = CanvasHit {
                target,
                kind: CanvasHitKind::Entity,
                paint_order: walk.entry_index(),
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
            interaction.focused = gui.focused(control.target) && hit.eligible;
            interaction.hovered = painted.interaction.hovered && hit.eligible;
            interaction.pressed = painted.interaction.pressed && hit.eligible;
            interaction.captured = painted.interaction.captured && hit.eligible;
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
            let control = self.state.gui.observation(GuiControlObservation {
                text: layout
                    .and_then(|view| view.control_labels.get(&entity))
                    .and_then(|label| Some((label.layout.clone()?, label.origin()))),
                scroll_bars: std::array::from_fn(|axis| {
                    scroll_bars
                        .iter()
                        .find(|bar| bar.axis == axis && bar.enabled())
                        .copied()
                }),
                focus_scope: parent_scope,
                group: gui.group_item(world, control, &ancestry),
                record: crate::systems::gui::presentation::GuiControlRecord {
                    target: control.target,
                    kind: control.kind,
                    value,
                    enabled: eligibility.enabled,
                    visible: eligibility.visible,
                    available: eligibility.available,
                    focusable: crate::systems::gui::local::control::focusable(world, entity),
                    focus_parts: crate::systems::gui::local::control::focus_parts(world, control),
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
                        input.numeric && control.target.component == ComponentValue::GUI_TEXT_INPUT
                    })
                    .map(
                        |input| crate::systems::gui::presentation::GuiNumberGeometry {
                            steps: input.step_parts.then(|| {
                                crate::systems::gui::local::number::number_step_rects(mapping.size)
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
            });
            observation = Some(control.clone());
            walk.controls.push(control);
            walk.hits.push(hit);
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
                        walk.entries.push(self.state.gui.primitive(primitive));
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
                walk.entries.push(self.state.gui.primitive(background));
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
            let override_geometry = layout.and_then(|view| view.geometry.get(&(entity, component)));
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
            walk.layout_changed |= changed_geometry
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
                walk.entries.push(entry.clone());
            }
            self.state.leaves.insert((entity, component), leaf);
        }
        if let Some(slot) =
            attachment_slot(context, entity, content_placement, density, layout_size)
        {
            walk.layout_changed |= !self.state.slots.contains_key(&entity);
            let entry = self
                .state
                .slots
                .entry(entity)
                .or_insert_with(|| Arc::new(CanvasPaintEntry::Attachment(slot.clone())));
            if entry.as_ref() != &CanvasPaintEntry::Attachment(slot.clone()) {
                if let CanvasPaintEntry::Attachment(previous) = entry.as_ref() {
                    walk.layout_changed |= previous.physical_extent != slot.physical_extent
                        || previous.position != slot.position
                        || previous.scale != slot.scale
                        || previous.clip != slot.clip;
                }
                *entry = Arc::new(CanvasPaintEntry::Attachment(slot.clone()));
            }
            let entry = entry.clone();
            walk.hits.push(CanvasHit {
                target: CanvasTarget {
                    entity,
                    component: ComponentValue::SURFACE,
                    incarnation: slot.surface_incarnation,
                },
                kind: CanvasHitKind::Attachment {
                    anchor: entity,
                    token: slot.token.clone(),
                },
                paint_order: walk.entry_index(),
                layer: slot.layer,
                bounds: content_placement.bounds([
                    0.0,
                    0.0,
                    layout_size.map_or(slot.physical_extent[0] as f32 * density, |size| size[0]),
                    layout_size.map_or(slot.physical_extent[1] as f32 * density, |size| size[1]),
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
                ancestry: ancestry(&walk.stack, entity),
            });
            walk.entries.push(entry);
        } else {
            walk.layout_changed |= self.state.slots.remove(&entity).is_some();
        }
        walk.records.push(CanvasWalkRecord {
            entity,
            placed,
            inherited: children_placement,
            bars: scroll_bars.clone(),
            scope,
            inert,
            start,
            end: start,
            subtree_end: position + 1,
            interaction,
        });
        walk.stack.push(CanvasWalkFrame {
            entity,
            position,
            placed,
            inherited: children_placement,
            bars: scroll_bars,
            scope,
            inert,
            deferred: deferred_paint,
            observation,
            seeded: false,
        });
    }
}

/// Root-first logical ancestry of `entity` below the walked `stack`.
fn ancestry(stack: &[CanvasWalkFrame], entity: EntityId) -> Arc<[EntityId]> {
    stack
        .iter()
        .map(|frame| frame.entity)
        .chain([entity])
        .collect::<Vec<_>>()
        .into()
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
